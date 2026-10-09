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
            ctx.check(Capability::Read, &cwd.to_string_lossy())?;
            if !cwd.is_dir() {
                return Err(Error::Tool(format!("dossier inexistant : {}", cwd.display())));
            }

            let mut command = Command::new("powershell.exe");
            command
                .args(["-NoProfile", "-NonInteractive", "-Command"])
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