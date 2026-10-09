//! Détection d'une mise à jour de Jimmy sur le dépôt distant.
//!
//! Le second PC tourne depuis un clone git : le commit local
//! (`git rev-parse HEAD` dans `paths.app`) se compare au bout de
//! `origin/main` (`git ls-remote origin HEAD`). Sans `.git` (zip,
//! installeur), la vérification se tait au lieu d'inventer un résultat.
//! Chaque appel `git` est borné dans le temps et tué en cas de dépassement.

use std::path::{Path, PathBuf};
use std::time::Duration;

/// Un `git` local doit répondre vite ou pas du tout.
const LOCAL_TIMEOUT: Duration = Duration::from_secs(10);
/// L'interrogation réseau du dépôt distant peut prendre plus longtemps.
const REMOTE_TIMEOUT: Duration = Duration::from_secs(30);
/// Premier contrôle après le démarrage (laisser mémoire et serveurs
/// s'installer), puis toutes les 30 minutes (même rythme que la revue).
const FIRST_CHECK_AFTER: Duration = Duration::from_secs(5 * 60);
const CHECK_EVERY: Duration = Duration::from_secs(30 * 60);

/// Un SHA-1 git complet : 40 caractères hexadécimaux.
pub fn is_valid_sha(text: &str) -> bool {
    text.len() == 40 && text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// `true` quand les deux bouts sont connus et différents. Un bout inconnu
/// (pas de `.git`, réseau coupé) ne vaut jamais « mise à jour ».
pub fn update_available(local: Option<&str>, remote: Option<&str>) -> bool {
    match (local, remote) {
        (Some(l), Some(r)) => is_valid_sha(l) && is_valid_sha(r) && l != r,
        _ => false,
    }
}

/// Prochaine valeur de `pending` : une alerte levée ne retombe que quand les
/// deux bouts se rejoignent (la mise à jour a été appliquée) ; un contrôle
/// sans réponse garde l'état précédent au lieu de l'effacer.
pub fn next_pending(was_pending: bool, local: Option<&str>, remote: Option<&str>) -> bool {
    if update_available(local, remote) {
        return true;
    }
    match (local, remote) {
        (Some(l), Some(r)) if is_valid_sha(l) && is_valid_sha(r) => false,
        _ => was_pending,
    }
}

/// Extrait le SHA de la sortie de `git ls-remote origin HEAD`
/// (de la forme `"<sha>\tHEAD\n"`).
pub fn parse_ls_remote(output: &str) -> Option<String> {
    let sha = output.split_whitespace().next()?;
    is_valid_sha(sha).then(|| sha.to_string())
}

/// Sortie d'un `git`, bornée dans le temps. `None` = pas de dépôt, `git`
/// absent, échec ou dépassement : le contrôle est reporté au cycle suivant.
async fn git_output(app_dir: &Path, args: &[&str], timeout: Duration) -> Option<String> {
    if !app_dir.join(".git").exists() {
        return None;
    }
    let child = tokio::process::Command::new("git")
        .args(args)
        .current_dir(app_dir)
        .kill_on_drop(true)
        .output();
    let output = tokio::time::timeout(timeout, child).await.ok()?.ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Commit sur lequel tourne ce Jimmy (`None` sans dépôt git local).
pub async fn local_head(app_dir: &Path) -> Option<String> {
    let out = git_output(app_dir, &["rev-parse", "HEAD"], LOCAL_TIMEOUT).await?;
    is_valid_sha(&out).then_some(out)
}

/// Bout de `origin/main` (`None` sans réseau ou sans dépôt).
pub async fn remote_head(app_dir: &Path) -> Option<String> {
    let out = git_output(app_dir, &["ls-remote", "origin", "HEAD"], REMOTE_TIMEOUT).await?;
    parse_ls_remote(&out)
}

/// État persisté (`data/update_state.json`) : une alerte levée survit au
/// redémarrage au lieu d'attendre le premier contrôle.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct UpdateState {
    pub last_check: Option<String>,
    pub local: Option<String>,
    pub remote: Option<String>,
    /// `true` tant que la mise à jour n'a pas été appliquée.
    pub pending: bool,
    /// SHA distant déjà annoncé dans une session « Mise à jour » : la
    /// session ne se réécrit pas à chaque cycle, seule la bulle revient.
    #[serde(default)]
    pub notified: Option<String>,
    /// SHA tiré par `/update` mais pas encore compilé (compilation ratée ou
    /// abandonnée) : l'alerte reste levée et `/update` recompile.
    #[serde(default)]
    pub unbuilt: Option<String>,
}

pub fn state_file(data_dir: &Path) -> PathBuf {
    data_dir.join("update_state.json")
}

/// Lecture tolérante : fichier absent ou corrompu = état neuf.
pub fn load_state(data_dir: &Path) -> UpdateState {
    std::fs::read_to_string(state_file(data_dir))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_state(data_dir: &Path, state: &UpdateState) {
    if let Err(error) = std::fs::write(
        state_file(data_dir),
        serde_json::to_string_pretty(state).unwrap_or_default(),
    ) {
        log::warn!("[update] état non sauvegardé : {error}");
    }
}

/// Un cycle de vérification : lit les deux bouts, met l'état à jour,
/// journalise. La notification suit dans `update_loop`.
pub async fn check(app: &crate::App) -> UpdateState {
    let local = local_head(&app.paths.app).await;
    let remote = remote_head(&app.paths.app).await;
    let mut state = load_state(&app.paths.data);
    state.last_check = Some(chrono::Utc::now().to_rfc3339());
    state.local = local.clone();
    state.remote = remote.clone();
    // Code tiré mais jamais compilé : les SHA se rejoignent, le binaire non.
    state.pending = next_pending(state.pending, local.as_deref(), remote.as_deref())
        || still_unbuilt(state.unbuilt.as_deref(), local.as_deref());
    save_state(&app.paths.data, &state);
    let short = |s: Option<&String>| {
        s.map(|sha| sha.chars().take(7).collect::<String>())
            .unwrap_or_else(|| "?".to_string())
    };
    if state.pending {
        log::info!(
            "[update] mise à jour disponible : local {} → distant {}",
            short(state.local.as_ref()),
            short(state.remote.as_ref())
        );
    } else if local.is_some() && remote.is_some() {
        log::info!("[update] à jour ({})", short(state.local.as_ref()));
    } else {
        log::info!("[update] vérification impossible (pas de dépôt git ou distant injoignable)");
    }
    state
}

/// Boucle de fond, sur le modèle de la revue périodique.
pub async fn update_loop(app: std::sync::Arc<crate::App>) {
    let mut first = true;
    loop {
        tokio::time::sleep(if first {
            first = false;
            FIRST_CHECK_AFTER
        } else {
            CHECK_EVERY
        })
        .await;
        remove_old_exe(&app.paths.app);
        check(&app).await;
        notify_if_pending(&app).await;
    }
}

/// Commande de mise à jour tapée dans le Chat.
pub const UPDATE_COMMAND: &str = "/update";

/// Reconnaît la commande, avec tolérance sur la casse et les blancs.
pub fn is_update_command(message: &str) -> bool {
    message.trim().eq_ignore_ascii_case(UPDATE_COMMAND)
}

/// Délai max du `git pull` (rapide ou réseau coupé).
const PULL_TIMEOUT: Duration = Duration::from_secs(120);
/// Délai max de la recompilation (elle prend plusieurs minutes).
const BUILD_TIMEOUT: Duration = Duration::from_secs(20 * 60);
/// Un signe de vie régulier pendant la compilation : le filet de 3 minutes
/// du Chat se réarme sur chaque événement.
const BUILD_PROGRESS_EVERY: Duration = Duration::from_secs(60);
/// Après la réponse, on ne coupe que le calme (bulle partie, annonce dite).
const RESTART_QUIET: Duration = Duration::from_secs(5);
/// Capuchon d'attente du calme : au-delà, on redémarre quand même.
const RESTART_WAIT_MAX: Duration = Duration::from_secs(5 * 60);

/// Sortie d'un `git pull --ff-only`, classée pour un message actionnable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullOutcome {
    /// Rien n'a bougé (quelqu'un a déjà appliqué).
    UpToDate,
    /// Des commits ont été récupérés.
    Updated,
    /// Dépôt local modifié ou divergé : `pull` refuse d'écraser.
    Rejected,
}

pub fn classify_pull(output: &str) -> PullOutcome {
    if output.contains("Not possible to fast-forward") {
        PullOutcome::Rejected
    } else if output.contains("Already up to date") {
        PullOutcome::UpToDate
    } else {
        PullOutcome::Updated
    }
}

/// Tronque un journal de commande pour l'afficher : une phrase, pas un mur.
fn short_log(text: &str, max: usize) -> String {
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.len() <= max {
        return flat;
    }
    format!("{}…", flat.chars().take(max).collect::<String>())
}

/// Résumé d'un journal de compilation raté : la première ligne d'erreur et
/// les deux suivantes (cargo met la cause sous `error:`). À défaut, la FIN
/// du journal : son début n'est que l'en-tête npm (`> jimmy-desktop…`).
fn build_error_summary(log: &str, max: usize) -> String {
    let lines: Vec<&str> = log.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let first_error = lines.iter().position(|l| {
        let lower = l.to_lowercase();
        lower.starts_with("error") || lower.contains("error:") || lower.contains("error[")
    });
    if let Some(i) = first_error {
        let end = (i + 3).min(lines.len());
        return short_log(&lines[i..end].join(" "), max);
    }
    let flat: String = log.split_whitespace().collect::<Vec<_>>().join(" ");
    let count = flat.chars().count();
    if count <= max {
        return flat;
    }
    format!("…{}", flat.chars().skip(count - max).collect::<String>())
}

/// `true` si le code tiré par un `/update` précédent n'a jamais été compilé
/// (compilation ratée ou abandonnée) et que le dépôt n'a pas bougé depuis.
pub fn still_unbuilt(unbuilt: Option<&str>, local: Option<&str>) -> bool {
    matches!((unbuilt, local), (Some(u), Some(l)) if u == l)
}

/// Binaire lancé par le raccourci et relancé après mise à jour.
const RELEASE_EXE: &str = "target/release/jimmy.exe";

/// Où l'exécutable en cours est écarté pendant la recompilation.
pub fn old_exe(exe: &Path) -> PathBuf {
    exe.with_extension("exe.old")
}

/// Écarte l'exécutable en cours (`jimmy.exe` → `jimmy.exe.old`) le temps de
/// la recompilation : Windows refuse de l'écraser (`os error 5`), mais
/// accepte de le renommer. Tant que `keep` n'est pas appelé, il revient à sa
/// place au `Drop` — compilation ratée, délai dépassé ou « STOP » (la future
/// abandonnée), le raccourci Bureau retrouve toujours un binaire.
pub struct ExeAside {
    exe: PathBuf,
    old: PathBuf,
    armed: bool,
}

impl ExeAside {
    pub fn new(exe: &Path) -> std::io::Result<Self> {
        let old = old_exe(exe);
        let armed = exe.exists();
        if armed {
            // Reste d'une mise à jour précédente (plus en cours d'exécution).
            if old.exists() {
                std::fs::remove_file(&old)?;
            }
            std::fs::rename(exe, &old)?;
        }
        Ok(Self {
            exe: exe.to_path_buf(),
            old,
            armed,
        })
    }

    /// Compilation réussie : le neuf reste, l'ancien attend le prochain
    /// démarrage pour être effacé (`remove_old_exe`).
    pub fn keep(mut self) {
        self.armed = false;
    }
}

impl Drop for ExeAside {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        // Un binaire partiel d'une compilation interrompue ne doit pas rester.
        if self.exe.exists() {
            let _ = std::fs::remove_file(&self.exe);
        }
        if let Err(error) = std::fs::rename(&self.old, &self.exe) {
            log::warn!("[update] binaire actuel non restauré : {error}");
        }
    }
}

/// Efface l'exécutable écarté par la dernière mise à jour. Silencieux s'il
/// tourne encore (ancien processus pas tout à fait sorti) : le prochain
/// démarrage réessaiera.
fn remove_old_exe(app_dir: &Path) {
    let old = old_exe(&app_dir.join(RELEASE_EXE));
    if old.exists() && std::fs::remove_file(&old).is_ok() {
        log::info!("[update] ancien binaire effacé");
    }
}

/// Le binaire est-il plus ancien que le dernier commit ? Cas d'un pull fait
/// sans compilation (échec d'un ancien `/update`, ou `git pull` à la main).
async fn binary_older_than_head(app_dir: &Path) -> bool {
    let Some(head_secs) = git_output(app_dir, &["log", "-1", "--format=%ct"], LOCAL_TIMEOUT)
        .await
        .and_then(|out| out.parse::<u64>().ok())
    else {
        return false;
    };
    let Ok(modified) = app_dir.join(RELEASE_EXE).metadata().and_then(|m| m.modified()) else {
        return false;
    };
    let exe_secs = modified
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    exe_secs < head_secs
}

/// Exécute `git` dans le dépôt, borné dans le temps. `Err` = pas de dépôt,
/// `git` absent, échec ou dépassement.
async fn git_run(app_dir: &Path, args: &[&str], timeout: Duration) -> Result<String, String> {
    if !app_dir.join(".git").exists() {
        return Err("ce Jimmy ne tourne pas depuis un clone git (zip ou installeur) : \
            passe par un clone pour les mises à jour automatiques"
            .to_string());
    }
    let mut child = tokio::process::Command::new("git")
        .args(args)
        .current_dir(app_dir)
        .kill_on_drop(true)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("git introuvable : {e}"))?;
    let start = std::time::Instant::now();
    loop {
        match child.try_wait().map_err(|e| format!("git illisible : {e}"))? {
            Some(status) => {
                let output = child
                    .wait_with_output()
                    .await
                    .map_err(|e| format!("git illisible : {e}"))?;
                let log = format!(
                    "{}{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                if status.success() {
                    return Ok(log);
                }
                return Err(short_log(&log, 200));
            }
            None => {
                if start.elapsed() > timeout {
                    let _ = child.kill().await;
                    return Err("délai dépassé".to_string());
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    }
}

/// Applique la mise à jour : `pull` fast-forward seul, recompilation,
/// redémarrage programmé. Tourne dans une tâche suivie (comme un tour
/// d'agent) : passage en fond, bouton « Arrêter » et annonces compris.
/// Un « STOP » abandonne la future et tue la commande en cours
/// (`kill_on_drop`) ; le Jimmy actuel continue alors comme avant.
pub async fn apply(
    app: std::sync::Arc<crate::App>,
    session_id: String,
    tx: tokio::sync::mpsc::Sender<crate::core::types::AgentEvent>,
) -> crate::Result<crate::core::types::AgentAnswer> {
    use crate::core::types::{AgentAnswer, AgentEvent};
    use crate::error::Error;
    let started = std::time::Instant::now();
    let answer = |text: String| AgentAnswer {
        session_id: session_id.clone(),
        text,
        tools_used: vec!["update".to_string()],
        duration_ms: started.elapsed().as_millis() as u64,
    };

    let before = local_head(&app.paths.app).await;
    let _ = tx
        .send(AgentEvent::ToolStart {
            call_id: "update-pull".to_string(),
            name: "update".to_string(),
            arguments: serde_json::json!({"step": "pull"}),
        })
        .await;
    let pull = git_run(&app.paths.app, &["pull", "--ff-only"], PULL_TIMEOUT).await;
    let _ = tx
        .send(AgentEvent::ToolEnd {
            call_id: "update-pull".to_string(),
            name: "update".to_string(),
            ok: pull.is_ok(),
            summary: "récupération".to_string(),
            duration_ms: 0,
        })
        .await;
    let pull_log = pull.map_err(|detail| {
        if detail.contains("Not possible to fast-forward") || classify_pull(&detail) == PullOutcome::Rejected
        {
            Error::Tool(
                "Ton dépôt local a des modifications : range-les avant /update.".to_string(),
            )
        } else if detail.starts_with("ce Jimmy") {
            Error::Tool(detail)
        } else {
            Error::Tool(format!(
                "Récupération impossible ({detail}). Vérifie ta connexion, puis retape /update."
            ))
        }
    })?;
    let after = local_head(&app.paths.app).await;
    let pulled_nothing =
        classify_pull(&pull_log) == PullOutcome::UpToDate || (before.is_some() && after == before);
    if pulled_nothing {
        // Code à jour, mais le binaire l'est-il ? (compilation précédente
        // ratée, ou `git pull` fait à la main sans recompiler.)
        let unbuilt = still_unbuilt(load_state(&app.paths.data).unbuilt.as_deref(), after.as_deref());
        if !unbuilt && !binary_older_than_head(&app.paths.app).await {
            return Ok(answer("Déjà à jour : rien à appliquer.".to_string()));
        }
        let _ = tx
            .send(AgentEvent::Progress {
                text: "Code déjà à jour, mais pas le binaire : je recompile.".to_string(),
            })
            .await;
    }

    // Noté AVANT la compilation : un échec, un délai dépassé ou un « STOP »
    // laissent l'alerte levée, et le prochain `/update` recompilera.
    let mut state = load_state(&app.paths.data);
    state.unbuilt = after.clone();
    state.pending = true;
    save_state(&app.paths.data, &state);

    // Windows refuse d'écraser l'exécutable en cours : on l'écarte, il
    // revient tout seul si la compilation n'aboutit pas (piège 97).
    let exe = app.paths.app.join(RELEASE_EXE);
    let aside = ExeAside::new(&exe).map_err(|e| {
        Error::Tool(format!(
            "Binaire actuel impossible à écarter avant compilation ({e}). Le Jimmy actuel tourne toujours."
        ))
    })?;

    let _ = tx
        .send(AgentEvent::ToolStart {
            call_id: "update-build".to_string(),
            name: "update".to_string(),
            arguments: serde_json::json!({"step": "build"}),
        })
        .await;
    let build_start = std::time::SystemTime::now();
    // Journal complet dans un fichier : des tubes jamais lus pendant la
    // compilation pourraient se remplir et la bloquer, et le détail reste
    // consultable après coup.
    let build_log = app.paths.data.join("logs").join("update-build.log");
    let open_log = || {
        std::fs::File::create(&build_log)
            .map_err(|e| Error::Tool(format!("journal de compilation impossible : {e}")))
    };
    let log_out = open_log()?;
    let log_err = log_out
        .try_clone()
        .map_err(|e| Error::Tool(format!("journal de compilation impossible : {e}")))?;
    let mut child = tokio::process::Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "scripts/build.ps1", "-Release"])
        .current_dir(&app.paths.app)
        .kill_on_drop(true)
        .stdout(std::process::Stdio::from(log_out))
        .stderr(std::process::Stdio::from(log_err))
        .spawn()
        .map_err(|e| Error::Tool(format!("compilation non lancée : {e}")))?;
    let mut last_progress = std::time::Instant::now();
    let status = loop {
        match child
            .try_wait()
            .map_err(|e| Error::Tool(format!("compilation illisible : {e}")))?
        {
            Some(status) => break status,
            None => {
                if build_start.elapsed().unwrap_or_default() > BUILD_TIMEOUT {
                    let _ = child.kill().await;
                    return Err(Error::Tool(
                        "Compilation trop longue (20 min), arrêtée. Le Jimmy actuel tourne toujours."
                            .to_string(),
                    ));
                }
                if last_progress.elapsed() > BUILD_PROGRESS_EVERY {
                    last_progress = std::time::Instant::now();
                    let minutes = build_start.elapsed().unwrap_or_default().as_secs() / 60;
                    let _ = tx
                        .send(AgentEvent::Progress {
                            text: format!("Compilation en cours… ({minutes} min)"),
                        })
                        .await;
                }
                tokio::time::sleep(Duration::from_secs(10)).await;
            }
        }
    };
    let _ = tx
        .send(AgentEvent::ToolEnd {
            call_id: "update-build".to_string(),
            name: "update".to_string(),
            ok: status.success(),
            summary: "compilation".to_string(),
            duration_ms: 0,
        })
        .await;
    if !status.success() {
        let log = std::fs::read(&build_log)
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_default();
        log::warn!("[update] compilation ratée, journal : {}", build_log.display());
        return Err(Error::Tool(format!(
            "Compilation impossible : {}. Le Jimmy actuel tourne toujours ; \
             retape /update après correction (journal : data/logs/update-build.log).",
            build_error_summary(&log, 300)
        )));
    }
    // Le binaire vient-il de ce build ? Sinon, on ne redémarre surtout pas.
    let fresh = exe
        .metadata()
        .and_then(|m| m.modified())
        .map(|modified| modified >= build_start)
        .unwrap_or(false);
    if !fresh {
        return Err(Error::Tool(
            "Compilation annoncée réussie mais binaire introuvable : je ne redémarre pas.".to_string(),
        ));
    }

    // L'alerte retombe maintenant : au redémarrage, local et distant se
    // rejoignent et aucun rappel ne repart.
    aside.keep();
    let mut state = load_state(&app.paths.data);
    state.local = after.clone();
    state.pending = false;
    state.unbuilt = None;
    save_state(&app.paths.data, &state);

    schedule_restart(app);
    Ok(answer(format!(
        "Mise à jour appliquée ({} → {}). Je redémarre dans quelques secondes.",
        before.as_deref().map(short_sha).unwrap_or("?".to_string()),
        after.as_deref().map(short_sha).unwrap_or("?".to_string())
    )))
}

/// Redémarre Jimmy après une mise à jour : attend le calme (réponse partie,
/// annonce dite), capuchon au cas où une nouvelle tâche occuperait Jimmy.
/// Vérifie avant de quitter que la mise à jour est bien appliquée : un
/// « STOP » en pleine compilation annule tout et ne doit pas redémarrer.
pub fn schedule_restart(app: std::sync::Arc<crate::App>) {
    tokio::spawn(async move {
        let start = std::time::Instant::now();
        while app.is_busy() && start.elapsed() < RESTART_WAIT_MAX {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        tokio::time::sleep(RESTART_QUIET).await;
        if load_state(&app.paths.data).pending {
            log::info!("[update] redémarrage annulé : mise à jour non appliquée");
            return;
        }
        log::info!("[update] redémarrage après mise à jour");
        app.shutdown_children().await;
        relaunch(&app.paths.app);
        std::process::exit(0);
    });
}

/// Relance Jimmy détaché (le lanceur du raccourci Bureau) : le nouveau
/// processus survit à la sortie de celui-ci. `-WaitPid` : le lanceur attend
/// que CE processus soit sorti, sinon l'instance unique de Tauri voit encore
/// l'ancien Jimmy et ferme le nouveau aussitôt (piège 97).
///
/// `CREATE_NO_WINDOW` et surtout PAS `DETACHED_PROCESS` : lancé sans console
/// depuis une appli graphique comme Jimmy, PowerShell 5.1 meurt avant
/// d'exécuter la moindre ligne (reproduit le 9 octobre avec un programme GUI
/// minimal : 0x08 → rien, 0x08000000 → script exécuté). Sortie du job tentée
/// d'abord (un job « tuer à la fermeture » emporterait le lanceur avec
/// Jimmy), sans elle si le job l'interdit.
#[cfg(windows)]
fn relaunch(app_dir: &Path) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
    let script = app_dir.join("scripts/launcher.ps1");
    let spawn = |flags: u32| {
        std::process::Command::new("powershell")
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(&script)
            .args(["-WaitPid", &std::process::id().to_string()])
            .current_dir(app_dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .creation_flags(flags)
            .spawn()
    };
    let spawned = spawn(CREATE_NO_WINDOW | CREATE_BREAKAWAY_FROM_JOB).or_else(|_| spawn(CREATE_NO_WINDOW));
    if let Err(error) = spawned {
        log::warn!("[update] lanceur non démarré : {error}");
    }
}

/// Hors Windows : pas de redémarrage automatique (Jimmy est Windows).
#[cfg(not(windows))]
fn relaunch(_app_dir: &Path) {
    log::warn!("[update] redémarrage non pris en charge hors Windows");
}

/// Texte de la bulle : court, sans jargon — il s'affiche au-dessus de la
/// tête de l'avatar et disparaît seul au bout de quelques secondes.
pub const BUBBLE_TEXT: &str = "Mise à jour disponible : tape /update dans le Chat.";

fn short_sha(sha: &str) -> String {
    sha.chars().take(7).collect()
}

fn announce_text(local: Option<&str>, remote: &str) -> String {
    format!(
        "Une mise à jour de Jimmy est disponible (local {} → distant {}). \
         Tape /update dans le Chat pour l'appliquer : \
         je récupère, recompile et redémarre tout seul.",
        local.map(short_sha).unwrap_or_else(|| "?".to_string()),
        short_sha(remote)
    )
}

/// La session « Mise à jour » ne s'écrit qu'une fois par SHA distant :
/// elle persiste dans l'Historique au lieu de se réécrire à chaque cycle.
pub fn should_announce(notified: Option<&str>, remote: Option<&str>) -> bool {
    match (notified, remote) {
        (Some(n), Some(r)) => n != r,
        (None, Some(_)) => true,
        _ => false,
    }
}

/// Notification : session persistante (une fois par SHA) + bulle de
/// l'avatar à chaque cycle **si Jimmy est disponible**. Une tâche au premier
/// plan ou une synthèse en cours fait sauter la bulle (pas la session) :
/// `/say` écraserait la bulle que l'utilisateur est en train de lire.
pub async fn notify_if_pending(app: &crate::App) {
    let mut state = load_state(&app.paths.data);
    let remote = match state.remote.clone() {
        Some(remote) if state.pending => remote,
        _ => return,
    };
    if should_announce(state.notified.as_deref(), Some(&remote)) {
        let text = announce_text(state.local.as_deref(), &remote);
        match app.history.create_session("Mise à jour") {
            Ok(session) => {
                let message = crate::core::types::Message::assistant(text);
                if app.history.append(&session, &message).is_ok() {
                    let _ = app.history.touch(&session);
                    state.notified = Some(remote.clone());
                    save_state(&app.paths.data, &state);
                    log::info!("[update] session « Mise à jour » écrite");
                }
            }
            Err(error) => log::warn!("[update] session impossible : {error}"),
        }
    }
    if app.is_busy() {
        log::debug!("[update] bulle sautée : Jimmy est occupé");
        return;
    }
    let settings = app.settings();
    let estimated =
        crate::providers::tts::estimate_ms(BUBBLE_TEXT, settings.tts.chars_per_minute);
    app.avatar.say(BUBBLE_TEXT, estimated).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha_valide_seulement_40_hex() {
        assert!(is_valid_sha("8c193c5b047514164277ad241e5b82f9b7681977"));
        assert!(!is_valid_sha("8c193c5"));
        assert!(!is_valid_sha(""));
        assert!(!is_valid_sha("zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz"));
        assert!(!is_valid_sha("8c193c5b047514164277ad241e5b82f9b76819770"));
    }

    #[test]
    fn disponible_quand_shas_connus_et_differents() {
        let local = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let remote = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        assert!(update_available(Some(local), Some(remote)));
        assert!(!update_available(Some(local), Some(local)));
        assert!(!update_available(Some(local), None));
        assert!(!update_available(None, Some(remote)));
        assert!(!update_available(None, None));
        assert!(!update_available(Some("court"), Some(remote)));
    }

    #[test]
    fn parse_ls_remote_extrait_le_sha() {
        let out = "8c193c5b047514164277ad241e5b82f9b7681977\tHEAD\n";
        assert_eq!(
            parse_ls_remote(out).as_deref(),
            Some("8c193c5b047514164277ad241e5b82f9b7681977")
        );
        assert_eq!(parse_ls_remote(""), None);
        assert_eq!(parse_ls_remote("n'importe quoi\n"), None);
    }

    #[test]
    fn pending_ne_retombe_que_quand_les_shas_se_rejoignent() {
        let local = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let remote = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        // Alerte levée quand le distant avance.
        assert!(next_pending(false, Some(local), Some(remote)));
        // Elle reste tant que l'écart dure.
        assert!(next_pending(true, Some(local), Some(remote)));
        // Appliquée (même SHA) : elle retombe.
        assert!(!next_pending(true, Some(local), Some(local)));
        // Contrôle sans réponse (réseau coupé) : on garde l'état.
        assert!(next_pending(true, Some(local), None));
        assert!(!next_pending(false, Some(local), None));
    }

    #[test]
    fn session_une_seule_fois_par_sha_distant() {
        let remote = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let autre = "cccccccccccccccccccccccccccccccccccccccc";
        assert!(should_announce(None, Some(remote)));
        assert!(!should_announce(Some(remote), Some(remote)));
        assert!(should_announce(Some(remote), Some(autre)));
        assert!(!should_announce(None, None));
        assert!(!should_announce(Some(remote), None));
    }

    #[test]
    fn annonce_dit_quoi_faire() {
        let text = announce_text(
            Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        );
        assert!(text.contains("/update"));
        assert!(text.contains("aaaaaaa"));
        assert!(text.contains("bbbbbbb"));
    }
    #[test]
    fn commande_reconnue_avec_tolerance() {
        assert!(is_update_command("/update"));
        assert!(is_update_command("  /update  "));
        assert!(is_update_command("/UPDATE"));
        assert!(!is_update_command("/update tout"));
        assert!(!is_update_command("mets à jour"));
        assert!(!is_update_command(""));
    }

    #[test]
    fn sortie_pull_classee_pour_un_message_actionnable() {
        assert_eq!(classify_pull("Already up to date.\n"), PullOutcome::UpToDate);
        assert_eq!(
            classify_pull("Updating 8c193c5..a1b2c3d\nFast-forward\n"),
            PullOutcome::Updated
        );
        assert_eq!(
            classify_pull("From github.com:xxx\nNot possible to fast-forward, aborting.\n"),
            PullOutcome::Rejected
        );
    }

    #[test]
    fn journal_tronque_en_une_phrase() {
        assert_eq!(short_log("ok", 200), "ok");
        let long = "x".repeat(500);
        assert!(short_log(&long, 200).len() <= 210);
        assert!(short_log(&long, 200).ends_with('…'));
    }

    /// Échec réel du 9 octobre au travail : le message montrait l'en-tête
    /// npm (`> jimmy-desktop…`) au lieu de l'erreur, qui est en fin de journal.
    #[test]
    fn erreur_de_compilation_lisible() {
        let log = "\n> jimmy-desktop@0.1.0 build\n> tsc && vite build\n\n\
            vite v5.4.0 building for production...\n✓ built in 1.94s\n   \
            Compiling jimmy-desktop v0.1.0 (C:\\x\\desktop\\src-tauri)\n\
            error: failed to remove file `C:\\x\\target\\release\\jimmy.exe`\n  \
            Accès refusé. (os error 5)\nfailed to build app: failed to build app\n       \
            Error failed to build app: failed to build app\n";
        let resume = build_error_summary(log, 300);
        assert!(resume.contains("failed to remove file"), "{resume}");
        assert!(resume.contains("Accès refusé"), "{resume}");
        assert!(!resume.contains("jimmy-desktop@"), "{resume}");
        // Sans ligne d'erreur : la fin du journal, pas son début.
        let sans = format!("{}fin utile", "début ".repeat(100));
        let resume = build_error_summary(&sans, 50);
        assert!(resume.contains("fin utile"), "{resume}");
    }

    /// Pull réussi mais compilation ratée : le code est à jour, le binaire
    /// non. L'alerte doit rester, et `/update` doit recompiler.
    #[test]
    fn code_tire_mais_pas_compile_reste_a_appliquer() {
        let a = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let b = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        assert!(still_unbuilt(Some(a), Some(a)));
        assert!(!still_unbuilt(None, Some(a)));
        // Le dépôt a bougé depuis (pull à la main) : plus le même cas.
        assert!(!still_unbuilt(Some(a), Some(b)));
        assert!(!still_unbuilt(Some(a), None));
    }

    /// Le bug du 9 octobre : `/update` recompile pendant que Jimmy tourne, et
    /// Windows refuse d'écraser un exécutable en cours (`os error 5`). Un
    /// exécutable en cours peut en revanche être renommé : on l'écarte en
    /// `.old`, et il revient si la compilation échoue ou est abandonnée.
    #[cfg(windows)]
    #[test]
    fn binaire_en_cours_ecarte_puis_restaure() {
        use std::fs;
        let dir = std::env::temp_dir().join(format!("jimmy-exe-aside-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("jimmy.exe");
        fs::copy(r"C:\Windows\System32\PING.EXE", &exe).unwrap();
        let mut child = std::process::Command::new(&exe)
            .args(["-n", "30", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        // Ce que fait cargo, et ce qui échouait : supprimer le binaire en cours.
        assert!(fs::remove_file(&exe).is_err(), "l'OS devrait verrouiller l'exécutable");

        // Compilation réussie : le neuf prend la place, l'ancien reste en `.old`.
        let aside = ExeAside::new(&exe).expect("un exécutable en cours se renomme");
        fs::write(&exe, b"neuf").expect("place libre après écartement");
        aside.keep();
        assert_eq!(fs::read(&exe).unwrap(), b"neuf");
        assert!(old_exe(&exe).exists());

        let _ = child.kill();
        let _ = child.wait();

        // Compilation ratée ou STOP : l'actuel revient à sa place.
        {
            let _aside = ExeAside::new(&exe).unwrap();
            assert!(!exe.exists());
        }
        assert_eq!(fs::read(&exe).unwrap(), b"neuf");
        let _ = fs::remove_dir_all(&dir);
    }

    /// Vérification réelle sur ce dépôt : les deux bouts se lisent et sont
    /// égaux (arbre poussé). Ignoré par défaut : exige `git` et le réseau.
    #[tokio::test]
    #[ignore]
    async fn tetes_locale_et_distante_lisibles_sur_ce_depot() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("racine du dépôt")
            .to_path_buf();
        let local = local_head(&root).await.expect("SHA local lisible");
        let remote = remote_head(&root).await.expect("SHA distant lisible");
        assert!(is_valid_sha(&local));
        assert!(is_valid_sha(&remote));
        assert!(
            !update_available(Some(&local), Some(&remote)),
            "arbre local en retard sur origin : pousser avant de tester"
        );
    }
}
