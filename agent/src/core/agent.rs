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

pub struct AgentDeps {
    pub llm: Arc<LlmClient>,
    pub history: Arc<History>,
    pub memory: Arc<crate::memory::MemoryStore>,
    pub skills: Arc<crate::skills::SkillStore>,
    pub synaptiq: Option<Arc<SynaptiqClient>>,
    pub registry: Arc<ToolRegistry>,
}

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
    let memory_block = prompt::recall_for(&deps.memory, &settings, &request);
    let synaptiq_block =
        prompt::synaptiq_context(deps.synaptiq.as_ref(), &settings, &request).await;
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
    }))];

    // 2. Historique récent de la session.
    messages.extend(deps.history.messages(&session_id, 20)?);
    messages.push(Message::user(request.clone()));

    deps.history
        .auto_title(&session_id, &request)
        .catch();
    deps.history
        .append(&session_id, &Message::user(request.clone()))
        .catch();

    let specs = deps.registry.specs();
    let max_iterations = settings.llm.max_iterations.max(1);
    let deadline = started + Duration::from_secs(600);
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

        let reply = deps
            .llm
            .chat(
                &settings.llm.model,
                &messages,
                &specs,
                settings.llm.temperature,
                settings.llm.max_tokens,
            )
            .await?;

        if !reply.content.trim().is_empty() {
            final_text = reply.content.trim().to_string();
        }

        if reply.tool_calls.is_empty() {
            messages.push(Message::assistant(reply.content.clone()));
            deps.history
                .append(&session_id, &Message::assistant(reply.content.clone()))
                .catch();
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

    if final_text.is_empty() {
        final_text = "Je n'ai pas pu formuler de réponse à cette demande.".into();
    }

    // 3. Mémorisation : ce que la conversation apprend de stable.
    if settings.memory.enabled {
        let learned = crate::memory::learn::learn(&deps.llm, &settings, &request, &final_text).await;
        for (kind, content) in learned {
            match deps.memory.remember(kind, &content, "auto", 0.55) {
                Ok(_) => emit(
                    &mut events,
                    AgentEvent::Memory {
                        action: "learn".into(),
                        detail: content,
                    },
                )
                .await,
                Err(error) => log::warn!("[memory] écriture impossible : {error}"),
            }
        }
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

    Ok(AgentAnswer {
        session_id,
        text: final_text,
        tools_used,
        duration_ms: started.elapsed().as_millis() as u64,
    })
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