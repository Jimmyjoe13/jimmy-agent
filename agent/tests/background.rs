//! Tâches de fond (7 octobre) : une tâche outillée qui dure passe en
//! arrière-plan, libère le premier plan, ignore « STOP » mais pas « arrête
//! tout » ni son propre bouton ; une seule à la fois.
//!
//! Tâches simulées (pas de modèle) : elles émettent des `ToolStart` comme la
//! boucle d'agent, ce qui suffit à déclencher le passage en fond.

use std::sync::Arc;
use std::time::Duration;

use jimmy_agent::config::Secrets;
use jimmy_agent::core::types::{AgentAnswer, AgentEvent};
use jimmy_agent::paths::Paths;
use jimmy_agent::{App, TaskOutcome};
use tokio::sync::mpsc::{Receiver, Sender};

fn app_isolee() -> (Arc<App>, std::path::PathBuf) {
    let racine = std::env::temp_dir().join(format!("jimmy-fond-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(racine.join("data")).unwrap();
    let paths = Paths { data: racine.join("data"), app: racine.clone(), dev: false };
    let app = App::new(paths, Secrets::default()).expect("app");
    // Pas de synthèse vocale dans les tests (l'annonce de fin parlerait).
    let mut settings = app.settings();
    settings.tts.enabled = false;
    app.save_settings(settings).expect("réglages");
    (app, racine)
}

fn outil(n: usize) -> AgentEvent {
    AgentEvent::ToolStart {
        call_id: format!("c{n}"),
        name: "run_command".into(),
        arguments: serde_json::json!({ "command": format!("étape {n}") }),
    }
}

/// Tâche outillée : trois outils (seuil de passage en fond), puis `duree`
/// de travail, puis une réponse finale.
async fn travail(tx: Sender<AgentEvent>, duree: Duration, texte: &str) -> jimmy_agent::Result<AgentAnswer> {
    for n in 1..=3 {
        let _ = tx.send(outil(n)).await;
    }
    tokio::time::sleep(duree).await;
    let _ = tx.send(AgentEvent::Final { text: texte.into() }).await;
    Ok(AgentAnswer { session_id: "s".into(), text: texte.into(), tools_used: vec![], duration_ms: 0 })
}

/// Prochain événement, ou échec au bout de 3 s.
async fn suivant(rx: &mut Receiver<AgentEvent>) -> AgentEvent {
    tokio::time::timeout(Duration::from_secs(3), rx.recv())
        .await
        .expect("événement attendu")
        .expect("canal ouvert")
}

/// Attend le `Final` enveloppé de la tâche de fond.
async fn fin_de_fond(rx: &mut Receiver<AgentEvent>) -> (String, String) {
    loop {
        if let AgentEvent::Background { session_id, event, .. } = suivant(rx).await {
            if let AgentEvent::Final { text } = *event {
                return (session_id, text);
            }
        }
    }
}

#[tokio::test]
async fn une_tache_outillee_passe_en_fond_et_libere_le_premier_plan() {
    let (app, racine) = app_isolee();
    let (events, mut rx) = tokio::sync::mpsc::channel::<AgentEvent>(64);
    let ticket = app.start_task("s1", "écris notify.py", events, |tx| travail(tx, Duration::from_secs(30), "fini"));

    let outcome = tokio::time::timeout(Duration::from_secs(3), ticket.wait()).await.expect("passage en fond rapide");
    assert!(matches!(outcome, TaskOutcome::Detached));
    // Deux outils au premier plan, puis le passage, puis le 3e enveloppé.
    assert!(matches!(suivant(&mut rx).await, AgentEvent::ToolStart { .. }));
    assert!(matches!(suivant(&mut rx).await, AgentEvent::ToolStart { .. }));
    match suivant(&mut rx).await {
        AgentEvent::Detached { session_id, title, .. } => {
            assert_eq!(session_id, "s1");
            assert_eq!(title, "écris notify.py");
        }
        other => panic!("Detached attendu, reçu {other:?}"),
    }
    assert!(matches!(suivant(&mut rx).await, AgentEvent::Background { .. }));

    assert!(!app.is_busy(), "le premier plan est libre pendant la tâche de fond");
    let tasks = app.tasks.list();
    assert_eq!(tasks.len(), 1);
    assert!(tasks[0].background);
    assert_eq!(tasks[0].step, "run_command étape 3");
    assert!(app.tasks.prompt_block("s1").is_some(), "Jimmy sait qu'elle tourne");

    assert!(app.request_stop_all());
    let _ = std::fs::remove_dir_all(&racine);
}

#[tokio::test]
async fn stop_epargne_la_tache_de_fond_arrete_tout_non() {
    let (app, racine) = app_isolee();
    let (events, mut rx) = tokio::sync::mpsc::channel::<AgentEvent>(64);
    let ticket = app.start_task("s1", "longue", events, |tx| travail(tx, Duration::from_secs(30), "jamais"));
    assert!(matches!(ticket.wait().await, TaskOutcome::Detached));

    // « STOP » : rien au premier plan, la tâche de fond continue.
    assert!(!app.request_stop(), "STOP n'a rien à arrêter au premier plan");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(app.tasks.list().len(), 1, "la tâche de fond a survécu à STOP");

    // « Arrête tout » : elle s'arrête, et le Chat reçoit la fin dans sa session.
    assert!(app.request_stop_all());
    let (session, text) = fin_de_fond(&mut rx).await;
    assert_eq!(session, "s1");
    assert_eq!(text, jimmy_agent::tasks::BACKGROUND_STOPPED);
    assert!(app.tasks.list().is_empty());
    let _ = std::fs::remove_dir_all(&racine);
}

#[tokio::test]
async fn le_bouton_d_une_tache_l_arrete_seule() {
    let (app, racine) = app_isolee();
    let (events, mut rx) = tokio::sync::mpsc::channel::<AgentEvent>(64);
    let ticket = app.start_task("s1", "longue", events, |tx| travail(tx, Duration::from_secs(30), "jamais"));
    let id = ticket.id.clone();
    assert!(matches!(ticket.wait().await, TaskOutcome::Detached));
    assert!(app.stop_task(&id));
    assert_eq!(fin_de_fond(&mut rx).await.1, jimmy_agent::tasks::BACKGROUND_STOPPED);
    assert!(!app.stop_task(&id), "déjà arrêtée");
    let _ = std::fs::remove_dir_all(&racine);
}

#[tokio::test]
async fn une_seule_tache_de_fond_la_seconde_reste_au_premier_plan() {
    let (app, racine) = app_isolee();
    let (events, mut rx) = tokio::sync::mpsc::channel::<AgentEvent>(64);
    let first = app.start_task("s1", "première", events.clone(), |tx| travail(tx, Duration::from_secs(30), "jamais"));
    assert!(matches!(first.wait().await, TaskOutcome::Detached));
    while !matches!(suivant(&mut rx).await, AgentEvent::Background { .. }) {}

    // Même seuil atteint, mais la place est prise : premier plan, comme avant.
    let second = app.start_task("s2", "seconde", events, |tx| travail(tx, Duration::from_millis(300), "seconde finie"));
    match second.wait().await {
        TaskOutcome::Done(Ok(answer)) => assert_eq!(answer.text, "seconde finie"),
        _ => panic!("la seconde tâche doit finir au premier plan"),
    }
    // Ses événements n'ont pas été enveloppés.
    let mut finals = 0;
    while let Ok(Some(event)) = tokio::time::timeout(Duration::from_millis(200), rx.recv()).await {
        match event {
            AgentEvent::Final { text } => {
                assert_eq!(text, "seconde finie");
                finals += 1;
            }
            AgentEvent::Detached { .. } => panic!("la seconde ne doit pas passer en fond"),
            _ => {}
        }
    }
    assert_eq!(finals, 1);
    app.request_stop_all();
    let _ = std::fs::remove_dir_all(&racine);
}

#[tokio::test]
async fn la_fin_d_une_tache_de_fond_arrive_dans_sa_session() {
    let (app, racine) = app_isolee();
    let (events, mut rx) = tokio::sync::mpsc::channel::<AgentEvent>(64);
    let ticket = app.start_task("s1", "courte", events, |tx| travail(tx, Duration::from_millis(300), "c'est écrit"));
    assert!(matches!(ticket.wait().await, TaskOutcome::Detached));
    assert_eq!(fin_de_fond(&mut rx).await, ("s1".to_string(), "c'est écrit".to_string()));
    // Le registre se vide une fois la fin traitée.
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(app.tasks.list().is_empty());
    let _ = std::fs::remove_dir_all(&racine);
}

#[tokio::test]
async fn pas_de_passage_en_fond_pendant_une_autorisation() {
    let (app, racine) = app_isolee();
    let (events, _rx) = tokio::sync::mpsc::channel::<AgentEvent>(64);
    let ticket = app.start_task("s1", "écris le .env", events, |tx| async move {
        let _ = tx.send(AgentEvent::Approval { id: "a1".into(), target: ".env".into(), detail: String::new() }).await;
        for n in 1..=3 {
            let _ = tx.send(outil(n)).await;
        }
        // L'utilisateur lit la carte : la tâche reste au premier plan.
        tokio::time::sleep(Duration::from_millis(600)).await;
        let _ = tx.send(AgentEvent::ApprovalResolved { id: "a1".into(), approved: true }).await;
        let _ = tx.send(outil(4)).await;
        tokio::time::sleep(Duration::from_secs(30)).await;
        Ok(AgentAnswer { session_id: "s1".into(), text: "jamais".into(), tools_used: vec![], duration_ms: 0 })
    });
    let started = std::time::Instant::now();
    assert!(matches!(ticket.wait().await, TaskOutcome::Detached));
    assert!(started.elapsed() >= Duration::from_millis(500), "passée en fond avant la réponse à la carte");
    app.request_stop_all();
    let _ = std::fs::remove_dir_all(&racine);
}
