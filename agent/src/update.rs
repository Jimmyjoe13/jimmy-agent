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
    state.pending = next_pending(state.pending, local.as_deref(), remote.as_deref());
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
    if classify_pull(&pull_log) == PullOutcome::UpToDate {
        return Ok(answer("Déjà à jour : rien à appliquer.".to_string()));
    }
    let after = local_head(&app.paths.app).await;
    if before.is_some() && after == before {
        return Ok(answer("Déjà à jour : rien à appliquer.".to_string()));
    }

    let _ = tx
        .send(AgentEvent::ToolStart {
            call_id: "update-build".to_string(),
            name: "update".to_string(),
            arguments: serde_json::json!({"step": "build"}),
        })
        .await;
    let build_start = std::time::SystemTime::now();
    let mut child = tokio::process::Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "scripts/build.ps1", "-Release"])
        .current_dir(&app.paths.app)
        .kill_on_drop(true)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
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
    let output = child
        .wait_with_output()
        .await
        .map_err(|e| Error::Tool(format!("compilation illisible : {e}")))?;
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
        let log = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return Err(Error::Tool(format!(
            "Compilation impossible : {}. Le Jimmy actuel tourne toujours.",
            short_log(&log, 300)
        )));
    }
    // Le binaire vient-il de ce build ? Sinon, on ne redémarre surtout pas.
    let fresh = app
        .paths
        .app
        .join("target/release/jimmy.exe")
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
    let mut state = load_state(&app.paths.data);
    state.local = after.clone();
    state.pending = false;
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
        relaunch(&app.paths.app);
        std::process::exit(0);
    });
}

/// Relance Jimmy détaché (le lanceur du raccourci Bureau) : le nouveau
/// processus survit à la sortie de celui-ci.
#[cfg(windows)]
fn relaunch(app_dir: &Path) {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x00000008;
    let script = app_dir.join("scripts/launcher.ps1");
    let _ = std::process::Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&script)
        .current_dir(app_dir)
        .creation_flags(DETACHED_PROCESS)
        .spawn();
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
