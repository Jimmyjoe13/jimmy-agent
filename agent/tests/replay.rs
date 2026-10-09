//! Diagnostic : rejoue la dernière requête d'un tour réel, plusieurs fois et
//! avec plusieurs températures, pour savoir si une réponse dégénérée (texte
//! qui part en chinois, russe, mots répétés) vient du modèle ou du contexte.
//!
//! Préparation (jamais sur les données réelles) : un dossier `JIMMY_REPLAY_DIR`
//! contenant `full.db` (copie complète) et `data/` (config + copie de la base
//! tronquée juste avant la demande). `JIMMY_REPLAY_SESSION` = début de
//! l'identifiant de session, `JIMMY_REPLAY_ROUNDS` = essais par température.
//!
//!   cargo test --test replay '--' --ignored --nocapture

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use jimmy_agent::config::Secrets;
use jimmy_agent::core::history::History;
use jimmy_agent::core::prompt::{self, PromptContext};
use jimmy_agent::core::types::{Message, Role};
use jimmy_agent::db::Db;
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
#[ignore = "rejoue une requête réelle sur le modèle de langage"]
async fn rejouer_une_reponse_degeneree() {
    let dir = PathBuf::from(std::env::var("JIMMY_REPLAY_DIR").expect("JIMMY_REPLAY_DIR"));
    let prefix = std::env::var("JIMMY_REPLAY_SESSION").expect("JIMMY_REPLAY_SESSION");
    let rounds: usize = std::env::var("JIMMY_REPLAY_ROUNDS").ok().and_then(|v| v.parse().ok()).unwrap_or(3);

    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf();
    let _ = jimmy_agent::paths::load_dotenv(&workspace.join(".env"));
    let paths = Paths { data: dir.join("data"), app: workspace, dev: true };
    let app = App::new(paths, Secrets::from_env()).expect("app");
    let settings = app.settings();

    // Le tour à rejouer, lu dans la copie complète.
    let full = History::new(Arc::new(Mutex::new(Db::open(&dir.join("full.db")).expect("full.db"))));
    let session = full
        .sessions(200)
        .unwrap()
        .into_iter()
        .find(|s| s.id.starts_with(&prefix))
        .expect("session")
        .id;
    let all = full.messages(&session, usize::MAX).unwrap();
    let kept = app.history.messages(&session, usize::MAX).unwrap().len();
    let turn = &all[kept..];
    let request = turn.first().filter(|m| m.role == Role::User).expect("demande").content.clone();
    println!("demande rejouée : « {request} » ({} messages dans le tour)", turn.len());

    // Contexte construit comme `agent::run` le construit.
    let memory_block = prompt::recall_for(&app.memory, &settings, &request).await;
    let vault_block = prompt::vault_context(app.vault.as_ref(), &settings, &request).await;
    let system = prompt::build_system(&PromptContext {
        settings: &settings,
        memory: &app.memory,
        skills: &app.skills,
        registry: &app.registry,
        request: &request,
        memory_block,
        vault_block,
        // `JIMMY_REPLAY_NO_DIGEST=1` : sans le résumé des outils récents.
        recent_tools: if std::env::var_os("JIMMY_REPLAY_NO_DIGEST").is_some() {
            None
        } else {
            app.history.tool_digest(&session, 3, 1500).ok().flatten()
        },
        // `JIMMY_AMENDMENT` : teste un amendement candidat sur cette requête.
        amendments: std::env::var("JIMMY_AMENDMENT").ok().filter(|a| !a.trim().is_empty()),
        background_task: None,
        voice: false,
    });
    let mut messages = vec![Message::system(system)];
    messages.extend(app.history.conversation(&session, 20).unwrap());
    messages.push(Message::user(request.clone()));
    // Appels d'outils et résultats du tour, jusqu'à la réponse finale exclue.
    let mut ids = std::collections::VecDeque::new();
    for m in &turn[1..] {
        match m.role {
            Role::Assistant if m.tool_calls.as_ref().is_some_and(|c| !c.is_empty()) => {
                ids.extend(m.tool_calls.as_ref().unwrap().iter().map(|c| c.id.clone()));
                messages.push(m.clone());
            }
            Role::Tool => {
                let id = ids.pop_front().unwrap_or_default();
                messages.push(Message::tool_result(id, m.name.clone().unwrap_or_default(), m.content.clone()));
            }
            _ => break,
        }
    }
    let chars: usize = messages.iter().map(|m| m.content.chars().count()).sum();
    println!("{} messages, {chars} caractères ; modèle {}", messages.len(), settings.llm.model);

    // `JIMMY_REPLAY_NO_TOOLS=1` : force une réponse texte (teste la génération).
    let specs = if std::env::var_os("JIMMY_REPLAY_NO_TOOLS").is_some() { Vec::new() } else { app.registry.specs() };
    // `JIMMY_REPLAY_MODEL` : un autre modèle ; `JIMMY_REPLAY_TEMPS` : « none,0.3 ».
    let model = std::env::var("JIMMY_REPLAY_MODEL").unwrap_or_else(|_| settings.llm.model.clone());
    let temperatures: Vec<Option<f32>> = std::env::var("JIMMY_REPLAY_TEMPS")
        .unwrap_or_else(|_| "none,0.7,0.3".into())
        .split(',')
        .map(|t| t.trim().parse::<f32>().ok())
        .collect();
    println!("modèle rejoué : {model}");
    for temperature in temperatures {
        let mut degenerate = 0;
        for round in 1..=rounds {
            match app.llm.chat(&model, &messages, &specs, temperature, settings.llm.max_tokens).await {
                Ok(reply) => {
                    let bad = foreign_chars(&reply.content);
                    if bad > 0 {
                        degenerate += 1;
                    }
                    println!(
                        "  température {temperature:?} essai {round} : {} caractères, {bad} étrangers, {} outil(s){}",
                        reply.content.chars().count(),
                        reply.tool_calls.len(),
                        if std::env::var_os("JIMMY_REPLAY_SHOW").is_some() {
                            // `JIMMY_REPLAY_SHOW=1` : le texte de la réponse (ex. tour sans outil).
                            let names: Vec<&str> = reply.tool_calls.iter().map(|c| c.name.as_str()).collect();
                            format!(
                                " {names:?} — « {} »",
                                reply.content.chars().take(240).collect::<String>().replace('\n', " ")
                            )
                        } else if bad > 0 {
                            // Extrait autour du premier caractère étranger (en caractères, pas en octets).
                            let chars: Vec<char> = reply.content.chars().collect();
                            let first = chars.iter().position(|c| foreign_chars(&c.to_string()) > 0).unwrap_or(0);
                            let excerpt: String = chars[first.saturating_sub(60)..].iter().take(120).collect();
                            format!(" — « …{excerpt} »")
                        } else {
                            String::new()
                        }
                    );
                }
                Err(error) => println!("  température {temperature:?} essai {round} : erreur {error}"),
            }
        }
        println!("température {temperature:?} : {degenerate}/{rounds} réponse(s) dégénérée(s)");
    }
}

/// Mesure : poids des définitions d'outils envoyées à chaque appel au modèle,
/// outils MCP compris (les serveurs de la vraie configuration sont lancés).
#[tokio::test(flavor = "multi_thread")]
#[ignore = "lance les serveurs MCP de la configuration réelle"]
async fn mesurer_le_poids_des_definitions_d_outils() {
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf();
    let _ = jimmy_agent::paths::load_dotenv(&workspace.join(".env"));
    let paths = Paths { data: workspace.join("data"), app: workspace, dev: true };
    let app = App::new(paths, Secrets::from_env()).expect("app");
    app.start_mcp().await;
    let specs = app.registry.specs();
    let size = |s: &jimmy_agent::core::types::ToolSpec| s.to_wire().to_string().chars().count();
    let (mcp, local): (Vec<_>, Vec<_>) = specs.iter().partition(|s| s.name.starts_with("mcp_"));
    let mcp_chars: usize = mcp.iter().map(|s| size(s)).sum();
    let local_chars: usize = local.iter().map(|s| size(s)).sum();
    println!("outils locaux : {} ({} caractères ≈ {} jetons)", local.len(), local_chars, local_chars / 4);
    println!("outils MCP    : {} ({} caractères ≈ {} jetons)", mcp.len(), mcp_chars, mcp_chars / 4);
    app.mcp.shutdown().await;
}
