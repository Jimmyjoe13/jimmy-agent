//! Projets et explorateur de fichiers de l'onglet Chat.
//!
//! Une conversation peut être rattachée à un projet (un dossier) : l'agent y
//! travaille, et un panneau latéral permet de naviguer dans ses fichiers, d'en
//! voir un aperçu, de les ouvrir avec l'application par défaut de Windows ou de
//! les montrer dans l'Explorateur — à la manière de Codex Desktop.
//!
//! Sécurité :
//! * chaque lecture passe par les permissions de Jimmy (capacité « lecture »),
//!   les mêmes que pour ses outils ;
//! * un fichier exécutable (`.exe`, `.bat`, `.ps1`…) n'est **jamais lancé**
//!   depuis l'explorateur : il est seulement montré dans l'Explorateur Windows.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;
use tauri::State;

use jimmy_agent::permissions::Capability;

use crate::AppState;

/// Entrées affichées au plus par dossier (un `node_modules` en a des milliers).
const MAX_ENTRIES: usize = 500;
/// Taille maximale de l'aperçu d'un fichier texte.
const PREVIEW_BYTES: usize = 64 * 1024;
/// Projets récents proposés dans le sélecteur.
const RECENT_PROJECTS: usize = 8;

/// Extensions qui exécutent du code quand on les « ouvre » sous Windows.
const EXECUTABLE_EXTENSIONS: &[&str] = &[
    "exe", "com", "bat", "cmd", "ps1", "psm1", "vbs", "vbe", "js", "jse", "wsf", "wsh", "msi", "msp", "scr",
    "lnk", "hta", "cpl", "reg", "jar", "pif", "appref-ms",
];

#[derive(Debug, Serialize, PartialEq)]
pub struct FsEntry {
    pub name: String,
    pub path: String,
    pub dir: bool,
    pub size: u64,
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Lecture autorisée par les permissions de Jimmy ? Message clair sinon.
fn check_read(state: &AppState, path: &Path) -> Result<(), String> {
    state
        .app
        .load_permissions()
        .check(Capability::Read, &path.to_string_lossy())
        .map_err(|_| {
            format!(
                "Jimmy n'a pas la permission de lire « {} » (Paramètres → Permissions).",
                path.display()
            )
        })
}

/// Le fichier exécute-t-il du code quand on l'ouvre ?
pub fn is_executable(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| EXECUTABLE_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// Contenu binaire (octet nul ou UTF-8 invalide) : pas d'aperçu texte.
pub fn looks_binary(bytes: &[u8]) -> bool {
    bytes.contains(&0) || std::str::from_utf8(bytes).is_err()
}

/// Dossiers d'abord, puis ordre alphabétique sans tenir compte de la casse.
pub fn sort_entries(entries: &mut [FsEntry]) {
    entries.sort_by(|a, b| b.dir.cmp(&a.dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
}

/// Nom court d'un projet : le dernier composant de son chemin.
fn project_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}

/// Contenu d'un dossier pour l'explorateur.
#[tauri::command]
pub async fn fs_list(state: State<'_, AppState>, path: String) -> Result<serde_json::Value, String> {
    let dir = PathBuf::from(path.trim());
    check_read(&state, &dir)?;
    if !dir.is_dir() {
        return Err(format!("dossier introuvable : {}", dir.display()));
    }
    let mut entries = Vec::new();
    let mut truncated = false;
    for entry in std::fs::read_dir(&dir).map_err(err)?.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        // Le dépôt Git interne n'est pas un fichier de projet.
        if name == ".git" {
            continue;
        }
        if entries.len() >= MAX_ENTRIES {
            truncated = true;
            break;
        }
        let meta = entry.metadata().ok();
        entries.push(FsEntry {
            name,
            path: entry.path().to_string_lossy().to_string(),
            dir: meta.as_ref().map(|m| m.is_dir()).unwrap_or(false),
            size: meta.as_ref().map(|m| m.len()).unwrap_or(0),
        });
    }
    sort_entries(&mut entries);
    Ok(serde_json::json!({ "path": dir.to_string_lossy(), "entries": entries, "truncated": truncated }))
}

/// Aperçu d'un fichier : début du texte, ou l'indication « binaire ».
#[tauri::command]
pub async fn fs_preview(state: State<'_, AppState>, path: String) -> Result<serde_json::Value, String> {
    let file = PathBuf::from(path.trim());
    check_read(&state, &file)?;
    let size = std::fs::metadata(&file).map_err(err)?.len();
    let mut bytes = Vec::with_capacity(PREVIEW_BYTES.min(size as usize));
    {
        use std::io::Read;
        std::fs::File::open(&file)
            .map_err(err)?
            .take(PREVIEW_BYTES as u64)
            .read_to_end(&mut bytes)
            .map_err(err)?;
    }
    // Une coupure à 64 Ko peut tomber au milieu d'un caractère : on la retire
    // avant de juger si c'est du texte.
    let text_end = match std::str::from_utf8(&bytes) {
        Ok(_) => bytes.len(),
        Err(e) if e.error_len().is_none() => e.valid_up_to(),
        Err(_) => bytes.len(),
    };
    let binary = looks_binary(&bytes[..text_end]);
    Ok(serde_json::json!({
        "path": file.to_string_lossy(),
        "size": size,
        "binary": binary,
        "text": if binary { String::new() } else { String::from_utf8_lossy(&bytes[..text_end]).to_string() },
        "truncated": size as usize > PREVIEW_BYTES,
        "executable": is_executable(&file),
    }))
}

/// Ouvre avec l'application par défaut de Windows. Un exécutable est montré
/// dans l'Explorateur au lieu d'être lancé. Renvoie « opened » ou « revealed ».
#[tauri::command]
pub async fn fs_open(state: State<'_, AppState>, path: String) -> Result<String, String> {
    let target = PathBuf::from(path.trim());
    check_read(&state, &target)?;
    if !target.exists() {
        return Err(format!("introuvable : {}", target.display()));
    }
    if target.is_file() && is_executable(&target) {
        reveal(&target)?;
        return Ok("revealed".into());
    }
    // `explorer.exe <chemin>` : application associée pour un fichier,
    // fenêtre de l'Explorateur pour un dossier.
    Command::new("explorer.exe").arg(&target).spawn().map_err(err)?;
    Ok("opened".into())
}

/// Montre le fichier (sélectionné) ou le dossier dans l'Explorateur Windows.
#[tauri::command]
pub async fn fs_reveal(state: State<'_, AppState>, path: String) -> Result<(), String> {
    let target = PathBuf::from(path.trim());
    check_read(&state, &target)?;
    reveal(&target)
}

/// Mentions « @ » du Chat (à la façon de Codex) : les fichiers du projet
/// dont le chemin relatif contient la suite. Requête vide = les entrées du
/// premier niveau seulement (le menu s'ouvre sans que l'utilisateur tape).
#[derive(Debug, Serialize, PartialEq)]
pub struct Mention {
    /// Chemin relatif à la racine, avec « / » — ce qui s'insère après « @ ».
    pub display: String,
    pub name: String,
    pub dir: bool,
}

/// Dossiers jamais traversés pour une recherche de mentions.
const MENTION_SKIP: &[&str] = &[
    ".git", ".venv", "venv", "env", "node_modules", "__pycache__", "target", "dist", "build",
    ".obsidian", ".trash", "coverage", ".pytest_cache", ".cache", ".next",
];
/// Budget du parcours (entrées visitées) et taille du menu.
const MENTION_VISITED: usize = 4000;
const MENTION_RESULTS: usize = 20;

/// Le parcours récursif lui-même, pur pour les tests. Requête vide : depth
/// 0 uniquement. Requête non vide : profondeur 6, filtre sur le chemin
/// relatif (insensible à la casse, accents repliés).
fn walk_mentions(root: &Path, query: &str) -> (Vec<Mention>, bool) {
    let needle = normalize(query.trim());
    let mut visited = 0usize;
    let mut truncated = false;
    let mut out = Vec::new();
    walk_level(root, "", &needle, &mut out, &mut visited, &mut truncated, if needle.is_empty() { 0 } else { 6 });
    (out, truncated)
}

/// Casse et accents repliés (« évasion » → « evasion »), comme la mémoire :
/// réutilisation du normaliseur de Jimmy plutôt qu'un doublon.
fn normalize(text: &str) -> String {
    jimmy_agent::memory::embed::normalize(text)
}

fn walk_level(
    dir: &Path,
    rel: &str,
    needle: &str,
    out: &mut Vec<Mention>,
    visited: &mut usize,
    truncated: &mut bool,
    depth: u32,
) {
    if out.len() >= MENTION_RESULTS || *visited >= MENTION_VISITED || *truncated {
        *truncated = out.len() >= MENTION_RESULTS || *visited >= MENTION_VISITED;
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut subdirs: Vec<PathBuf> = Vec::new();
    for entry in entries.flatten() {
        *visited += 1;
        if *visited >= MENTION_VISITED || out.len() >= MENTION_RESULTS {
            *truncated = true;
            return;
        }
        let Ok(file_type) = entry.file_type() else { continue };
        let name = entry.file_name().to_string_lossy().to_string();
        let dir = file_type.is_dir();
        if dir
            && (MENTION_SKIP.iter().any(|skip| name.eq_ignore_ascii_case(skip))
                || (name.starts_with('.') && name != ".github"))
        {
            continue;
        }
        let display = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
        // Requête vide : premier niveau, sans les fichiers cachés (bruit).
        // Avec une requête : tout candidat passe — taper « .env » montre
        // `.env.example`, et l'aperçu d'un fichier de secrets reste la
        // décision de l'utilisateur.
        let matched = if needle.is_empty() {
            !name.starts_with('.')
        } else {
            normalize(&display).contains(needle)
        };
        if matched && !(needle.is_empty() && depth > 0) {
            out.push(Mention { display: display.clone(), name, dir });
        }
        if dir && depth > 0 {
            subdirs.push(entry.path());
        }
    }
    if depth > 0 {
        for subdir in subdirs {
            // Le relatif se reconstruit depuis le chemin : dernier composant.
            let sub_name = subdir
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let sub_rel = if rel.is_empty() { sub_name } else { format!("{rel}/{sub_name}") };
            walk_level(&subdir, &sub_rel, needle, out, visited, truncated, depth - 1);
        }
    }
}

/// Commande du menu « @ ». La racine est le projet de la conversation.
#[tauri::command]
pub async fn fs_search(state: State<'_, AppState>, root: String, query: String) -> Result<serde_json::Value, String> {    let root_path = PathBuf::from(root.trim());
    check_read(&state, &root_path)?;
    let valid = root_path.exists() && root_path.is_dir();
    if !valid {
        return Ok(serde_json::json!({
            "entries": [],
            "truncated": false,
            "rootInvalid": true,
        }));
    }
    let (entries, truncated) = walk_mentions(&root_path, &query);
    Ok(serde_json::json!({
        "entries": entries,
        "truncated": truncated,
        "rootInvalid": false,
    }))
}

/// Le dépôt contenant un chemin : le plus proche ancêtre avec un `.git`.
fn find_repo(from: &Path) -> Option<PathBuf> {
    let mut probe = if from.is_dir() {
        Some(from.to_path_buf())
    } else {
        from.parent().map(Path::to_path_buf)
    };
    while let Some(dir) = probe {
        if dir.join(".git").exists() {
            return Some(dir);
        }
        probe = dir.parent().map(Path::to_path_buf);
    }
    None
}

/// Sortie bornée d'une sous-commande (500 Ko), temps borné (10 s) :
/// les règles du projet valent pour les commandes de l'interface aussi.
const DIFF_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
const DIFF_MAX_BYTES: usize = 500 * 1024;

/// Le diff git d'un fichier écrit pendant le tour. Lecture seule : le
/// dépôt est trouvé depuis le chemin, la commande tourne dans ce dépôt.
/// Diff vide (fichier non commité ou rien à voir) avec sa raison.
#[tauri::command]
pub async fn fs_diff(state: State<'_, AppState>, file: String) -> Result<serde_json::Value, String> {
    let path = PathBuf::from(file.trim());
    check_read(&state, &path)?;
    let Some(repo) = find_repo(&path) else {
        return Ok(serde_json::json!({ "diff": "", "reason": "hors d'un dépôt git" }));
    };
    let rel = match path.strip_prefix(&repo) {
        Ok(rel) => rel.to_path_buf(),
        Err(_) => return Ok(serde_json::json!({ "diff": "", "reason": "chemin hors du dépôt" })),
    };
    // Bornée en temps comme toute commande de l'interface (DIFF_TIMEOUT) : un
    // `git diff` sur un dépôt énorme ne doit pas figer le bloc « travaux ».
    let mut cmd = tokio::process::Command::new("git");
    cmd.arg("-C")
        .arg(&repo)
        .arg("diff")
        .arg("HEAD")
        .arg("--")
        .arg(&rel)
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let child = tokio::time::timeout(DIFF_TIMEOUT, cmd.output())
        .await
        .map_err(|_| "diff git : délai dépassé (10 s)".to_string())?
        .map_err(err)?;
    let diff = String::from_utf8_lossy(&child.stdout);
    let diff = diff.chars().take(DIFF_MAX_BYTES).collect::<String>();
    if diff.trim().is_empty() {
        return Ok(serde_json::json!({
            "diff": "",
            "reason": "pas de diff : fichier nouveau (jamais commité) ou déjà identique",
        }));
    }
    Ok(serde_json::json!({ "diff": diff, "reason": "" }))
}


fn reveal(target: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // `/select,"chemin"` doit arriver tel quel : la mise entre guillemets
        // automatique de Rust ferait échouer la sélection.
        Command::new("explorer.exe")
            .raw_arg(format!("/select,\"{}\"", target.display()))
            .spawn()
            .map_err(err)?;
    }
    #[cfg(not(windows))]
    {
        let _ = target;
    }
    Ok(())
}

/// Sélecteur de dossier natif de Windows. `None` si l'utilisateur annule.
#[tauri::command]
pub async fn pick_folder(app: tauri::AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog().file().set_title("Ouvrir un projet").blocking_pick_folder()
    })
    .await
    .map_err(err)?;
    Ok(picked.and_then(|p| p.into_path().ok()).map(|p| p.to_string_lossy().to_string()))
}

/// Projets récents (conversations rattachées), le dossier par défaut en plus.
#[tauri::command]
pub async fn projects_recent(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let app = &state.app;
    let permissions = app.load_permissions();
    let describe = |path: &str| {
        serde_json::json!({
            "path": path,
            "name": project_name(path),
            "exists": Path::new(path).is_dir(),
            "readable": permissions.check(Capability::Read, path).is_ok(),
        })
    };
    let recent: Vec<serde_json::Value> = app
        .history
        .recent_projects(RECENT_PROJECTS)
        .map_err(err)?
        .iter()
        .map(|p| describe(p))
        .collect();
    let workspace = app.settings().workspace;
    Ok(serde_json::json!({ "recent": recent, "default": describe(&workspace) }))
}

/// Rattache un projet à une conversation (`None` : dossier par défaut).
#[tauri::command]
pub async fn session_set_project(
    state: State<'_, AppState>,
    session_id: String,
    project: Option<String>,
) -> Result<(), String> {
    if let Some(path) = project.as_deref().filter(|p| !p.trim().is_empty()) {
        if !Path::new(path.trim()).is_dir() {
            return Err(format!("dossier introuvable : {path}"));
        }
    }
    state.app.history.set_project(&session_id, project.as_deref()).map_err(err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_executable_n_est_jamais_lance() {
        assert!(is_executable(Path::new(r"C:\x\setup.EXE")));
        assert!(is_executable(Path::new(r"C:\x\build.ps1")));
        assert!(is_executable(Path::new(r"C:\x\raccourci.lnk")));
        assert!(!is_executable(Path::new(r"C:\x\README.md")));
        assert!(!is_executable(Path::new(r"C:\x\main.rs")));
        assert!(!is_executable(Path::new(r"C:\x\Makefile")));
    }

    #[test]
    fn le_binaire_est_reconnu() {
        assert!(looks_binary(&[0x89, b'P', b'N', b'G', 0, 1]));
        assert!(!looks_binary("fn main() { println!(\"é\"); }".as_bytes()));
    }

    #[test]
    fn les_dossiers_passent_avant_les_fichiers() {
        let e = |name: &str, dir: bool| FsEntry { name: name.into(), path: name.into(), dir, size: 0 };
        let mut entries = vec![e("b.txt", false), e("Zeta", true), e("a.txt", false), e("agent", true)];
        sort_entries(&mut entries);
        let names: Vec<&str> = entries.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, ["agent", "Zeta", "a.txt", "b.txt"]);
    }

    /// Racine temporaire : agent, docs, un gros dossier exclu, un fichier
    /// caché à ne pas montrer, et `src` en profondeur.
    fn racine() -> PathBuf {
        let root = std::env::temp_dir().join(format!("mentions-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src").join("agent")).unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::create_dir_all(root.join("node_modules")).unwrap();
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::write(root.join("src").join("pipeline.py"), "x").unwrap();
        std::fs::write(root.join("src").join("agent").join("notify.py"), "x").unwrap();
        std::fs::write(root.join("README.md"), "x").unwrap();
        std::fs::write(root.join(".secret"), "x").unwrap();
        std::fs::write(root.join("node_modules").join("perdu.js"), "x").unwrap();
        std::fs::write(root.join("docs").join("guide.md"), "x").unwrap();
        root
    }

    #[test]
    fn vide_liste_le_premier_niveau() {
        let (entries, truncated) = walk_mentions(&racine(), "");
        let names: Vec<&str> = entries.iter().map(|m| m.name.as_str()).collect();
        assert!(!truncated);
        // Premier niveau seulement : `src` et `docs` visibles, `notify.py`
        // (en profondeur) absent, `.secret` exclu, pas de dot-fichier.
        assert!(names.contains(&"src") && names.contains(&"docs"), "{names:?}");
        assert!(!names.iter().any(|n| n.contains("pipeline")), "{names:?}");
        assert!(!names.iter().any(|n| n.starts_with('.')), "{names:?}");
    }

    #[test]
    fn filtre_par_nom_avec_casse_et_accents() {
        let (entries, _) = walk_mentions(&racine(), "NOTIFY");
        assert_eq!(entries.len(), 1, "{entries:?}");
        assert_eq!(entries[0].display, "src/agent/notify.py");
        assert!(!entries[0].dir);
    }

    #[test]
    fn les_dossiers_interdits_ne_sont_pas_traverses() {
        let (entries, _) = walk_mentions(&racine(), "perdu");
        assert!(entries.is_empty(), "{entries:?}");
    }

    #[test]
    fn le_depot_est_le_plus_proche_ancetre_avec_git() {
        let root = std::env::temp_dir().join(format!("diff-repo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::create_dir_all(root.join("src").join("deep")).unwrap();
        let fichier = root.join("src").join("deep").join("fichier.rs");
        std::fs::write(&fichier, "x").unwrap();
        assert_eq!(find_repo(&fichier), Some(root));
        // Hors dépôt : remontée jusqu'à la racine, rien.
        let non_repo = std::env::temp_dir().join(format!("diff-sans-repo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&non_repo);
        std::fs::create_dir_all(non_repo.join("a")).unwrap();
        assert_eq!(find_repo(&non_repo.join("a")), None);
    }
}
