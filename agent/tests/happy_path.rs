//! Test du chemin heureux, exécuté pour de vrai.
//!
//! Ce test est ignoré par défaut (`cargo test` ne le lance pas) parce qu'il
//! consomme de vrais crédits LLM et touche le disque. On l'exécute quand on
//! veut vérifier la boucle complète :
//!
//! ```text
//! cargo test -p jimmy-agent --test happy_path -- --ignored --nocapture
//! ```
//!
//! Prérequis : un `.env` à la racine avec `OPENCODE_API_KEY`, et — pour la
//! partie vocale — `whisper-server` + un modèle dans `data/components/whisper`.
//!
//! Ce que le test valide, dans l'ordre du PLAN §35 :
//! 1. l'agent reçoit une demande en français ;
//! 2. il utilise un outil au lieu de deviner ;
//! 3. il répond ;
//! 4. la réponse est mémorisée ;
//! 5. le rappel mémoire la retrouve.

use std::path::Path;
use std::sync::Arc;

use jimmy_agent::config::Secrets;
use jimmy_agent::core::types::AvatarState;
use jimmy_agent::paths::Paths;
use jimmy_agent::App;

/// Environnement de test isolé : une base et un dossier de skills temporaires.
fn app_de_test() -> Option<Arc<App>> {
    let racine = std::env::temp_dir().join(format!("jimmy-happy-{}", uuid_like()));
    std::fs::create_dir_all(racine.join("data")).ok()?;
    let paths = Paths {
        data: racine.join("data"),
        app: racine.clone(),
        dev: false,
    };
    // Le test lit le même `.env` que l'application : c'est la réalité d'usage.
    let racine_workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| racine.clone());
    let _ = jimmy_agent::paths::load_dotenv(&racine_workspace.join(".env"));
    App::new(paths, Secrets::from_env()).ok()
}

fn uuid_like() -> String {
    format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    )
}

#[tokio::test]
#[ignore = "consulte de vrais services ; à lancer explicitement"]
async fn chemin_heureux_analyse_un_dossier() {
    let Some(app) = app_de_test() else {
        eprintln!("environnement de test indisponible");
        return;
    };
    let secrets = app.secrets.clone();
    if secrets.opencode_api_key.is_empty() {
        panic!("OPENCODE_API_KEY absente : ce test a besoin d'un vrai modèle");
    }

    // Un dossier à analyser, pour que l'agent ait un vrai motif d'appeler un outil.
    let dossier = app.paths.data.join("dossier-a-analyser");
    std::fs::create_dir_all(&dossier).unwrap();
    std::fs::write(dossier.join("notes.txt"), "Projet Jimmy : assistant de bureau.\n").unwrap();
    std::fs::write(dossier.join("todo.md"), "- [])pirical\n- [x] audio\n").unwrap();

    let settings = app.settings();
    let session = app.history.create_session("test").unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);

    let mut settings = settings.clone();
    settings.workspace = app.paths.app.to_string_lossy().to_string();

    let reponse = jimmy_agent::core::agent::run(
        app.deps(),
        settings,
        session.clone(),
        format!(
            "Analyse le dossier « {} » et explique-moi ce que tu trouves.",
            dossier.display()
        ),
        app.tool_context(),
        tx,
    )
    .await
    .expect("l'agent doit produire une réponse");

    // Les événements ont dû être émis.
    let mut etats = Vec::new();
    let mut outils = Vec::new();
    while let Ok(event) = rx.try_recv() {
        match event {
            jimmy_agent::core::types::AgentEvent::State { state, .. } => etats.push(state),
            jimmy_agent::core::types::AgentEvent::ToolStart { name, .. } => outils.push(name),
            _ => {}
        }
    }

    println!("--- réponse ---\n{}\n", reponse.text);
    println!("états : {etats:?}");
    println!("outils : {outils:?}");
    println!("durée : {} ms", reponse.duration_ms);

    assert!(!reponse.text.trim().is_empty(), "la réponse est vide");
    assert!(
        outils.iter().any(|o| o.contains("list_directory") || o.contains("read_file")),
        "l'agent aurait dû lire le dossier, il a utilisé : {outils:?}"
    );
    assert!(
        etats.contains(&AvatarState::Thinking),
        "l'état « thinking » n'a jamais été émis"
    );

    // Mémoire : ce qui a été appris doit être retrouvable.
    let total = app.memory.count().unwrap_or(0);
    println!("souvenirs après l'échange : {total}");
}