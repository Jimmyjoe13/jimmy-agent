//! Exécution de commandes sur la machine.
//!
//! La commande est confiée à PowerShell : c'est l'interpréteur natif de la
//! machine cible, il gère nativement les chemins Windows, et il permet à Jimmy
//! d'utiliser aussi bien `git`, `npm`, `cargo` que les cmdlets.
//!
//! La vérification de permission se fait sur la ligne de commande **avant**
//! toute exécution. C'est le point de contrôle principal : tout ce qui passe
//! par ici est observé et filtré.

use std::process::Stdio;

use tokio::process::Command;

use super::{arg_str, arg_u64, clip, schema, BoxFuture, Tool, ToolContext, MAX_TOOL_OUTPUT};
use crate::error::{Error, Result};
use crate::permissions::Capability;

/// Racine d'un profil utilisateur : `~` nu, variable de profil nue,
/// `C:\Users\<nom>` avec au plus un séparateur final. Un sous-dossier ciblé
/// (`C:\Users\x\.claude`) n'est PAS une racine : il reste autorisé.
fn is_home_root(text: &str) -> bool {
    let t = text.trim().trim_matches(['"', '\'']).to_lowercase();
    if matches!(t.as_str(), "~" | "~/" | "$env:userprofile" | "%userprofile%" | "$home") {
        return true;
    }
    if let Some(rest) = t.strip_prefix("c:\\users\\").or_else(|| t.strip_prefix("c:/users/")) {
        let tail = rest.trim_matches(['\\', '/']);
        // Vide (`C:\Users\` nu) ou simple nom (`jimmy`) : la maison entière.
        // Un cran de plus (`jimmy\.claude`) : ciblé, autorisé.
        if tail.is_empty() || (!tail.contains(['\\', '/']) && !tail.contains(' ')) {
            return true;
        }
    }
    false
}

/// Ligne de commande qui vide tout l'environnement (`Get-ChildItem Env:`,
/// `printenv` nu, `set` nu) : les clés qui en sortent partent au modèle et
/// bloquent l'envoi suivant (cas réel du 13 octobre, règle 8 du prompt).
fn is_env_dump(command: &str) -> bool {
    for part in command.split([';', '&', '|']) {
        let words: Vec<&str> = part.split_whitespace().collect();
        let first_two: Vec<String> = words.iter().take(2).map(|w| w.to_lowercase().trim_matches(['"', '\'']).to_string()).collect();
        match first_two.as_slice() {
            [a, b] if (a == "get-childitem" || a == "gci" || a == "dir") && b == "env:" => return true,
            [a] if a == "printenv" || a == "set" => return true,
            _ => {}
        }
    }
    false
}

/// Racine d'un balayage `**` : le texte après le dernier guillemet qui
/// précède le premier `**` (ex. `C:\Users\x\` dans `glob('C:\Users\x\**…')`).
fn glob_root(command: &str) -> Option<String> {
    let before = command.split("**").next()?;
    let token = before
        .rsplit(['\'', '"'])
        .next()
        .or_else(|| before.split_whitespace().last())?;
    Some(token.trim_matches(['\'', '"', '(', '=']).to_string())
}

/// Refuse une énumération maison : balayage récursif (`-Recurse`, `**`)
/// ancré à la racine du profil, ou vidage de l'environnement (cas réels du
/// 13 octobre : glob `**/*mcp*.json` récursif → timeout de 120 s ; dump
/// `Env:` → envoi bloqué). `Some(conseil)` = refusé, `None` = autorisé.
pub fn home_wide_scan(command: &str) -> Option<String> {
    if is_env_dump(command) {
        return Some(
            "énumération de l'environnement refusée : les clés qui en sortent partent au modèle. \
             Lis le fichier nommé (`read_file` sur le `.env`), valeurs masquées à l'affichage."
                .to_string(),
        );
    }
    let low = command.to_lowercase();
    let recursive = low.contains("-recurse") || low.contains("recursive=true") || command.contains("**");
    if !recursive {
        return None;
    }
    // Balayage `**` : seule sa racine compte (un projet ciblé reste libre).
    if command.contains("**") {
        if let Some(root) = glob_root(command) {
            if is_home_root(&root) {
                return Some(
                    "balayage récursif refusé : il gèle la conversation (mesuré : 120 s de silence). \
                     Vise un sous-dossier précis ou utilise `search_files` avec la racine du projet."
                        .to_string(),
                );
            }
        }
        return None;
    }
    // `-Recurse` PowerShell : un argument chemin à la racine du profil = refus ;
    // `-Recurse` nu (dossier de travail) ou sous-dossier ciblé = autorisé.
    for token in low.split_whitespace() {
        let clean = token.trim_matches(['"', '\'', ',', ';', '(', ')']);
        if clean.contains('\\') || clean.contains('/') || clean.starts_with('~') || clean.starts_with("$env") || clean.contains('%') {
            if is_home_root(clean) {
                return Some(
                    "balayage récursif refusé : il gèle la conversation (mesuré : 120 s de silence). \
                     Vise un sous-dossier précis ou utilise `search_files` avec la racine du projet."
                        .to_string(),
                );
            }
        }
    }
    None
}

pub struct RunCommand;

impl Tool for RunCommand {
    fn name(&self) -> &str {
        "run_command"
    }
    fn description(&self) -> &str {
        "Exécute une commande dans le terminal Windows (PowerShell) et renvoie stdout, stderr et le code de sortie. Utilise cet outil pour compiler, tester, versionner ou inspecter. La commande est DÉJÀ exécutée par PowerShell : ne l'enveloppe jamais dans « powershell -Command \"…\" » (les variables $ seraient vidées). Pour lire un fichier, préfère read_file."
    }
    fn parameters(&self) -> serde_json::Value {
        schema(
            serde_json::json!({
                "command": {"type": "string", "description": "Ligne de commande à exécuter, ex. « git status --short »."},
                "cwd": {"type": "string", "description": "Dossier de travail. Par défaut le dossier de travail de Jimy."},
                "timeout_ms": {"type": "integer", "description": "Durée maximale, en millisecondes (défaut 120000)."}
            }),
            &["command"],
        )
    }
    fn capability(&self) -> Capability {
        Capability::Execute
    }
    fn call<'a>(
        &'a self,
        args: &'a serde_json::Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            let command_line = arg_str(args, "command")
                .ok_or_else(|| Error::Tool("« command » manquant".into()))?;
            let cwd = ctx.resolve(&arg_str(args, "cwd").unwrap_or_else(|| ".".into()));
            let timeout_ms = arg_u64(args, "timeout_ms", 120_000).min(600_000);

            ctx.check(Capability::Execute, &command_line)?;
            // Balayage maison ou vidage d'environnement (13 octobre) : refusé
            // avant exécution, avec la méthode à suivre.
            if let Some(conseil) = home_wide_scan(&command_line) {
                return Err(Error::Tool(format!("REFUSÉ : {conseil}")));
            }
            ctx.check(Capability::Read, &cwd.to_string_lossy())?;
            if !cwd.is_dir() {
                return Err(Error::Tool(format!("dossier inexistant : {}", cwd.display())));
            }

            // Interpréteur natif de la machine (lot 4) : PowerShell sur
            // Windows, `zsh`/`sh` ailleurs — plus de `powershell.exe` en dur.
            let (shell, shell_args) = crate::os::shell();
            let mut command = Command::new(shell);
            command
                .args(shell_args)
                .arg(&command_line)
                .current_dir(&cwd)
                .stdin(Stdio::null())
                .kill_on_drop(true);
            #[cfg(windows)]
            {
                command.creation_flags(0x08000000); // CREATE_NO_WINDOW
            }

            let started = std::time::Instant::now();
            let output = tokio::time::timeout(
                std::time::Duration::from_millis(timeout_ms),
                command.output(),
            )
            .await
            .map_err(|_| Error::Timeout)?
            .map_err(|e| Error::Tool(format!("exécution impossible : {e}")))?;

            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let code = output.status.code().unwrap_or(-1);
            let elapsed = started.elapsed().as_millis();

            let mut report = format!("code de sortie : {code} ({elapsed} ms)\n");
            if !stdout.trim().is_empty() {
                report.push_str("\n--- stdout ---\n");
                report.push_str(stdout.trim_end());
            }
            if !stderr.trim().is_empty() {
                report.push_str("\n--- stderr ---\n");
                report.push_str(stderr.trim_end());
            }
            if stdout.trim().is_empty() && stderr.trim().is_empty() {
                report.push_str("\n(aucune sortie)");
            }
            if code != 0 {
                report.push_str("\n\nLa commande a échoué : utilise cette information pour corriger et réessayer.");
            }
            Ok(clip(&report, MAX_TOOL_OUTPUT))
        })
    }
}

#[cfg(test)]
mod home_scan_tests {
    use super::*;

    /// Cas réels du 13 octobre : glob récursif maison (timeout 120 s) et dump
    /// `Env:` (envoi bloqué ensuite). Les lectures ciblées restent libres.
    #[test]
    fn balayage_maison_et_dump_env_sont_refuses() {
        // Tués dans l'œuf : glob maison, -Recurse maison, dump d'environnement.
        assert!(home_wide_scan("python -c \"import glob; glob.glob(r'C:\\Users\\jimmy\\**\\*mcp*.json', recursive=True)\"").is_some());
        assert!(home_wide_scan("Get-ChildItem \"$env:USERPROFILE\" -Filter \"*mcp*.json\" -Recurse -ErrorAction SilentlyContinue").is_some());
        assert!(home_wide_scan("echo \"=== ENV ===\"; Get-ChildItem Env: | Where-Object { $_.Name -match \"API\" }").is_some());
        assert!(home_wide_scan("printenv").is_some());
        // Libres : sous-dossier ciblé, projet cerné, commandes ordinaires.
        assert_eq!(home_wide_scan("Get-ChildItem \"$env:USERPROFILE\\.claude\" -File | Select-Object Name"), None);
        assert_eq!(home_wide_scan("dir \"%APPDATA%\\Claude\""), None);
        assert_eq!(home_wide_scan("python -c \"import glob; glob.glob(r'C:\\Projet\\x\\**\\*.py', recursive=True)\""), None);
        assert_eq!(home_wide_scan("Get-ChildItem -Force C:\\Projet\\x -Recurse | Select-Object Name"), None);
        assert_eq!(home_wide_scan("git status --short"), None);
        assert_eq!(home_wide_scan("Get-Content .env | Select-String API"), None);
    }
}