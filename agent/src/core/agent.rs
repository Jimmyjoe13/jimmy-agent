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
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::config::Settings;
use crate::core::history::History;
use crate::core::prompt::{self, PromptContext};
use crate::core::types::{AgentAnswer, AgentEvent, AvatarState, Message};
use crate::error::{Error, Result};
use crate::providers::llm::LlmClient;
use crate::memory::vault::Vault;
use crate::tools::{ToolContext, ToolRegistry};

#[derive(Clone)]
pub struct AgentDeps {
    pub llm: Arc<LlmClient>,
    pub history: Arc<History>,
    pub memory: Arc<crate::memory::MemoryStore>,
    pub skills: Arc<crate::skills::SkillStore>,
    pub vault: Option<Arc<Vault>>,
    pub registry: Arc<ToolRegistry>,
    /// Dossier de données de Jimmy : les amendements du prompt (lot
    /// « croissance ») y sont lus au début de chaque demande.
    pub data_dir: PathBuf,
    /// Demandes d'autorisation en attente : modifier un fichier sensible
    /// (`.env`, clés, secrets) suspend l'outil jusqu'à l'accord du Chat.
    pub approvals: Arc<crate::sensitive::Approvals>,
    /// Demande dite à voix haute : réponse courte, peu d'étapes, et on fait
    /// répéter une phrase incohérente plutôt que de partir l'explorer.
    pub voice: bool,
}

/// Budget d'étapes d'une demande vocale : l'utilisateur attend la réponse en
/// silence, une longue exploration casse la conversation.
// 6 étapes ne suffisaient pas à une vraie tâche : chaque « go » finissait par
// une conclusion forcée et une question, et rien n'était jamais écrit (cas
// réel du 4 octobre : quatre « oui » d'affilée, `notify.py` jamais créé). Une
// tâche de code prend 15 à 20 étapes avec MiMo : la voix a désormais le même
// budget que l'écrit, et dit où elle en est (`AgentEvent::Progress`).
const VOICE_MAX_ITERATIONS: u32 = 25;

impl AgentDeps {
    pub fn tool_context(&self, settings: &Settings) -> ToolContext {
        ToolContext {
            workspace: std::path::PathBuf::from(&settings.workspace),
            permissions: Arc::new(std::sync::RwLock::new(
                crate::permissions::Permissions::default(),
            )),
            memory: self.memory.clone(),
            skills: self.skills.clone(),
            vault: self.vault.clone(),
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

    // 1. Contexte : mémoire locale puis, si la règle le justifie, le vault.
    let step = Instant::now();
    let memory_block = prompt::recall_for(&deps.memory, &settings, &request).await;
    log::info!("[agent] mémoire consultée en {} ms", step.elapsed().as_millis());
    let step = Instant::now();
    let vault_block = prompt::vault_context(deps.vault.as_ref(), &settings, &request).await;
    log::info!(
        "[agent] vault en {} ms ({})",
        step.elapsed().as_millis(),
        if vault_block.is_some() { "contexte injecté" } else { "rien" }
    );
    if vault_block.is_some() {
        emit(
            &mut events,
            AgentEvent::Vault {
                action: "context".into(),
                detail: "notes du vault injectées dans la demande".into(),
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
        vault_block,
        // 3 derniers tours, ≤ 1 500 caractères : le fil des outils sans
        // rejouer leurs résultats bruts (piège 45).
        recent_tools: deps.history.tool_digest(&session_id, 3, 1500).ok().flatten(),
        // Amendements (lot « croissance ») : des instructions additionnelles
        // actées avec l'utilisateur. Absente ou vide : prompt inchangé.
        amendments: crate::growth::read_amendments(&deps.data_dir),
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
    // Les outils MCP passent par `mcp_call` : leurs ~79 définitions (~9 500
    // jetons) ne partent plus à chaque appel (voir `tools::mcp`).
    specs.retain(|spec| !crate::mcp::is_proxy_tool(&spec.name));
    if deps.voice {
        // Le contexte mémoire est déjà injecté dans le prompt : ces deux
        // outils ne font qu'ajouter des allers-retours au modèle (2 à 4 s
        // chacun), alors que la conversation est à voix haute.
        specs
            .retain(|spec| !matches!(spec.name.as_str(), "search_memory" | "vault_search"));
    }
    let max_iterations = if deps.voice {
        settings.llm.max_iterations.clamp(1, VOICE_MAX_ITERATIONS)
    } else {
        settings.llm.max_iterations.max(1)
    };
    // À voix haute, une longue exploration est une panne silencieuse : 90 s au
    // plus, puis réponse avec ce qu'on a (mesuré : 139 s d'attente sur « analyse
    // ce dossier »).
    // Même durée qu'à l'écrit : mesuré, MiMo met 3 à 4 minutes pour une tâche
    // de code ; 180 s coupaient chaque fois avant l'écriture des fichiers.
    let deadline = started + Duration::from_secs(600);
    // Modèle vocal facultatif : un modèle plus rapide pour la conversation.
    let model = if deps.voice && !settings.llm.voice_model.trim().is_empty() {
        settings.llm.voice_model.trim().to_string()
    } else {
        settings.llm.model.clone()
    };
    let mut completed = false;
    let mut cache: HashMap<String, String> = HashMap::new();
    let mut tools_used: Vec<String> = Vec::new();
    // Échecs d'outils de la trajectoire : matière du journal d'expérience
    // (lot « croissance »). Avant, une erreur d'outil était corrigée puis
    // oubliée — la même erreur pouvait se reproduire à chaque tâche similaire.
    let mut tool_errors: Vec<(String, String)> = Vec::new();
    // Point d'étape : lectures sans aucune action (voir `EXPLORATION_NUDGE_AT`).
    let mut reads = 0usize;
    let mut acts = 0usize;
    let mut nudges = 0u32;
    // Appels d'outil arrivés coupés ou en texte : redemandés au plus 2 fois.
    let mut repairs = 0u32;
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
        // En vocal, un appel ne peut pas bloquer plus de 45 s : le 4 octobre,
        // MiMo est resté muet 95 s avant un HTTP 500, l'utilisateur attendait
        // en silence. Au-delà, Error::Timeout → « la demande a pris trop de temps ».
        let budget = deps.voice.then_some(VOICE_CALL_TIMEOUT);
        let mut first_try =
            call_model_streaming(&deps, &model, &messages, &specs, &settings, budget, &events).await;
        // Erreur passagère du fournisseur (5xx, 429, coupure) qui arrive vite :
        // une seule nouvelle tentative. Une panne lente n'est pas rejouée, elle
        // doublerait l'attente. Sans flux : ce qui a pu être affiché du premier
        // essai ne doit pas être écrit deux fois.
        if let Err(error) = &first_try {
            if is_transient(error) && call_started.elapsed() < FAST_FAILURE {
                log::warn!("[agent] erreur passagère du modèle {model} ({error}) : nouvelle tentative");
                tokio::time::sleep(Duration::from_millis(800)).await;
                first_try = call_model(&deps, &model, &messages, &specs, &settings, budget).await;
            }
        }
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
            Err(error) => {
                // Sans cette ligne, une panne du fournisseur en mode vocal ne
                // laissait aucune trace dans le journal.
                log::error!(
                    "[agent] appel au modèle {model} en échec après {} ms : {error}",
                    call_started.elapsed().as_millis()
                );
                if iteration == 0 {
                    return Err(error);
                }
                // Des étapes sont déjà faites : on ne jette pas ce travail. La
                // conclusion (plus bas) résume ce qui a été fait et ce qui reste,
                // au lieu d'un échec sec qui perdait tout (cas réel : 119 s de
                // travail perdues sur un délai dépassé).
                emit(
                    &mut events,
                    AgentEvent::Notice { message: format!("Le modèle n'a pas répondu ({error}) : je résume ce qui est fait.") },
                )
                .await;
                break;
            }
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

        // Filet anti-dérive (piège 60) : une réponse finale qui part dans
        // d'autres écritures n'est ni enregistrée, ni affichée, ni lue à voix
        // haute. On la régénère une fois ; si la seconde déraille aussi, on
        // garde la partie saine. Coût : un appel de plus, seulement dans ce cas.
        let mut reply = reply;
        if reply.tool_calls.is_empty() && is_degenerate(&reply.content, &request) {
            log::warn!(
                "[agent] réponse dégénérée ({} caractères étrangers) : nouvelle génération",
                foreign_script_chars(&reply.content)
            );
            match deps
                .llm
                .chat(&model, &messages, &specs, settings.llm.temperature, settings.llm.max_tokens)
                .await
            {
                Ok(retry) if !(retry.tool_calls.is_empty() && is_degenerate(&retry.content, &request)) => {
                    log::info!("[agent] nouvelle génération saine ({} caractères)", retry.content.chars().count());
                    reply = retry;
                }
                _ => {
                    log::warn!("[agent] seconde génération dégénérée ou en échec : réponse coupée");
                    reply.content = cut_degenerate(&reply.content);
                }
            }
        }

        // Appel d'outil arrivé dans le texte (cas réel : `write_file` coupé par
        // la limite de longueur, affiché comme réponse finale et perdu).
        if reply.tool_calls.is_empty() && reply.content.contains("<tool_call>") {
            match crate::core::inline_tools::parse_inline_tool_calls(&reply.content) {
                Some((text, calls)) => {
                    log::warn!("[agent] {} appel(s) d'outil reçu(s) en texte : récupéré(s)", calls.len());
                    reply.content = text;
                    reply.tool_calls = calls;
                }
                None if repairs < 2 => {
                    repairs += 1;
                    log::warn!(
                        "[agent] appel d'outil coupé ou illisible (réponse tronquée : {}) : redemandé",
                        reply.truncated
                    );
                    messages.push(Message::assistant(crate::core::inline_tools::strip_tool_markup(&reply.content)));
                    messages.push(Message::user(REPAIR_NOTE.to_string()));
                    continue;
                }
                None => {
                    reply.content = crate::core::inline_tools::strip_tool_markup(&reply.content);
                }
            }
        }

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

        // À voix haute : dire où on en est (la boucle vocale n'en dit qu'une
        // phrase de temps en temps).
        if deps.voice {
            if let Some(text) = progress_phrase(&reply.content, &reply.tool_calls) {
                emit(&mut events, AgentEvent::Progress { text }).await;
            }
        }

        for call in &reply.tool_calls {
            if is_read_tool(&call.name) {
                reads += 1;
            } else {
                acts += 1;
            }
        }

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

            log::info!(
                "[agent] outil « {} » appelé {}",
                call.name,
                {
                    let args: String = call.arguments.to_string().chars().take(200).collect();
                    if args.is_empty() { "(sans argument)".to_string() } else { args }
                }
            );
            let key = format!("{}:{}", call.name, call.arguments);
            let (text, duration_ms) = if let Some(previous) = cache.get(&key) {
                (
                    format!("Résultat déjà obtenu à l'étape précédente, réutilisé :\n{previous}"),
                    0u64,
                )
            } else if let Some(refusal) = crate::sensitive::authorize(
                &deps.approvals,
                deps.voice,
                &call.name,
                &call.arguments,
                &events,
                crate::sensitive::APPROVAL_TIMEOUT,
            )
            .await
            {
                // Fichier sensible sans accord de l'utilisateur (cas réel du
                // 5 octobre : un `.env` de production réécrit sans rien
                // demander) : l'outil n'est pas exécuté, le modèle reçoit le
                // refus et la consigne de ne pas contourner. Pas dans
                // `tool_errors` : un refus voulu n'est pas une leçon à tirer.
                emit(
                    &mut events,
                    AgentEvent::ToolEnd {
                        call_id: call.id.clone(),
                        name: call.name.clone(),
                        ok: false,
                        summary: "refusé : fichier sensible, pas d'accord".into(),
                        duration_ms: 0,
                    },
                )
                .await;
                (refusal, 0u64)
            } else {
                let started_tool = Instant::now();
                match execute(&deps, &call, &tool_context).await {
                    Ok(text) => {
                        cache.insert(key, text.clone());
                        let elapsed = started_tool.elapsed().as_millis() as u64;
                        tools_used.push(call.name.clone());
                        log::info!(
                            "[agent] outil « {} » terminé en {elapsed} ms ({} caractères)",
                            call.name,
                            text.chars().count()
                        );
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
                        // la renvoie au modèle pour qu'il se corrige. Et elle
                        // alimente le journal d'expérience (leçon après coup).
                        tool_errors.push((call.name.clone(), error.to_string()));
                        log::warn!(
                            "[agent] outil « {} » en erreur en {} ms : {error}",
                            call.name,
                            started_tool.elapsed().as_millis()
                        );
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

        // Cas réel du 4 octobre : 20 lectures en 11 étapes, « j'ai tout lu, rien
        // d'écrit ». Après beaucoup de lectures sans aucune action, un rappel
        // (non enregistré dans l'historique) pousse à agir ou à conclure.
        // Mesuré : un seul rappel ne suffisait pas à MiMo (écriture à l'étape
        // 15) ; un second, plus ferme, suit à l'étape 10 si rien n'a bougé.
        let step = iteration + 1;
        if acts == 0 && reads >= EXPLORATION_READS {
            let nudge = match nudges {
                0 if step >= EXPLORATION_NUDGE_AT => Some(EXPLORATION_NUDGE),
                1 if step >= EXPLORATION_NUDGE_AGAIN_AT => Some(EXPLORATION_NUDGE_AGAIN),
                _ => None,
            };
            if let Some(text) = nudge {
                nudges += 1;
                log::info!("[agent] point d'étape n°{nudges} : {reads} lectures sans action, rappel d'agir");
                messages.push(Message::user(text.to_string()));
            }
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
        // Même budget que les autres appels : la conclusion ne doit pas, à son
        // tour, laisser l'utilisateur sans réponse pendant des minutes.
        let budget = deps.voice.then_some(VOICE_CALL_TIMEOUT);
        if let Ok(reply) = call_model(&deps, &model, &messages, &[], &settings, budget).await {
            if !reply.content.trim().is_empty() {
                final_text = reply.content.trim().to_string();
            }
        }
        if final_text.is_empty() {
            final_text = if tools_used.is_empty() {
                "Je n'ai pas pu terminer cette demande dans le temps imparti.".into()
            } else {
                format!(
                    "Je n'ai pas pu terminer : le modèle ne répond plus. J'avais déjà utilisé {} outil(s) ({}). Redis-moi « continue » pour reprendre.",
                    tools_used.len(),
                    tools_used.iter().rev().take(4).cloned().collect::<Vec<_>>().join(", ")
                )
            };
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
        let vault = deps.vault.clone();
        let llm = deps.llm.clone();
        let memory = deps.memory.clone();
        let skills = deps.skills.clone();
        let settings = settings.clone();
        let request = request.clone();
        let answer = final_text.clone();
        let tool_errors = tool_errors.clone();
        let tools_used = tools_used.clone();
        let weak = events.downgrade();
        tokio::spawn(async move {
            let learned =
                crate::memory::learn::learn(&llm, &settings, &request, &answer, &tool_errors).await;
            for (kind, source, content) in learned {
                let (importance, action) = if source == "lesson" {
                    (0.65, "lesson")
                } else {
                    (0.55, "learn")
                };
                match memory.remember_indexed(kind, &content, source, importance).await {
                    Ok(_) => {}
                    Err(error) => log::warn!("[memory] écriture impossible : {error}"),
                }
                // Même souvenir, côté vault Obsidian : une note datée que
                // l'utilisateur relit. Le vault est un complément : son
                // échec n'annule pas le souvenir en base.
                if let Some(vault) = &vault {
                    if let Err(error) = vault.remember(kind.as_str(), &content).await {
                        log::warn!("[vault] écriture impossible : {error}");
                    }
                }
                if let Some(events) = weak.upgrade() {
                    let _ = events
                        .send(AgentEvent::Memory { action: action.into(), detail: content })
                        .await;
                }
            }
            // Capture de compétence (Voyager) : une trajectoire qui a
            // mobilisé plusieurs outils se condense en skill réutilisable.
            // Un appel de plus, en tâche de fond, au-delà du seuil seulement :
            // une question simple n'apprend rien de procédural.
            if settings.skills.auto_capture && tools_used.len() >= 3 {
                match crate::skills::capture::capture(
                    &llm, &settings, &request, &answer, &tools_used, &skills,
                )
                .await
                {
                    Some(proposal) => {
                        let honed = skills.load(&proposal.name).is_ok();
                        match skills.write(&proposal.name, &proposal.description, &proposal.body) {
                            Ok(path) => {
                                log::info!(
                                    "[skills] compétence {} : {} ({})",
                                    if honed { "aiguisée" } else { "capturée" },
                                    proposal.name,
                                    path.display()
                                );
                                if let Some(events) = weak.upgrade() {
                                    let _ = events
                                        .send(AgentEvent::Memory {
                                            action: "skill".into(),
                                            detail: proposal.name.clone(),
                                        })
                                        .await;
                                }
                            }
                            Err(error) => log::warn!("[skills] capture non écrite : {error}"),
                        }
                    }
                    None => {}
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
/// Caractères d'écritures étrangères au français (cyrillique, kana, CJK,
/// hangul). Une réponse de Jimmy n'en contient pas, sauf si la demande en
/// contenait.
fn foreign_script_chars(text: &str) -> usize {
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

/// La réponse a-t-elle déraillé ? Cas réel (4 octobre, `space-bunny-free`) :
/// une réponse longue part en chinois, japonais et russe mêlés au français,
/// et la synthèse vocale la lit à voix haute. Rare et aléatoire (1 rejeu sur
/// 32, quelle que soit la température), mais jamais acceptable.
fn is_degenerate(reply: &str, request: &str) -> bool {
    foreign_script_chars(reply) >= 2 && foreign_script_chars(request) == 0
}

/// Garde la partie saine d'une réponse déraillée : jusqu'à la dernière fin de
/// phrase avant le premier caractère étranger, avec une note honnête.
fn cut_degenerate(reply: &str) -> String {
    let chars: Vec<char> = reply.chars().collect();
    let first = chars
        .iter()
        .position(|c| foreign_script_chars(&c.to_string()) > 0)
        .unwrap_or(chars.len());
    let head: String = chars[..first].iter().collect();
    let cut = head
        .rfind(['.', '!', '?', '\n'])
        .map(|i| head[..=i].to_string())
        .unwrap_or(head);
    format!(
        "{}\n\n(La suite de ma réponse était incohérente, je l'ai coupée. Redemande-moi si besoin.)",
        cut.trim_end()
    )
}

/// Consigne quand un appel d'outil arrive coupé ou dans le texte.
const REPAIR_NOTE: &str = "Ton dernier appel d'outil est arrivé coupé ou sous forme de texte (réponse \
trop longue). Refais-le avec un vrai appel d'outil. Pour un gros fichier, écris-le en plusieurs fois : \
write_file pour le début, puis write_file avec append=true pour chaque suite.";

/// Étape à partir de laquelle on rappelle d'agir, si rien n'a été fait.
const EXPLORATION_NUDGE_AT: u32 = 6;
/// Lectures minimales avant ce rappel.
const EXPLORATION_READS: usize = 6;
const EXPLORATION_NUDGE: &str = "Point d'étape (message automatique) : tu as déjà beaucoup lu sans rien \
faire. Si la demande est d'écrire, de modifier ou d'exécuter, fais-le maintenant avec ce que tu sais \
(tu pourras corriger ensuite) ; s'il ne s'agit que de répondre, conclus.";

/// Second rappel, plus ferme, si le premier n'a rien changé.
const EXPLORATION_NUDGE_AGAIN_AT: u32 = 10;
const EXPLORATION_NUDGE_AGAIN: &str = "Second rappel (message automatique) : n'ouvre plus aucun fichier. \
Écris ou exécute maintenant ce qui est demandé, avec ce que tu as déjà lu ; sinon réponds.";

/// Phrase de progression : l'annonce du modèle (« Je lis config.py… ») si
/// elle existe, sinon une phrase tirée du premier outil appelé.
fn progress_phrase(content: &str, calls: &[crate::core::types::ToolCall]) -> Option<String> {
    // Fin de phrase = ponctuation suivie d'un espace ou d'un retour à la ligne :
    // un point seul est celui d'un nom de fichier (« scoring.py »).
    let text = crate::core::inline_tools::strip_tool_markup(content);
    let end = [". ", "! ", "? ", "\n"].iter().filter_map(|m| text.find(m)).min().unwrap_or(text.len());
    let sentence = text[..end].trim().trim_end_matches(['.', '!', '?', '…']);
    if !sentence.is_empty() {
        let short: String = sentence.chars().take(140).collect();
        return Some(format!("{short}."));
    }
    let call = calls.first()?;
    let arg = |key: &str| call.arguments.get(key).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let base = |path: String| {
        std::path::Path::new(&path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or(path)
    };
    let text = match call.name.as_str() {
        "read_file" => format!("Je lis {}.", base(arg("path"))),
        "write_file" => format!("J'écris {}.", base(arg("path"))),
        "list_directory" => format!("Je regarde le dossier {}.", base(arg("path"))),
        "search_files" => {
            let what = if arg("query").is_empty() { arg("pattern") } else { arg("query") };
            format!("Je cherche {what} dans le projet.")
        }
        "run_command" => "Je lance une commande.".to_string(),
        "mcp_call" => format!("J'utilise {}.", arg("tool")),
        _ => return None,
    };
    Some(text)
}

/// Outils qui ne font que lire : ils ne comptent pas comme une action.
fn is_read_tool(name: &str) -> bool {
    matches!(
        name,
        "read_file" | "search_files" | "list_directory" | "vault_read" | "vault_search" | "search_memory"
            | "list_skills" | "read_skill" | "mcp_list_tools" | "http_request"
    )
}

/// Durée maximale d'un appel au modèle pendant une conversation vocale.
// 45 puis 90 s coupaient la génération d'un fichier entier : MiMo raisonne
// longuement avant d'écrire (mesuré : 3 636 jetons de sortie pour 0 caractère
// visible, puis l'appel d'écriture au-delà de 90 s). 150 s laissent finir ;
// pendant ce temps, la boucle vocale donne signe de vie.
const VOICE_CALL_TIMEOUT: Duration = Duration::from_secs(150);
/// En dessous, un échec passager est rejoué une fois.
const FAST_FAILURE: Duration = Duration::from_secs(20);

/// Un appel au modèle, borné par `budget` s'il est donné.
async fn call_model(
    deps: &AgentDeps,
    model: &str,
    messages: &[Message],
    specs: &[crate::core::types::ToolSpec],
    settings: &crate::config::Settings,
    budget: Option<Duration>,
) -> Result<crate::providers::llm::LlmReply> {
    let call = deps.llm.chat(model, messages, specs, settings.llm.temperature, settings.llm.max_tokens);
    match budget {
        Some(limit) => tokio::time::timeout(limit, call).await.unwrap_or(Err(Error::Timeout)),
        None => call.await,
    }
}

/// Le même appel, **en flux** : chaque fragment du contenu visible part sur le
/// canal d'événements (`AgentEvent::Delta`) — le chat affiche la réponse
/// pendant qu'elle s'écrit. Résultat identique à [`call_model`] : la boucle ne
/// change pas. `try_send` : si le canal est plein (interface lente), le
/// fragment est perdu, le suivant rattrapera — jamais de blocage de la
/// génération. Utilisé pour le premier essai seulement : les chemins de
/// secours (réessai passager, régénération anti-dérive, conclusion) rejouent
/// `chat` sans flux, sinon ce qui est déjà affiché serait écrit deux fois.
async fn call_model_streaming(
    deps: &AgentDeps,
    model: &str,
    messages: &[Message],
    specs: &[crate::core::types::ToolSpec],
    settings: &crate::config::Settings,
    budget: Option<Duration>,
    events: &tokio::sync::mpsc::Sender<crate::core::types::AgentEvent>,
) -> Result<crate::providers::llm::LlmReply> {
    let delta_tx = events.clone();
    let call = deps.llm.chat_stream(
        model,
        messages,
        specs,
        settings.llm.temperature,
        settings.llm.max_tokens,
        move |text| {
            let _ = delta_tx.try_send(crate::core::types::AgentEvent::Delta { text: text.to_string() });
        },
    );
    match budget {
        Some(limit) => tokio::time::timeout(limit, call).await.unwrap_or(Err(Error::Timeout)),
        None => call.await,
    }
}

/// Erreur du fournisseur qui a des chances de passer en réessayant : panne
/// serveur (5xx), limite de débit (429) ou coupure réseau. Une 400 (requête
/// refusée) ou une 401 (clé) ne passera pas mieux la seconde fois.
fn is_transient(error: &Error) -> bool {
    match error {
        Error::Provider { message, .. } => {
            ["HTTP 500", "HTTP 502", "HTTP 503", "HTTP 504", "HTTP 429"]
                .iter()
                .any(|code| message.contains(code))
                || message.contains("error sending request")
                || message.contains("connection")
        }
        Error::Http(_) => true,
        _ => false,
    }
}

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

#[cfg(test)]
mod degenerate_tests {
    use super::*;

    #[test]
    fn la_progression_se_dit_simplement() {
        use crate::core::types::ToolCall;
        let call = |name: &str, args: serde_json::Value| ToolCall { id: "c".into(), name: name.into(), arguments: args };
        // L'annonce du modèle a priorité, coupée à la première phrase.
        assert_eq!(
            progress_phrase("Je lis scoring.py pour caler le seuil. Puis storage.", &[]).as_deref(),
            Some("Je lis scoring.py pour caler le seuil.")
        );
        // Sinon, tirée de l'outil.
        let read = call("read_file", serde_json::json!({"path": "C:\\p\\src\\config.py"}));
        assert_eq!(progress_phrase("", &[read]).as_deref(), Some("Je lis config.py."));
        let write = call("write_file", serde_json::json!({"path": "src/notify.py", "content": "x"}));
        assert_eq!(progress_phrase("", &[write]).as_deref(), Some("J'écris notify.py."));
        assert!(progress_phrase("", &[call("list_skills", serde_json::json!({}))]).is_none());
        // Jamais de balise d'appel lue à voix haute.
        assert!(progress_phrase("<tool_call>…", &[]).is_none());
    }

    #[test]
    fn les_lectures_ne_comptent_pas_comme_des_actions() {
        assert!(is_read_tool("read_file") && is_read_tool("search_files") && is_read_tool("mcp_list_tools"));
        assert!(!is_read_tool("write_file") && !is_read_tool("run_command") && !is_read_tool("mcp_call"));
    }

    /// Cas réel du 4 octobre : « HTTP 500 Internal Server Error — Unknown Error ».
    #[test]
    fn les_pannes_passageres_sont_reconnues() {
        assert!(is_transient(&Error::provider("OpenCode Go", "HTTP 500 Internal Server Error — Unknown Error")));
        assert!(is_transient(&Error::provider("OpenCode Go", "HTTP 429 Too Many Requests — slow down")));
        assert!(is_transient(&Error::provider("OpenCode Go", "error sending request for url (https://opencode.ai/…)")));
        // Ce qui ne passera pas mieux en réessayant.
        assert!(!is_transient(&Error::provider("OpenCode Go", "HTTP 400 Bad Request — invalid_request_error")));
        assert!(!is_transient(&Error::provider("OpenCode Go", "HTTP 401 Unauthorized — bad key")));
        assert!(!is_transient(&Error::Timeout));
    }

    /// Extrait réel de la réponse du 4 octobre, 13:28:39.
    const REELLE: &str = "Autrement dit, antigravity c'est un IDE. La logique est simple. Un sous-agent utilisable en non interactifRightarrow se résout par trois conditions. Un exécutable avec un mode headless pour接受 un prompt. Un prompt 入力 ruisselant.";

    #[test]
    fn la_reponse_qui_deraille_est_detectee() {
        assert!(is_degenerate(REELLE, "tu peux utiliser antigravity comme sous agent"));
        // Le premier cas, plus discret : deux caractères seulement.
        assert!(is_degenerate("Si tu veux le评估 complet du skill-creator, c'est jouable.", "réponds juste"));
    }

    #[test]
    fn une_reponse_normale_n_est_pas_touchee() {
        assert!(!is_degenerate("Le skill est en place. Où il est ? Dans agent\\skills, avec « guillemets » et accents : éàù œ.", "installe"));
        // Une demande en japonais autorise une réponse en japonais.
        assert!(!is_degenerate("こんにちは", "Comment dit-on bonjour : こんにちは ?"));
    }

    #[test]
    fn la_reponse_coupee_garde_la_partie_saine() {
        let cut = cut_degenerate(REELLE);
        assert!(cut.starts_with("Autrement dit, antigravity c'est un IDE."), "{cut}");
        assert_eq!(foreign_script_chars(&cut), 0, "{cut}");
        assert!(cut.contains("incohérente"), "{cut}");
        assert!(!cut.contains("headless pour"), "coupé avant la phrase fautive : {cut}");
    }
}
