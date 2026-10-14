//! Contrôle machine en lecture seule (lot 4, tranche 1) : fiche système et
//! liste des processus, sans shell interpolé et sans ligne de commande
//! (leurs arguments peuvent contenir des secrets, piège 108 : seuls le PID
//! et le nom de l'image remontent).

use super::{arg_str, arg_u64, clip, schema, BoxFuture, Tool, ToolContext, MAX_TOOL_OUTPUT};
use crate::error::{Error, Result};
use crate::permissions::Capability;

pub struct SystemInfo;

impl Tool for SystemInfo {
    fn name(&self) -> &str {
        "system_info"
    }
    fn description(&self) -> &str {
        "Décrit la machine pilotée (système, architecture, interpréteur, cœurs). \
         À appeler avant toute commande qui dépend de l'OS, au lieu de le deviner."
    }
    fn parameters(&self) -> serde_json::Value {
        schema(serde_json::json!({}), &[])
    }
    fn capability(&self) -> Capability {
        Capability::Read
    }
    fn call<'a>(
        &'a self,
        _args: &'a serde_json::Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            ctx.check_granted(Capability::Read, "système")?;
            Ok(crate::os::describe())
        })
    }
}

pub struct ProcessList;

impl Tool for ProcessList {
    fn name(&self) -> &str {
        "process_list"
    }
    fn description(&self) -> &str {
        "Liste les processus (PID + nom de l'image), filtrés par sous-chaîne insensible à la casse. \
         Sans les lignes de commande : elles peuvent contenir des secrets. Préférer cet outil à un `tasklist`/`ps` manuel."
    }
    fn parameters(&self) -> serde_json::Value {
        schema(
            serde_json::json!({
                "filter": {"type": "string", "description": "Sous-chaîne filtrant le nom (ex. « godot »). Vide = tout."},
                "limit": {"type": "integer", "description": "Lignes au plus (défaut 200)."}
            }),
            &[],
        )
    }
    fn capability(&self) -> Capability {
        Capability::Read
    }
    fn call<'a>(
        &'a self,
        args: &'a serde_json::Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            ctx.check_granted(Capability::Read, "processus")?;
            let filter = arg_str(args, "filter").unwrap_or_default().to_lowercase();
            let limit = arg_u64(args, "limit", 200).clamp(1, 1000) as usize;
            let listed = list_processes().await?;
            Ok(format_processes(&listed, &filter, limit))
        })
    }
}

/// Un processus : PID + nom d'image (jamais sa ligne de commande).
pub struct ProcEntry {
    pub pid: u32,
    pub name: String,
}

async fn list_processes() -> Result<Vec<ProcEntry>> {
    let output = if std::env::consts::OS == "windows" {
        // Sans `/V` : avec, les lignes de commande (et leurs secrets) sortiraient.
        tokio::process::Command::new("tasklist.exe")
            .args(["/FO", "CSV", "/NH"])
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true)
            .output()
            .await
    } else {
        // `comm` seul, sans `args` : même raison.
        tokio::process::Command::new("ps")
            .args(["-e", "-o", "pid=,comm="])
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true)
            .output()
            .await
    }
    .map_err(|e| Error::Tool(format!("liste des processus impossible : {e}")))?;
    if !output.status.success() {
        return Err(Error::Tool("liste des processus en échec".into()));
    }
    let text = String::from_utf8_lossy(&output.stdout).to_string();
    Ok(if std::env::consts::OS == "windows" { parse_tasklist(&text) } else { parse_ps(&text) })
}

/// `tasklist /FO CSV /NH` : `"nom","pid",…` — on ne garde que les 2 premiers.
fn parse_tasklist(text: &str) -> Vec<ProcEntry> {
    let mut out = Vec::new();
    for line in text.lines() {
        let mut fields = Vec::new();
        let mut field = String::new();
        let mut quoted = false;
        for c in line.chars() {
            match c {
                '"' => quoted = !quoted,
                ',' if !quoted => {
                    fields.push(std::mem::take(&mut field));
                }
                _ => field.push(c),
            }
        }
        fields.push(field);
        if fields.len() >= 2 {
            if let Ok(pid) = fields[1].trim().parse::<u32>() {
                let name = fields[0].trim().to_string();
                if !name.is_empty() {
                    out.push(ProcEntry { pid, name });
                }
            }
        }
    }
    out
}

/// `ps -e -o pid=,comm=` : `PID NOM`, en-têtes supprimés par `=`.
fn parse_ps(text: &str) -> Vec<ProcEntry> {
    let mut out = Vec::new();
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        if let (Some(pid), Some(name)) = (parts.next(), parts.next()) {
            if let Ok(pid) = pid.parse::<u32>() {
                out.push(ProcEntry { pid, name: name.to_string() });
            }
        }
    }
    out
}

fn format_processes(listed: &[ProcEntry], filter: &str, limit: usize) -> String {
    let sought = filter.to_lowercase();
    let kept: Vec<&ProcEntry> = listed
        .iter()
        .filter(|p| sought.is_empty() || p.name.to_lowercase().contains(&sought))
        .take(limit)
        .collect();
    let mut report = format!("{} processus (filtre : « {} »)", kept.len(), if sought.is_empty() { "aucun" } else { filter });
    for p in kept {
        report.push_str(&format!("\n{} {}", p.pid, p.name));
    }
    clip(&report, MAX_TOOL_OUTPUT)
}

/// Extensions qui exécutent du code quand on les « ouvre » : même liste que
/// l'explorateur (`desktop/.../explorer.rs`), plus paquets et scripts des
/// autres OS. Un exécutable ne s'ouvre jamais, il se montre dans
/// l'Explorateur (règle héritée de l'explorateur de fichiers).
const NO_OPEN_EXTENSIONS: &[&str] = &[
    "exe", "com", "bat", "cmd", "ps1", "psm1", "vbs", "vbe", "js", "jse", "wsf", "wsh", "msi", "msp",
    "scr", "lnk", "hta", "cpl", "reg", "jar", "pif", "appref-ms", "sh", "command", "app", "run", "bin",
    "dmg", "pkg",
];

/// Ce chemin peut-il s'ouvrir dans son application par défaut ?
pub fn is_openable(path: &std::path::Path) -> bool {
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    // Un paquet macOS est un dossier : il lance une app, il ne s'ouvre pas.
    if ext == "app" {
        return false;
    }
    !NO_OPEN_EXTENSIONS.contains(&ext.as_str())
}

pub struct OpenPath;

impl Tool for OpenPath {
    fn name(&self) -> &str {
        "open_path"
    }
    fn description(&self) -> &str {
        "Ouvre un fichier dans son application par défaut (jamais un exécutable : refusé, à montrer dans l'Explorateur), ou un dossier dans l'Explorateur."
    }
    fn parameters(&self) -> serde_json::Value {
        schema(
            serde_json::json!({
                "path": {"type": "string", "description": "Fichier ou dossier à ouvrir."}
            }),
            &["path"],
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
            let path = arg_str(args, "path").ok_or_else(|| Error::Tool("« path » manquant".into()))?;
            let target = ctx.resolve(&path);
            ctx.check(Capability::Read, &target.to_string_lossy())?;
            ctx.check(Capability::Execute, &format!("open {}", target.display()))?;
            if target.is_file() && !is_openable(&target) {
                return Err(Error::Tool(format!(
                    "REFUSÉ : un exécutable ne s'ouvre jamais ({}). Montre-le dans l'Explorateur.",
                    target.display()
                )));
            }
            if !target.exists() {
                return Err(Error::Tool(format!("introuvable : {}", target.display())));
            }
            // Application associée pour un fichier, Explorateur (ou Finder)
            // pour un dossier — sans attendre (même recette que `fs_open`
            // côté interface). Programme fixe selon l'OS, chemin validé :
            // aucune interpolation de texte du modèle.
            if std::env::consts::OS == "windows" {
                std::process::Command::new("explorer.exe").arg(&target).stdin(std::process::Stdio::null()).spawn().map_err(|e| Error::Tool(format!("ouverture impossible : {e}")))?;
            } else {
                std::process::Command::new("open").arg(&target).stdin(std::process::Stdio::null()).spawn().map_err(|e| Error::Tool(format!("ouverture impossible : {e}")))?;
            }
            Ok(format!("ouvert : {}", target.display()))
        })
    }
}

/// Ce PID peut-il être arrêté ? Jimmy lui-même et le système, jamais.
pub fn kill_refused(pid: u32, own: u32) -> Option<String> {
    if pid == own {
        return Some("REFUSÉ : Jimmy ne s'arrête pas lui-même.".into());
    }
    if pid <= 4 {
        return Some(format!("REFUSÉ : le PID {pid} appartient au système."));
    }
    None
}

pub struct ProcessKill;

impl Tool for ProcessKill {
    fn name(&self) -> &str {
        "process_kill"
    }
    fn description(&self) -> &str {
        "Arrête un processus par son PID (voir `process_list`). Demande toujours l'accord de l'utilisateur (carte) : arrêter un programme peut faire perdre du travail. Doux par défaut, `force` en dernier recours."
    }
    fn parameters(&self) -> serde_json::Value {
        schema(
            serde_json::json!({
                "pid": {"type": "integer", "description": "PID à arrêter."},
                "force": {"type": "boolean", "description": "Forcer l'arrêt (défaut false)."}
            }),
            &["pid"],
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
            let pid = args.get("pid").and_then(|v| v.as_u64()).ok_or_else(|| Error::Tool("« pid » manquant".into()))? as u32;
            let force = args.get("force").and_then(|v| v.as_bool()).unwrap_or(false);
            if let Some(refus) = kill_refused(pid, std::process::id()) {
                return Err(Error::Tool(refus));
            }
            ctx.check(Capability::Execute, &format!("process_kill {pid}"))?;
            // PID validé (entier) : aucune interpolation de texte du modèle.
            let mut command = if std::env::consts::OS == "windows" {
                let mut c = tokio::process::Command::new("taskkill.exe");
                c.arg("/PID").arg(pid.to_string());
                if force {
                    c.arg("/F");
                }
                c
            } else {
                let mut c = tokio::process::Command::new("kill");
                if force {
                    c.arg("-9");
                }
                c.arg(pid.to_string());
                c
            };
            let output = tokio::time::timeout(
                std::time::Duration::from_secs(15),
                command.stdin(std::process::Stdio::null()).kill_on_drop(true).output(),
            )
            .await
            .map_err(|_| Error::Timeout)?
            .map_err(|e| Error::Tool(format!("arrêt impossible : {e}")))?;
            let code = output.status.code().unwrap_or(-1);
            if output.status.success() {
                Ok(format!("processus {pid} arrêté (code {code})"))
            } else {
                Err(Error::Tool(format!(
                    "arrêt refusé par le système (code {code}) : {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                )))
            }
        })
    }
}

/// Une fenêtre : PID, application, titre (vide = pas une vraie fenêtre).
pub struct WinEntry {
    pub pid: u32,
    pub app: String,
    pub title: String,
}

/// Sortie PowerShell (`Id`, tabulation, nom, tabulation, titre) : les lignes
/// sans titre ne sont pas des fenêtres.
fn parse_window_list_win(text: &str) -> Vec<WinEntry> {
    let mut out = Vec::new();
    for line in text.lines() {
        let mut parts = line.split('\t');
        if let (Some(pid), Some(app), Some(title)) = (parts.next(), parts.next(), parts.next()) {
            let title = title.trim().to_string();
            if title.is_empty() {
                continue;
            }
            if let Ok(pid) = pid.trim().parse::<u32>() {
                out.push(WinEntry { pid, app: app.trim().to_string(), title });
            }
        }
    }
    out
}

/// Sortie `osascript` (`pid, nom` par ligne, macOS, non vérifié sans Mac).
fn parse_window_list_mac(text: &str) -> Vec<WinEntry> {
    let mut out = Vec::new();
    for line in text.lines() {
        let mut parts = line.splitn(2, ',');
        if let (Some(pid), Some(app)) = (parts.next(), parts.next()) {
            let app = app.trim().to_string();
            if app.is_empty() {
                continue;
            }
            if let Ok(pid) = pid.trim().parse::<u32>() {
                out.push(WinEntry { pid, app: app.clone(), title: app });
            }
        }
    }
    out
}

fn format_windows(listed: &[WinEntry], limit: usize) -> String {
    let kept: Vec<&WinEntry> = listed.iter().take(limit).collect();
    let mut report = format!("{} fenêtre(s)", kept.len());
    for w in kept {
        let title: String = w.title.chars().take(80).collect();
        report.push_str(&format!("\n{} {} — {title}", w.pid, w.app));
    }
    clip(&report, MAX_TOOL_OUTPUT)
}

pub struct WindowList;

impl Tool for WindowList {
    fn name(&self) -> &str {
        "window_list"
    }
    fn description(&self) -> &str {
        "Liste les fenêtres visibles (PID, application, titre), sans prendre le focus. \
         Pour lire avant `window_focus`."
    }
    fn parameters(&self) -> serde_json::Value {
        schema(
            serde_json::json!({
                "limit": {"type": "integer", "description": "Fenêtres au plus (défaut 100)."}
            }),
            &[],
        )
    }
    fn capability(&self) -> Capability {
        Capability::Read
    }
    fn call<'a>(
        &'a self,
        args: &'a serde_json::Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            ctx.check_granted(Capability::Read, "fenêtres")?;
            let limit = arg_u64(args, "limit", 100).clamp(1, 500) as usize;
            // Scripts fixes, aucune entrée du modèle : pas d'injection.
            let output = if std::env::consts::OS == "windows" {
                tokio::process::Command::new("powershell.exe")
                    .args(["-NoProfile", "-NonInteractive", "-Command",
                        "Get-Process | Where-Object { $_.MainWindowTitle } | ForEach-Object { \"$($_.Id)`t$($_.ProcessName)`t$($_.MainWindowTitle)\" }"])
                    .stdin(std::process::Stdio::null())
                    .kill_on_drop(true)
                    .output()
                    .await
            } else {
                tokio::process::Command::new("osascript")
                    .args(["-e", "tell application \"System Events\" to get {unix id, name} of (every process whose background only is false)"])
                    .stdin(std::process::Stdio::null())
                    .kill_on_drop(true)
                    .output()
                    .await
            }
            .map_err(|e| Error::Tool(format!("liste des fenêtres impossible : {e}")))?;
            if !output.status.success() {
                return Err(Error::Tool("liste des fenêtres en échec".into()));
            }
            let text = String::from_utf8_lossy(&output.stdout).to_string();
            let listed = if std::env::consts::OS == "windows" { parse_window_list_win(&text) } else { parse_window_list_mac(&text) };
            Ok(format_windows(&listed, limit))
        })
    }
}

pub struct WindowFocus;

impl Tool for WindowFocus {
    fn name(&self) -> &str {
        "window_focus"
    }
    fn description(&self) -> &str {
        "Amène une fenêtre au premier plan par son PID (voir `window_list`). \
         Dérange l'utilisateur : à n'utiliser que sur sa demande ou en fin de tâche qu'il attend."
    }
    fn parameters(&self) -> serde_json::Value {
        schema(
            serde_json::json!({
                "pid": {"type": "integer", "description": "PID de la fenêtre (voir `window_list`)."}
            }),
            &["pid"],
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
            let pid = args.get("pid").and_then(|v| v.as_u64()).ok_or_else(|| Error::Tool("« pid » manquant".into()))? as u32;
            ctx.check(Capability::Execute, &format!("window_focus {pid}"))?;
            // PID validé (entier) : aucune interpolation de texte du modèle.
            let ok = if std::env::consts::OS == "windows" {
                let script = format!("(New-Object -ComObject WScript.Shell).AppActivate({pid})");
                let output = tokio::process::Command::new("powershell.exe")
                    .args(["-NoProfile", "-NonInteractive", "-Command", &script])
                    .stdin(std::process::Stdio::null())
                    .kill_on_drop(true)
                    .output()
                    .await
                    .map_err(|e| Error::Tool(format!("focus impossible : {e}")))?;
                String::from_utf8_lossy(&output.stdout).to_lowercase().contains("true")
            } else {
                let script = format!("tell application \"System Events\" to set frontmost of (first process whose unix id is {pid}) to true");
                let output = tokio::process::Command::new("osascript")
                    .args(["-e", &script])
                    .stdin(std::process::Stdio::null())
                    .kill_on_drop(true)
                    .output()
                    .await
                    .map_err(|e| Error::Tool(format!("focus impossible : {e}")))?;
                output.status.success()
            };
            if ok {
                Ok(format!("fenêtre du processus {pid} au premier plan"))
            } else {
                Err(Error::Tool(format!("processus {pid} introuvable ou sans fenêtre")))
            }
        })
    }
}

pub struct AppOpen;

impl Tool for AppOpen {
    fn name(&self) -> &str {
        "app_open"
    }
    fn description(&self) -> &str {
        "Lance une application par son nom (« notepad », « Calculator ») dans sa version du système. \
         Pour un fichier, `open_path` ; pour une commande, `run_command`."
    }
    fn parameters(&self) -> serde_json::Value {
        schema(
            serde_json::json!({
                "name": {"type": "string", "description": "Nom de l'application."}
            }),
            &["name"],
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
            let name = arg_str(args, "name").ok_or_else(|| Error::Tool("« name » manquant".into()))?;
            ctx.check(Capability::Execute, &format!("app_open {name}"))?;
            // Un seul argument quoted : le nom ne peut pas déborder en commande.
            if std::env::consts::OS == "windows" {
                tokio::process::Command::new("cmd")
                    .args(["/D", "/C", "start", "", &name])
                    .stdin(std::process::Stdio::null())
                    .kill_on_drop(true)
                    .output()
                    .await
                    .map_err(|e| Error::Tool(format!("ouverture impossible : {e}")))?;
            } else {
                std::process::Command::new("open")
                    .arg("-a")
                    .arg(&name)
                    .stdin(std::process::Stdio::null())
                    .spawn()
                    .map_err(|e| Error::Tool(format!("ouverture impossible : {e}")))?;
            }
            Ok(format!("application lancée : {name}"))
        })
    }
}

#[cfg(test)]
mod sys_tests {
    use super::*;

    /// Formats réels : CSV de `tasklist`, lignes de `ps`. Seuls PID et nom
    /// sortent ; une virgule dans un nom ne casse pas le découpage.
    #[test]
    fn les_listes_sont_lues_sans_les_lignes_de_commande() {
        let csv = "\"System Idle Process\",\"0\",\"Services\",\"0\",\"8 Ko\"\r\n\"Godot_v4.5, test.exe\",\"16432\",\"Console\",\"1\",\"12 000 Ko\"";
        let procs = parse_tasklist(csv);
        assert_eq!(procs.len(), 2);
        assert_eq!(procs[1].pid, 16432);
        assert!(procs[1].name.contains("Godot"), "{}", procs[1].name);
        let ps = "    1 launchd\n  422 cupsd\n";
        let procs = parse_ps(ps);
        assert_eq!(procs.len(), 2);
        assert_eq!(procs[0].pid, 1);
        // Filtre insensible à la casse, borne respectée.
        let texte = format_processes(&procs, "CUPSD", 10);
        assert!(texte.contains("422") && !texte.contains("launchd"), "{texte}");
        let texte = format_processes(&procs, "", 1);
        assert!(texte.contains("1 processus"), "{texte}");
    }

    /// Les deux outils se déclarent comme de la lecture (rappels d'exploration
    /// : inspecter n'est pas agir).
    #[test]
    fn fiche_et_processus_sont_des_outils_de_lecture() {
        assert_eq!(SystemInfo.spec().name, "system_info");
        assert_eq!(ProcessList.spec().name, "process_list");
        assert_eq!(SystemInfo.capability(), Capability::Read);
        assert_eq!(ProcessList.capability(), Capability::Read);
    }

    /// Lot 4, tranche 2b : listes de fenêtres lues sans prendre le focus
    /// (voler le focus en test est interdit) — fixtures des deux OS.
    #[test]
    fn les_fenetres_sont_lues_sans_prendre_le_focus() {
        let win = "1234\tnotepad\tSans titre - Bloc-notes\n5678\tchrome\tJimy - Google Chrome\n910\tidle\t\n";
        let fenetres = parse_window_list_win(win);
        assert_eq!(fenetres.len(), 2, "une fenêtre sans titre n'en est pas une");
        assert_eq!(fenetres[0].pid, 1234);
        assert!(fenetres[1].title.contains("Chrome"), "{}", fenetres[1].title);
        let mac = "1234, Safari\n5678, Terminal\n";
        let fenetres = parse_window_list_mac(mac);
        assert_eq!(fenetres.len(), 2);
        assert_eq!(fenetres[0].app, "Safari");
        let texte = format_windows(&fenetres, 10);
        assert!(texte.contains("2 fenêtre") && texte.contains("Terminal"), "{texte}");
    }

    /// Lot 4, tranche 2 : un exécutable ne s'ouvre jamais (même liste que
    /// l'explorateur, plus les installateurs/paquets macOS) ; Jimmy
    /// lui-même et le système ne s'arrêtent jamais.
    #[test]
    fn executables_et_processus_proteges_sont_refuses() {
        for ouvert in ["setup.EXE", "build.ps1", "raccourci.lnk", "paquet.pkg", "image.dmg", "appli.app"] {
            assert!(!is_openable(std::path::Path::new(ouvert)), "{ouvert} ne doit jamais s'ouvrir");
        }
        for doc in ["README.md", "photo.png", "dossier"] {
            assert!(is_openable(std::path::Path::new(doc)), "{doc} doit s'ouvrir");
        }
        let moi = std::process::id();
        assert!(kill_refused(moi, moi).is_some(), "Jimmy ne s'arrête pas lui-même");
        for protege in [0, 1, 4] {
            assert!(kill_refused(protege, moi).is_some(), "PID {protege} protégé");
        }
        assert_eq!(kill_refused(1234, moi), None);
    }
}
