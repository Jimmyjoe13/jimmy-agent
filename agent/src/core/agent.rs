//! Boucle agentique.
//!
//! Le principe tient en peu de lignes : on demande au modèle, on exécute ce
//! qu'il demande, on lui renvoie le résultat, on recommence — jusqu'à ce qu'il
//! réponde sans appeler d'outil.
//!
//! Trois garde-fous qui évitent les boucles coûteuses :
//!
//! * un **budget d'itérations** par demande ;
//! * une **mémoire d'appels** par demande : appeler deux fois le même outil
//!   avec les mêmes arguments renvoie le premier résultat au lieu de le
//!   recalculer (et signale la répétition au modèle) ;
//! * une **borne de temps** globale.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::config::Settings;
use crate::core::history::History;
use crate::core::prompt::{self, PromptContext};
use crate::core::types::{AgentAnswer, AgentEvent, AvatarState, Message};
use crate::error::{Error, Result};
use crate::providers::llm::LlmClient;
use crate::synaptiq::SynaptiqClient;
use crate::tools::{ToolContext, ToolRegistry};

#[derive(Clone)]
pub struct AgentDeps {
    pub llm: Arc<LlmClient>,
    pub history: Arc<History>,
    pub memory: Arc<crate::memory::MemoryStore>,
    pub skills: Arc<crate::skills::SkillStore>,
    pub synaptiq: Option<Arc<SynaptiqClient>>,
    pub registry: Arc<ToolRegistry>,
    /// Demande dite à voix haute : réponse courte, peu d'étapes, et on fait
    /// répéter une phrase incohérente plutôt que de partir l'explorer.
    pub voice: bool,
}

/// Budget d'étapes d'une demande vocale : l'utilisateur attend la réponse en
/// silence, une longue exploration casse la conversation.
const VOICE_MAX_ITERATIONS: u32 = 6;

impl AgentDeps {
    pub fn tool_context(&self, settings: &Settings) -> ToolContext {
        ToolContext {
            workspace: std::path::PathBuf::from(&settings.workspace),
            permissions: Arc::new(std::sync::RwLock::new(
                crate::permissions::Permissions::default(),
            )),
            memory: self.memory.clone(),
            skills: self.skills.clone(),
            synaptiq: self.synaptiq.clone(),
        }
    }
}

/// Enveloppe une fonction asynchrone pour la diffuser en flux d'événements.
///
/// On évite une dépendance à `async-stream` : le canal `tokio::sync::mpsc`
/// suffit, et le code reste explicite.
pub async fn run(
    deps: Arc<AgentDeps>,
    settings: Settings,
    session_id: String,
    request: String,
    tool_context: ToolContext,
    mut events: tokio::sync::mpsc::Sender<AgentEvent>,
) -> Result<AgentAnswer> {
    let started = Instant::now();

    emit(
        &mut events,
        AgentEvent::State {
            state: AvatarState::Thinking,
            detail: request.chars().take(80).collect(),
        },
    )
    .await;

    // 1. Contexte : mémoire locale puis, si la règle le justifie, Synaptiq.
    let step = Instant::now();
    let memory_block = prompt::recall_for(&deps.memory, &settings, &request).await;
    log::info!("[agent] mémoire consultée en {} ms", step.elapsed().as_millis());
    let step = Instant::now();
    let synaptiq_block =
        prompt::synaptiq_context(deps.synaptiq.as_ref(), &settings, &request).await;
    log::info!(
        "[agent] Synaptiq en {} ms ({})",
        step.elapsed().as_millis(),
        if synaptiq_block.is_some() { "contexte injecté" } else { "rien" }
    );
    if synaptiq_block.is_some() {
        emit(
            &mut events,
            AgentEvent::Synaptiq {
                action: "context".into(),
                detail: "contexte antérieur injecté dans la demande".into(),
            },
        )
        .await;
    }

    let mut messages = vec![Message::system(prompt::build_system(&PromptContext {
        settings: &settings,
        memory: &deps.memory,
        skills: &deps.skills,
        registry: &deps.registry,
        request: &request,
        memory_block: memory_block.clone(),
        synaptiq_block,
        voice: deps.voice,
    }))];

    // 2. Historique récent de la session.
    messages.extend(deps.history.conversation(&session_id, 20)?);
    messages.push(Message::user(request.clone()));

    deps.history
        .auto_title(&session_id, &request)
        .catch();
    deps.history
        .append(&session_id, &Message::user(request.clone()))
        .catch();

    let mut specs = deps.registry.specs();
    if deps.voice {
        // Le contexte mémoire est déjà injecté dans le prompt : ces deux
        // outils ne font qu'ajouter des allers-retours au modèle (2 à 4 s
        // chacun), alors que la conversation est à voix haute.
        specs.retain(|spec| !matches!(spec.name.as_str(), "search_memory" | "synaptiq_search"));
    }
    let max_iterations = if deps.voice {
        settings.llm.max_iterations.clamp(1, VOICE_MAX_ITERATIONS)
    } else {
        settings.llm.max_iterations.max(1)
    };
    // À voix haute, une longue exploration est une panne silencieuse : 90 s au
    // plus, puis réponse avec ce qu'on a (mesuré : 139 s d'attente sur « analyse
    // ce dossier »).
    let deadline = started + Duration::from_secs(if deps.voice { 90 } else { 600 });
    // Modèle vocal facultatif : un modèle plus rapide pour la conversation.
    let model = if deps.voice && !settings.llm.voice_model.trim().is_empty() {
        settings.llm.voice_model.trim().to_string()
    } else {
        settings.llm.model.clone()
    };
    let mut completed = false;
    let mut cache: HashMap<String, String> = HashMap::new();
    let mut tools_used: Vec<String> = Vec::new();
    let mut final_text = String::new();

    for iteration in 0..max_iterations {
        if Instant::now() > deadline {
            emit(
                &mut events,
                AgentEvent::Notice {
                    message: "limite de temps atteinte pour cette demande".into(),
                },
            )
            .await;
            break;
        }

        let call_started = Instant::now();
        let first_try = deps
            .llm
            .chat(&model, &messages, &specs, settings.llm.temperature, settings.llm.max_tokens)
            .await;
        let reply = match first_try {
            Ok(reply) => reply,
            // Filet de sécurité : si le fournisseur refuse la requête (400) dès
            // le premier appel, l'historique est suspect — on réessaie avec la
            // seule demande plutôt que de laisser la conversation morte.
            Err(error) if iteration == 0 && is_bad_request(&error) && messages.len() > 2 => {
                log::warn!("[agent] requête refusée, historique ignoré pour ce tour : {error}");
                emit(
                    &mut events,
                    AgentEvent::Notice {
                        message: "L'historique de cette conversation a été ignoré (requête refusée).".into(),
                    },
                )
                .await;
                let system = messages.remove(0);
                messages = vec![system, Message::user(request.clone())];
                deps.llm
                    .chat(&model, &messages, &specs, settings.llm.temperature, settings.llm.max_tokens)
                    .await?
            }
            Err(error) => return Err(error),
        };
        log::info!(
            "[agent] appel au modèle n°{} en {} ms ({} outil(s), {} caractères ; jetons : {} en entrée, {} en sortie)",
            iteration + 1,
            call_started.elapsed().as_millis(),
            reply.tool_calls.len(),
            reply.content.chars().count(),
            reply.usage.prompt_tokens,
            reply.usage.completion_tokens
        );

        if !reply.content.trim().is_empty() {
            final_text = reply.content.trim().to_string();
        }

        if reply.tool_calls.is_empty() {
            messages.push(Message::assistant(reply.content.clone()));
            deps.history
                .append(&session_id, &Message::assistant(reply.content.clone()))
                .catch();
            completed = true;
            break;
        }

        // Le modèle travaille : on le montre.
        emit(
            &mut events,
            AgentEvent::State {
                state: AvatarState::Executing,
                detail: format!("{}/{}", iteration + 1, max_iterations),
            },
        )
        .await;

        let mut assistant = Message::assistant(reply.content.clone());
        assistant.tool_calls = Some(reply.tool_calls.clone());
        messages.push(assistant);
        deps.history
            .append(&session_id, &Message {
                role: crate::core::types::Role::Assistant,
                content: reply.content.clone(),
                tool_calls: Some(reply.tool_calls.clone()),
                tool_call_id: None,
                name: None,
            })
            .catch();

        for call in reply.tool_calls {
            emit(
                &mut events,
                AgentEvent::ToolStart {
                    call_id: call.id.clone(),
                    name: call.name.clone(),
                    arguments: call.arguments.clone(),
                },
            )
            .await;

            let key = format!("{}:{}", call.name, call.arguments);
            let (text, duration_ms) = if let Some(previous) = cache.get(&key) {
                (
                    format!("Résultat déjà obtenu à l'étape précédente, réutilisé :\n{previous}"),
                    0u64,
                )
            } else {
                let started_tool = Instant::now();
                match execute(&deps, &call, &tool_context).await {
                    Ok(text) => {
                        cache.insert(key, text.clone());
                        let elapsed = started_tool.elapsed().as_millis() as u64;
                        tools_used.push(call.name.clone());
                        emit(
                            &mut events,
                            AgentEvent::ToolEnd {
                                call_id: call.id.clone(),
                                name: call.name.clone(),
                                ok: true,
                                summary: summarize(&text),
                                duration_ms: elapsed,
                            },
                        )
                        .await;
                        (text, elapsed)
                    }
                    Err(error) => {
                        // Une erreur d'outil n'est pas une erreur de Jimmy : on
                        // la renvoie au modèle pour qu'il se corrige.
                        emit(
                            &mut events,
                            AgentEvent::ToolEnd {
                                call_id: call.id.clone(),
                                name: call.name.clone(),
                                ok: false,
                                summary: error.to_string(),
                                duration_ms: started_tool.elapsed().as_millis() as u64,
                            },
                        )
                        .await;
                        (format!("ERREUR : {error}"), 0u64)
                    }
                }
            };
            let _ = duration_ms;

            messages.push(Message::tool_result(call.id.clone(), call.name.clone(), text.clone()));
            deps.history
                .append(&session_id, &Message::tool_result(call.id, call.name, text))
                .catch();
        }
    }

    // Limite d'étapes ou de temps atteinte : le dernier texte du modèle n'était
    // souvent qu'une phrase d'annonce (« Je lis la documentation… »), lue telle
    // quelle à voix haute. Un dernier appel, sans outil, lui demande de conclure
    // avec ce qu'il a trouvé. Et la réponse entre dans l'historique : sans cela,
    // le tour suivant voyait une question restée sans réponse.
    if !completed {
        emit(
            &mut events,
            AgentEvent::Notice { message: "Limite atteinte : je conclus avec ce que j'ai trouvé.".into() },
        )
        .await;
        messages.push(Message::user(
            "Tu as atteint la limite d'étapes pour cette demande. Réponds maintenant, sans appeler d'outil, \
             avec ce que tu as trouvé : sois bref, et dis honnêtement ce qu'il reste à faire."
                .to_string(),
        ));
        if let Ok(reply) = deps
            .llm
            .chat(&model, &messages, &[], settings.llm.temperature, settings.llm.max_tokens)
            .await
        {
            if !reply.content.trim().is_empty() {
                final_text = reply.content.trim().to_string();
            }
        }
        if final_text.is_empty() {
            final_text = "Je n'ai pas pu terminer cette demande dans le temps imparti.".into();
        }
        deps.history
            .append(&session_id, &Message::assistant(final_text.clone()))
            .catch();
    }

    if final_text.is_empty() {
        final_text = "Je n'ai pas pu formuler de réponse à cette demande.".into();
    }

    deps.history.touch(&session_id).catch();

    emit(
        &mut events,
        AgentEvent::State {
            state: AvatarState::Speaking,
            detail: final_text.chars().take(80).collect(),
        },
    )
    .await;
    emit(
        &mut events,
        AgentEvent::Final {
            text: final_text.clone(),
        },
    )
    .await;

    // 3. Mémorisation, **après** la réponse et en tâche de fond. Avant, elle
    // se faisait ici même, avant d'envoyer la réponse : un appel de plus au
    // modèle (3 à 5 s) retardait chaque réponse — mesuré dans le journal.
    // `WeakSender` : la tâche ne retient pas le canal ouvert, sinon l'interface
    // attendrait la fin de l'apprentissage avant de lire la réponse.
    if settings.memory.enabled {
        let llm = deps.llm.clone();
        let memory = deps.memory.clone();
        let settings = settings.clone();
        let request = request.clone();
        let answer = final_text.clone();
        let weak = events.downgrade();
        tokio::spawn(async move {
            let learned = crate::memory::learn::learn(&llm, &settings, &request, &answer).await;
            for (kind, content) in learned {
                match memory.remember_indexed(kind, &content, "auto", 0.55).await {
                    Ok(_) => {
                        if let Some(events) = weak.upgrade() {
                            let _ = events
                                .send(AgentEvent::Memory { action: "learn".into(), detail: content })
                                .await;
                        }
                    }
                    Err(error) => log::warn!("[memory] écriture impossible : {error}"),
                }
            }
        });
    }

    Ok(AgentAnswer {
        session_id,
        text: final_text,
        tools_used,
        duration_ms: started.elapsed().as_millis() as u64,
    })
}

/// La requête a-t-elle été refusée par le fournisseur (HTTP 400) ?
fn is_bad_request(error: &Error) -> bool {
    matches!(error, Error::Provider { message, .. } if message.contains("HTTP 400"))
}

async fn execute(
    deps: &AgentDeps,
    call: &crate::core::types::ToolCall,
    ctx: &ToolContext,
) -> Result<String> {
    let Some(tool) = deps.registry.get(&call.name) else {
        return Err(Error::UnknownTool {
            name: call.name.clone(),
        });
    };
    tool.call(&call.arguments, ctx).await
}

fn summarize(text: &str) -> String {
    text.lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .chars()
        .take(120)
        .collect()
}

async fn emit(events: &mut tokio::sync::mpsc::Sender<AgentEvent>, event: AgentEvent) {
    // Un frontend fermé ne doit pas faire échouer la demande : on ignore.
    let _ = events.send(event).await;
}

/// Petit utilitaire : exécute une opération d'historique sans interrompre la
/// demande si elle échoue. Un historique incomplet est moins grave qu'une
/// réponse perdue.
trait Catch {
    fn catch(self);
}

impl Catch for Result<()> {
    fn catch(self) {
        if let Err(error) = &self {
            log::warn!("[history] {error}");
        }
    }
}

impl Catch for Result<String> {
    fn catch(self) {
        if let Err(error) = &self {
            log::warn!("[history] {error}");
        }
    }
}