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

/// Ce PID ou ce nom de processus peut-il être arrêté ? Jimmy lui-même, son
/// avatar Godot, ses serveurs vocaux et le système, jamais (cas réel du
/// 14 octobre : « ferme toutes les fenêtres » a tué l'avatar Godot).
pub fn kill_refused(pid: u32, own: u32) -> Option<String> {
    if pid == own {
        return Some("REFUSÉ : Jimmy ne s'arrête pas lui-même.".into());
    }
    if pid <= 4 {
        return Some(format!("REFUSÉ : le PID {pid} appartient au système."));
    }
    None
}

/// Le nom d'image appartient-il aux processus vitaux de Jimy (avatar Godot,
/// serveurs Whisper, binaire principal) ?
pub fn is_protected_process_name(name: &str) -> bool {
    let low = name.trim().to_lowercase();
    let stem = low.rsplit(['\\', '/']).next().unwrap_or(&low).trim_end_matches(".exe");
    stem == "jimmy"
        || stem == "jimy"
        || stem.starts_with("godot")
        || stem.starts_with("whisper-server")
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
            if let Ok(procs) = list_processes().await {
                if let Some(found) = procs.iter().find(|p| p.pid == pid) {
                    if is_protected_process_name(&found.name) {
                        return Err(Error::Tool(format!(
                            "REFUSÉ : le processus {} ({pid}) appartient à Jimy (avatar ou voix) et ne doit pas être arrêté.",
                            found.name
                        )));
                    }
                }
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

/// La fenêtre appartient-elle à Jimy ou à son avatar Godot ? Écartée de
/// `window_list` (comme dans `screen::pick`) pour que « ferme toutes les
/// fenêtres » ne cible jamais l'avatar ni la fenêtre de Jimy.
pub fn is_own_window(w: &WinEntry, own_pid: u32) -> bool {
    if w.pid == own_pid {
        return true;
    }
    let app = w.app.trim().to_lowercase();
    let title = w.title.trim().to_lowercase();
    app.contains("godot")
        || app == "jimmy"
        || app == "jimy"
        || title == "jimy avatar"
        || title == "jimmy avatar"
}

/// Sortie PowerShell (`Id`, tabulation, nom, tabulation, titre) : les lignes
/// sans titre et les fenêtres propres à Jimy/Godot sont écartées.
fn parse_window_list_win(text: &str) -> Vec<WinEntry> {
    let own = std::process::id();
    let mut out = Vec::new();
    for line in text.lines() {
        let mut parts = line.split('\t');
        if let (Some(pid), Some(app), Some(title)) = (parts.next(), parts.next(), parts.next()) {
            let title = title.trim().to_string();
            if title.is_empty() {
                continue;
            }
            if let Ok(pid) = pid.trim().parse::<u32>() {
                let entry = WinEntry { pid, app: app.trim().to_string(), title };
                if !is_own_window(&entry, own) {
                    out.push(entry);
                }
            }
        }
    }
    out
}

/// Sortie `osascript` (`pid, nom` par ligne, macOS, non vérifié sans Mac).
fn parse_window_list_mac(text: &str) -> Vec<WinEntry> {
    let own = std::process::id();
    let mut out = Vec::new();
    for line in text.lines() {
        let mut parts = line.splitn(2, ',');
        if let (Some(pid), Some(app)) = (parts.next(), parts.next()) {
            let app = app.trim().to_string();
            if app.is_empty() {
                continue;
            }
            if let Ok(pid) = pid.trim().parse::<u32>() {
                let entry = WinEntry { pid, app: app.clone(), title: app };
                if !is_own_window(&entry, own) {
                    out.push(entry);
                }
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

/// Délai par défaut avant extinction ou redémarrage (en secondes) : laisse à
/// l'utilisateur le temps de dire « STOP » ou d'annuler (`cancel`).
pub const DEFAULT_POWER_DELAY_SECS: u64 = 15;

/// Construit la commande système fixe (programme + arguments + résumé) pour
/// `system_power`, sans aucune interpolation de texte libre du modèle.
pub fn power_command(os: &str, action: &str, delay_secs: u64) -> Result<(&'static str, Vec<String>, String)> {
    let kind = match action.trim().to_lowercase().as_str() {
        "shutdown" | "poweroff" | "halt" => "shutdown",
        "restart" | "reboot" => "restart",
        "sleep" | "suspend" => "sleep",
        "cancel" | "abort" => "cancel",
        other => {
            return Err(Error::Tool(format!(
                "action inconnue « {other} » — attendues : shutdown, restart, sleep, cancel"
            )))
        }
    };
    let delay = delay_secs.clamp(5, 3600);
    match (os, kind) {
        ("windows", "shutdown") => Ok((
            "shutdown.exe",
            vec!["/s".into(), "/t".into(), delay.to_string()],
            format!("extinction de l'ordinateur programmée dans {delay} s (`system_power` avec `action: \"cancel\"` pour annuler)"),
        )),
        ("windows", "restart") => Ok((
            "shutdown.exe",
            vec!["/r".into(), "/t".into(), delay.to_string()],
            format!("redémarrage de l'ordinateur programmé dans {delay} s (`system_power` avec `action: \"cancel\"` pour annuler)"),
        )),
        ("windows", "cancel") => Ok((
            "shutdown.exe",
            vec!["/a".into()],
            "extinction ou redémarrage annulé".into(),
        )),
        ("windows", "sleep") => Ok((
            "rundll32.exe",
            vec!["powrprof.dll,SetSuspendState".into(), "0,1,0".into()],
            "mise en veille de l'ordinateur demandée".into(),
        )),
        ("macos", "shutdown") => Ok((
            "shutdown",
            vec!["-h".into(), "+1".into()],
            "extinction de l'ordinateur programmée dans 1 min".into(),
        )),
        ("macos", "restart") => Ok((
            "shutdown",
            vec!["-r".into(), "+1".into()],
            "redémarrage de l'ordinateur programmé dans 1 min".into(),
        )),
        ("macos", "cancel") => Ok((
            "killall",
            vec!["shutdown".into()],
            "extinction ou redémarrage annulé".into(),
        )),
        ("macos", "sleep") => Ok((
            "pmset",
            vec!["sleepnow".into()],
            "mise en veille de l'ordinateur demandée".into(),
        )),
        (_, "shutdown") => Ok((
            "shutdown",
            vec!["-h".into(), "+1".into()],
            "extinction de l'ordinateur programmée dans 1 min".into(),
        )),
        (_, "restart") => Ok((
            "shutdown",
            vec!["-r".into(), "+1".into()],
            "redémarrage de l'ordinateur programmé dans 1 min".into(),
        )),
        (_, "cancel") => Ok((
            "shutdown",
            vec!["-c".into()],
            "extinction ou redémarrage annulé".into(),
        )),
        (_, "sleep") => Ok((
            "systemctl",
            vec!["suspend".into()],
            "mise en veille de l'ordinateur demandée".into(),
        )),
        _ => unreachable!(),
    }
}

pub struct SystemPower;

impl Tool for SystemPower {
    fn name(&self) -> &str {
        "system_power"
    }
    fn description(&self) -> &str {
        "Éteint (`shutdown`), redémarre (`restart`) ou met en veille (`sleep`) l'ordinateur, ou annule un arrêt programmé (`cancel`). \
         Appelle directement cet outil quand l'utilisateur demande d'éteindre ou redémarrer son PC : c'est l'application qui demande l'accord (carte ou « confirmé » vocal) sauf s'il l'a autorisé en permanence dans les Paramètres."
    }
    fn parameters(&self) -> serde_json::Value {
        schema(
            serde_json::json!({
                "action": {
                    "type": "string",
                    "enum": ["shutdown", "restart", "sleep", "cancel"],
                    "description": "Action : « shutdown » (éteindre), « restart » (redémarrer), « sleep » (mettre en veille), « cancel » (annuler un arrêt en cours)."
                },
                "delay_seconds": {
                    "type": "integer",
                    "description": "Délai en secondes avant arrêt ou redémarrage (défaut 15, minimum 5)."
                }
            }),
            &["action"],
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
            let action = arg_str(args, "action").ok_or_else(|| Error::Tool("« action » manquant".into()))?;
            let delay = arg_u64(args, "delay_seconds", DEFAULT_POWER_DELAY_SECS);
            let (program, cmd_args, summary) = power_command(std::env::consts::OS, &action, delay)?;
            // Si l'utilisateur a placé `shutdown*` dans `deny_commands` (mode
            // « Interdit » dans Paramètres → Permissions), ce contrôle refuse
            // avant toute exécution (sauf `cancel` qui reste toujours libre).
            if !action.eq_ignore_ascii_case("cancel") && !action.eq_ignore_ascii_case("abort") {
                ctx.check(Capability::Execute, &format!("shutdown {action}"))?;
            }
            ctx.check(Capability::Execute, &format!("system_power {action}"))?;
            let mut command = tokio::process::Command::new(program);
            command
                .args(&cmd_args)
                .stdin(std::process::Stdio::null())
                .kill_on_drop(true);
            #[cfg(windows)]
            {
                command.creation_flags(0x08000000); // CREATE_NO_WINDOW
            }
            let output = tokio::time::timeout(std::time::Duration::from_secs(15), command.output())
                .await
                .map_err(|_| Error::Timeout)?
                .map_err(|e| Error::Tool(format!("action système impossible : {e}")))?;
            if output.status.success() {
                Ok(summary)
            } else {
                let code = output.status.code().unwrap_or(-1);
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                Err(Error::Tool(format!(
                    "action refusée par le système (code {code}) : {stderr}"
                )))
            }
        })
    }
}

/// Voir l'utilisateur via sa webcam, **à sa demande seulement** (Lot 2).
/// La photo part au modèle avec le tour (comme `screen.rs`) : jamais sur
/// disque, jamais dans l'historique — seule une note y figure. **1 capture
/// par minute au plus** (`webcam::MIN_INTERVAL`) et nettoyage automatique
/// (panier d'images vidé à chaque tour) : pas de stockage qui grossit, pas
/// de jetons brûlés en boucle.
pub struct WebcamCapture;

impl Tool for WebcamCapture {
    fn name(&self) -> &str {
        "webcam_capture"
    }
    fn description(&self) -> &str {
        "Prend une photo avec la webcam pour voir l'utilisateur ou ce qu'il montre (« regarde-moi », « tu me vois ? »). \
         À n'appeler que si l'utilisateur l'a explicitement demandé : c'est l'application qui demande son accord (carte ou « confirmé » vocal) sauf s'il l'a autorisé en permanence. \
         1 photo par minute au plus ; l'image est jointe au modèle puis oubliée (jamais sur disque)."
    }
    fn parameters(&self) -> serde_json::Value {
        schema(
            serde_json::json!({
                "camera": {"type": "integer", "description": "Index de la caméra (voir `webcam_list`, défaut 0)."},
                "detail": {"type": "string", "description": "Ce que l'utilisateur veut montrer (visage, objet tenu) : une phrase pour le contexte."}
            }),
            &[],
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
            let camera = args.get("camera").and_then(|v| v.as_u64()).map(|n| n as u32);
            let detail = arg_str(args, "detail").unwrap_or_default();
            // Mode « Interdit » (Paramètres → Permissions) : refus sec avant
            // d'allumer la caméra.
            ctx.check(Capability::Execute, "webcam_capture")?;
            let capture = tokio::task::spawn_blocking(move || crate::webcam::capture_photo(camera))
                .await
                .map_err(|e| Error::Tool(format!("capture webcam interrompue : {e}")))??;
            let label = if detail.trim().is_empty() {
                capture.camera.clone()
            } else {
                format!("{} — {}", capture.camera, detail.trim())
            };
            let summary = format!(
                "photo webcam prise ({} — {}×{}, {} Ko) : image jointe au modèle pour ce tour, puis oubliée (jamais sur disque). Décris ce que tu vois en une phrase, sans jargon technique.",
                label,
                capture.width,
                capture.height,
                capture.image.base64.len() * 3 / 4 / 1024
            );
            // Panier du tour : la boucle de l'agent joint ces images au
            // prochain appel au modèle, puis les oublie (nettoyage auto).
            if let Ok(mut pending) = ctx.pending_images.lock() {
                pending.clear();
                let mut image = capture.image.clone();
                image.label = label.chars().take(120).collect();
                pending.push(image);
            }
            log::info!("[webcam] {}", summary);
            Ok(summary)
        })
    }
}

/// Liste les webcams vues par le système, sans allumer de caméra.
pub struct WebcamList;

impl Tool for WebcamList {
    fn name(&self) -> &str {
        "webcam_list"
    }
    fn description(&self) -> &str {
        "Liste les webcams disponibles (index + nom), sans les allumer. À appeler avant `webcam_capture` quand l'index est inconnu."
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
            ctx.check_granted(Capability::Read, "webcams")?;
            let cameras = tokio::task::spawn_blocking(crate::webcam::list_cameras)
                .await
                .map_err(|e| Error::Tool(format!("liste des webcams interrompue : {e}")))?;
            Ok(crate::webcam::format_cameras(&cameras))
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
        // Avatar Godot et Whisper protégés par leur nom d'image (14 octobre).
        for vital in ["Godot_v4.5.1-stable_win64.exe", "whisper-server.exe", "jimmy.exe"] {
            assert!(is_protected_process_name(vital), "{vital} doit être protégé");
        }
        assert!(!is_protected_process_name("chrome.exe"));
        assert!(!is_protected_process_name("notepad.exe"));
    }

    #[test]
    fn fenetre_avatar_godot_ecartee_de_window_list() {
        let win = "1234\tnotepad\tSans titre - Bloc-notes\n4321\tGodot_v4.5.1-stable_win64\tJimy Avatar\n5678\tchrome\tGoogle Chrome\n";
        let fenetres = parse_window_list_win(win);
        assert_eq!(fenetres.len(), 2, "Godot ne doit pas remonter dans window_list");
        assert!(fenetres.iter().all(|w| !w.app.to_lowercase().contains("godot")));
    }

    #[test]
    fn system_power_construit_les_commandes_avec_delai_de_securite() {
        let (prog, args, msg) = power_command("windows", "shutdown", 15).expect("shutdown win");
        assert_eq!(prog, "shutdown.exe");
        assert_eq!(args, vec!["/s", "/t", "15"]);
        assert!(msg.contains("15 s"));

        // Un délai 0 est relevé à 5 s minimum pour laisser le temps d'annuler.
        let (_, args_min, _) = power_command("windows", "restart", 0).expect("restart min");
        assert_eq!(args_min, vec!["/r", "/t", "5"]);

        let (prog_c, args_c, _) = power_command("windows", "cancel", 15).expect("cancel");
        assert_eq!(prog_c, "shutdown.exe");
        assert_eq!(args_c, vec!["/a"]);

        assert!(power_command("windows", "explode", 15).is_err());
    }

    #[test]
    fn webcam_outils_branches_et_capabilites() {
        assert_eq!(WebcamCapture.spec().name, "webcam_capture");
        assert_eq!(WebcamCapture.capability(), Capability::Execute);
        assert_eq!(WebcamList.spec().name, "webcam_list");
        assert_eq!(WebcamList.capability(), Capability::Read);
    }
}
