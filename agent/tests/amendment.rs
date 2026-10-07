//! Filet de l'auto-édition du prompt (lot « croissance », inspiré de GEPA).
//!
//! Un amendement n'est acté que si le filet passe. Le filet : sur les
//! dernières demandes réelles de l'utilisateur (lues dans la vraie base), la
//! réponse produite AVEC l'amendement doit rester saine — non vide, sans
//! dérive d'écritures étrangères — et de longueur comparable à la réponse
//! SANS l'amendement. Aucun outil : on ne valide ici que l'effet des
//! instructions sur les réponses texte.
//!
//!   cargo test --test amendment '--' --ignored --nocapture
//!
//! L'amendement testé : le contenu courant de `data/growth_amendments.md`
//! (dans le dossier de données réel), ou `JIMMY_AMENDMENT` pour un candidat
//! pas encore écrit. Sans amendement, le test échoue : rien à valider.

use std::path::PathBuf;

use jimmy_agent::config::Secrets;
use jimmy_agent::core::prompt::{self, PromptContext};
use jimmy_agent::core::types::{Message, Role};
use jimmy_agent::paths::Paths;
use jimmy_agent::App;

/// Caractères d'écritures qui n'ont rien à faire dans une réponse française.
fn foreign_chars(text: &str) -> usize {
    text.chars()
        .filter(|c| {
            let c = *c as u32;
            (0x0400..=0x04FF).contains(&c)
                || (0x3040..=0x30FF).contains(&c)
                || (0x3400..=0x9FFF).contains(&c)
                || (0xAC00..=0xD7AF).contains(&c)
        })
        .count()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "appelle le vrai modèle sur des demandes réelles ; à lancer explicitement"]
async fn l_amendement_ne_degrade_pas_les_reponses() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).is_test(true).try_init();
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf();
    let _ = jimmy_agent::paths::load_dotenv(&workspace.join(".env"));
    let paths = Paths { data: workspace.join("data"), app: workspace.clone(), dev: true };
    let app = App::new(paths, Secrets::from_env()).expect("app");
    let settings = app.settings();

    let amendment = match std::env::var("JIMMY_AMENDMENT").ok().filter(|a| !a.trim().is_empty()) {
        Some(a) => a,
        None => jimmy_agent::growth::read_amendments(&workspace.join("data"))
            .expect("aucun amendement à valider : JIMMY_AMENDMENT ou data/growth_amendments.md"),
    };
    println!("amendement : « {} »", amendment.chars().take(120).collect::<String>());

    // Corpus : les 3 dernières demandes réelles (une par session récente,
    // assez longue pour être un vrai tour de travail).
    let mut corpus: Vec<String> = Vec::new();
    for session in app.history.sessions(20).unwrap() {
        let request = app
            .history
            .messages(&session.id, 100)
            .unwrap()
            .into_iter()
            .find(|m| m.role == Role::User && m.content.chars().count() > 30)
            .map(|m| m.content);
        if let Some(request) = request {
            corpus.push(request);
            if corpus.len() == 3 {
                break;
            }
        }
    }
    assert!(corpus.len() >= 2, "corpus trop petit : {} demande(s)", corpus.len());

    for (round, request) in corpus.iter().enumerate() {
        // Contexte commun aux deux variantes (souvenirs, vault, outils).
        let memory_block = prompt::recall_for(&app.memory, &settings, request).await;
        let vault_block = prompt::vault_context(app.vault.as_ref(), &settings, request).await;
        let common = |amendments: Option<String>| PromptContext {
            settings: &settings,
            memory: &app.memory,
            skills: &app.skills,
            registry: &app.registry,
            request,
            memory_block: memory_block.clone(),
            vault_block: vault_block.clone(),
            recent_tools: None,
            amendments,
            background_task: None,
            voice: false,
        };
        let messages = |system: String| {
            vec![
                Message::system(system),
                Message::user(request.clone()),
            ]
        };
        let specs: Vec<jimmy_agent::core::types::ToolSpec> = Vec::new();
        let base = app
            .llm
            .chat(&settings.llm.model, &messages(prompt::build_system(&common(None))), &specs, Some(0.0), settings.llm.max_tokens)
            .await;
        let candidate = app
            .llm
            .chat(&settings.llm.model, &messages(prompt::build_system(&common(Some(amendment.clone())))), &specs, Some(0.0), settings.llm.max_tokens)
            .await;
        match (base, candidate) {
            (Ok(base), Ok(cand)) => {
                let (n_base, n_cand) = (base.content.chars().count(), cand.content.chars().count());
                let degenerate = foreign_chars(&cand.content);
                println!(
                    "demande {round} : référence {n_base} car., avec amendement {n_cand} car., {degenerate} caractère(s) étranger(s)"
                );
                assert!(!cand.content.trim().is_empty(), "réponse vide avec l'amendement");
                assert_eq!(degenerate, 0, "dérive d'écritures étrangères avec l'amendement");
                // Un amendement qui écrase la réponse de moitié au moins est
                // suspect (instruction envahissante) : filet large, -70 %.
                assert!(
                    n_cand * 10 >= n_base * 3,
                    "l'amendement écrase la réponse : {n_base} → {n_cand} caractères"
                );
            }
            (Err(error), _) | (_, Err(error)) => panic!("appel du modèle : {error}"),
        }
    }
    println!("filet passé : {} demande(s), aucun recul", corpus.len());
}
