//! Filet de l'âme auto-éditée (`data/SOUL.md`, lot « croissance », inspiré
//! de GEPA ; remplace l'ancien filet des amendements).
//!
//! Jimy fait évoluer son âme seul : ce filet vérifie, à la demande, qu'elle
//! ne l'a pas abîmé. Sur les dernières demandes réelles de l'utilisateur
//! (lues dans la vraie base), la réponse produite AVEC l'âme testée doit
//! rester saine — non vide, sans dérive d'écritures étrangères — et de
//! longueur comparable à la réponse avec l'âme d'ORIGINE. Aucun outil : on
//! ne valide ici que l'effet des instructions sur les réponses texte.
//!
//!   cargo test --test soul '--' --ignored --nocapture
//!
//! L'âme testée : `data/SOUL.md` (dossier de données réel), ou `JIMMY_SOUL`
//! pour une âme candidate pas encore écrite.

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
async fn l_ame_ne_degrade_pas_les_reponses() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).is_test(true).try_init();
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf();
    let _ = jimmy_agent::paths::load_dotenv(&workspace.join(".env"));
    let paths = Paths { data: workspace.join("data"), app: workspace.clone(), dev: true };
    let app = App::new(paths, Secrets::from_env()).expect("app");
    let settings = app.settings();

    let soul = std::env::var("JIMMY_SOUL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| jimmy_agent::soul::read_for_prompt(&workspace.join("data")));
    let origin = jimmy_agent::soul::DEFAULT_SOUL.trim().to_string();
    assert!(soul.trim() != origin, "âme identique à l'origine : rien à valider");
    println!("âme testée : {} caractères", soul.chars().count());

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
        let common = |soul: Option<String>| PromptContext {
            settings: &settings,
            memory: &app.memory,
            skills: &app.skills,
            registry: &app.registry,
            request,
            memory_block: memory_block.clone(),
            vault_block: vault_block.clone(),
            recent_tools: None,
            earlier: None,
            chantiers: None,
            work_notes: None,
            soul,
            background_task: None,
            situation: Vec::new(),
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
            .chat(&settings.llm.model, &messages(prompt::build_system(&common(Some(soul.clone())))), &specs, Some(0.0), settings.llm.max_tokens)
            .await;
        match (base, candidate) {
            (Ok(base), Ok(cand)) => {
                let (n_base, n_cand) = (base.content.chars().count(), cand.content.chars().count());
                let degenerate = foreign_chars(&cand.content);
                println!(
                    "demande {round} : référence {n_base} car., avec l'âme testée {n_cand} car., {degenerate} caractère(s) étranger(s)"
                );
                assert!(!cand.content.trim().is_empty(), "réponse vide avec l'âme testée");
                assert_eq!(degenerate, 0, "dérive d'écritures étrangères avec l'âme testée");
                // Une âme qui écrase la réponse de moitié au moins est
                // suspect (instruction envahissante) : filet large, -70 %.
                assert!(
                    n_cand * 10 >= n_base * 3,
                    "l'âme écrase la réponse : {n_base} → {n_cand} caractères"
                );
            }
            (Err(error), _) | (_, Err(error)) => panic!("appel du modèle : {error}"),
        }
    }
    println!("filet passé : {} demande(s), aucun recul", corpus.len());
}

/// A/B du Fast-Path (28 octobre) : pendant une tâche de fond, une question en
/// parallèle (« Réponds uniquement par le mot : cerise ») doit recevoir sa
/// réponse, pas celle de la tâche (parcours UI « tâche longue »). Compare
/// l'ancien socle Fast-Path (identité courte, avant l'âme) et le nouveau
/// (âme + socle), mêmes messages, `JIMMY_AB_ROUNDS` appels chacun.
///
///   cargo test --test soul fast_path_ab '--' --ignored --nocapture
#[tokio::test(flavor = "multi_thread")]
#[ignore = "appelle le vrai modèle ; à lancer explicitement"]
async fn fast_path_ab_reponse_en_parallele() {
    const OLD_FAST: &str = "Tu es Jimy (« Agent Jimy »), un assistant personnel agentique qui vit sur le bureau de l'utilisateur, incarné par un renard humanoïde 3D. Ton nom s'écrit **Jimy** (souvent transcrit « Jimmy »).\n\n## Sortie : courte, nette, précise\nRéponds en français, sauf indication contraire. Va droit au but : une à trois phrases suffisent par défaut.\n- Commence par la réponse, sans préambule ni reformulation de la question.\n- La prose est lue à voix haute telle quelle : une seule idée par phrase, pas de symboles, pas de tableau, pas de code ni de chemin de fichier dans la prose.\n- Si l'utilisateur apprend ou corrige un fait stable sur lui-même, enregistre-le avec `remember`.";
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf();
    let _ = jimmy_agent::paths::load_dotenv(&workspace.join(".env"));
    let paths = Paths { data: workspace.join("data"), app: workspace.clone(), dev: true };
    let app = App::new(paths, Secrets::from_env()).expect("app");
    let settings = app.settings();
    let rounds: usize = std::env::var("JIMMY_AB_ROUNDS").ok().and_then(|v| v.parse().ok()).unwrap_or(3);

    let first = "Exécute avec run_command la commande : Start-Sleep -Seconds 70; 'fini-fond'. Puis réponds uniquement par ce qu'elle affiche.";
    let task = format!(
        "- Tâche en arrière-plan : « {first} » (dans cette conversation), lancée il y a 45 s, dernière étape : run_command. \
         Elle continue sans toi : réponds à la nouvelle demande normalement. Si on te demande où elle en est, \
         réponds d'après cette ligne. Ne refais pas son travail et ne modifie pas les fichiers qu'elle touche. \
         Pour l'arrêter : bouton « Arrêter » de la tâche dans le Chat, ou « arrête tout » à la voix."
    );
    let request = "Réponds uniquement par le mot : cerise";
    // Le bloc mémoire du Fast-Path, comme dans les vrais tours.
    let memory_block = prompt::recall_for_fast(&app.memory, &settings, request);
    let ctx = PromptContext {
        settings: &settings,
        memory: &app.memory,
        skills: &app.skills,
        registry: &app.registry,
        request,
        memory_block: memory_block.clone(),
        vault_block: None,
        recent_tools: None,
        earlier: None,
        chantiers: None,
        work_notes: None,
        soul: None,
        background_task: Some(task.clone()),
        situation: Vec::new(),
        voice: false,
    };
    let new_system = prompt::join_parts(&prompt::system_parts_fast(&ctx));
    let old_system = format!("{OLD_FAST}\n\n## Maintenant\n{task}\n\n{}", memory_block.unwrap_or_default());
    let all = app.registry.specs();
    let variants = [
        ("ancien", old_system, all.iter().filter(|s| s.name == "remember").cloned().collect::<Vec<_>>()),
        ("nouveau", new_system, all.iter().filter(|s| s.name == "remember" || s.name == "soul_update").cloned().collect()),
    ];
    for (label, system, specs) in &variants {
        let mut ok = 0;
        for _ in 0..rounds {
            let messages = vec![
                Message::system(system.clone()),
                Message::user(first.to_string()),
                Message::assistant("(Je m'en occupe en arrière-plan.)".to_string()),
                Message::user(request.to_string()),
            ];
            let reply = app.llm.chat(&settings.llm.model, &messages, specs, None, settings.llm.max_tokens).await.expect("modèle");
            let text = reply.content.trim().to_string();
            if text.to_lowercase().contains("cerise") {
                ok += 1;
            }
            println!("{label} : « {} »", text.chars().take(80).collect::<String>());
        }
        println!("== {label} : {ok}/{rounds} cerise");
    }
}
