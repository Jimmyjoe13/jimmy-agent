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
    /// Tâches en cours : la tâche de fond est signalée dans le prompt, pour
    /// que Jimmy réponde en parallèle en sachant qu'elle tourne.
    pub tasks: Arc<crate::tasks::Tasks>,
    /// La tâche est passée en arrière-plan (`App::start_task`) : son budget
    /// d'étapes et de temps s'élargit. `None` hors tâche suivie (tests).
    pub background: Option<Arc<std::sync::atomic::AtomicBool>>,
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
    /// Mêmes dépendances, reliées à l'état « en fond » d'une tâche.
    pub fn in_task(self: Arc<Self>, background: Arc<std::sync::atomic::AtomicBool>) -> Arc<Self> {
        let mut deps = (*self).clone();
        deps.background = Some(background);
        Arc::new(deps)
    }

    fn in_background(&self) -> bool {
        self.background.as_ref().is_some_and(|flag| flag.load(std::sync::atomic::Ordering::SeqCst))
    }

    pub fn tool_context(&self, settings: &Settings) -> ToolContext {
        ToolContext::with_empty_images(
            std::path::PathBuf::from(&settings.workspace),
            Arc::new(std::sync::RwLock::new(
                crate::permissions::Permissions::default(),
            )),
            self.memory.clone(),
            self.skills.clone(),
            self.vault.clone(),
        )
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
    events: tokio::sync::mpsc::Sender<AgentEvent>,
) -> Result<AgentAnswer> {
    run_with_images(deps, settings, session_id, request, Vec::new(), tool_context, events).await
}

/// Comme [`run`], avec des images jointes à la demande (capture de la fenêtre
/// active, déclenchée par l'utilisateur seul). Les images partent au modèle
/// avec la demande ; l'historique n'en garde qu'une note (`screen_note`).
pub async fn run_with_images(
    deps: Arc<AgentDeps>,
    settings: Settings,
    session_id: String,
    request: String,
    images: Vec<crate::core::types::Image>,
    tool_context: ToolContext,
    mut events: tokio::sync::mpsc::Sender<AgentEvent>,
) -> Result<AgentAnswer> {
    let started = Instant::now();
    // Temps passé en coupure réseau (veille du PC…), hors budget de la tâche.
    let mut paused = Duration::ZERO;
    // Message de l'utilisateur tel qu'envoyé au modèle (images comprises), et
    // tel qu'historisé (texte + note, jamais l'image).
    let user_message = Message::user_with_images(request.clone(), images.clone());
    let stored_request = match screen_note(&images) {
        Some(note) => format!("{request}\n\n{note}"),
        None => request.clone(),
    };

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
    // Lot 3 : une demande courte est rappelée avec le sujet en cours (message
    // précédent, résumé de la conversation, tâche de fond).
    let recap = deps.history.recap(&session_id).unwrap_or_default();
    let previous = deps
        .history
        .conversation(&session_id, 2)
        .ok()
        .and_then(|list| list.into_iter().rev().find(|m| m.role == crate::core::types::Role::User))
        .map(|m| m.content);
    let recall_query =
        prompt::recall_query(&request, previous.as_deref(), &recap.summary, deps.tasks.background_title().as_deref());
    if recall_query != request {
        log::info!("[contexte] rappel enrichi : « {} »", recall_query.replace('\n', " / ").chars().take(160).collect::<String>());
    }
    let memory_block = prompt::recall_for(&deps.memory, &settings, &recall_query).await;
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

    // Bloc « Maintenant » (lot 5) : date et canal, cartes d'accord en attente,
    // navigateur, fenêtre regardée (si le réglage l'autorise).
    let situation: Vec<String> = [
        Some(crate::core::situation::now_line(deps.voice)),
        crate::core::situation::approvals_line(&deps.approvals.pending_scopes()),
        crate::core::situation::browser_line(&deps.history),
        crate::core::situation::window_line(settings.memory.share_window_title).await,
    ]
    .into_iter()
    .flatten()
    .collect();
    let system_parts = prompt::system_parts(&PromptContext {
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
        // Carnet de bord (lots 2 et 4) : le début de la conversation sorti de
        // la fenêtre, et les chantiers suivis d'une conversation à l'autre.
        earlier: crate::core::recap::summary_block(&recap),
        chantiers: deps
            .history
            .chantiers(crate::core::recap::CHANTIERS_SHOWN)
            .ok()
            .and_then(|list| crate::core::recap::chantiers_block(&list, &session_id)),
        // Carnet de travail (14 octobre) : les acquis vérifiés des tours
        // outillés précédents, pour ne pas tout réinspecter à chaque « ok go ».
        work_notes: crate::core::notes::work_notes_block(deps.history.work_notes(&session_id).ok().flatten().as_deref()),
        // Amendements (lot « croissance ») : des instructions additionnelles
        // actées avec l'utilisateur. Absente ou vide : prompt inchangé.
        amendments: crate::growth::read_amendments(&deps.data_dir),
        background_task: deps.tasks.prompt_block(&session_id),
        situation,
        voice: deps.voice,
    });
    let mut messages = vec![Message::system(prompt::join_parts(&system_parts))];

    // 2. Historique récent de la session.
    messages.extend(deps.history.conversation(&session_id, crate::core::recap::WINDOW)?);
    messages.push(user_message.clone());
    // Relevé du contexte (lot 0) : quel bloc coûte quoi, à chaque demande.
    log::info!(
        "[contexte] système {} ; historique {} message(s), {} caractères ; demande {} caractères",
        prompt::sizes(&system_parts),
        messages.len() - 2,
        messages[1..messages.len() - 1].iter().map(|m| m.content.chars().count()).sum::<usize>(),
        request.chars().count()
    );

    deps.history
        .auto_title(&session_id, &request)
        .catch();
    deps.history
        .append(&session_id, &Message::user(stored_request.clone()))
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
    let foreground_iterations = if deps.voice {
        settings.llm.max_iterations.clamp(1, VOICE_MAX_ITERATIONS)
    } else {
        settings.llm.max_iterations.max(1)
    };
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
    // Point d'étape : étapes de lecture depuis la dernière action (voir
    // `ExplorationWatch`).
    let mut watch = ExplorationWatch::default();
    // Points de budget déjà envoyés (voir `BudgetWatch`).
    let mut budget_watch = BudgetWatch::default();
    // Cibles lues depuis la dernière action (détecteur de relecture en
    // boucle, 13 octobre) et erreurs d'outil consécutives (hors refus).
    let mut read_targets: Vec<ReadTarget> = Vec::new();
    let mut consec_errors = 0u32;
    // Appels d'outil arrivés coupés ou en texte : redemandés au plus 2 fois.
    let mut repairs = 0u32;
    let mut final_text = String::new();

    for iteration in 0u32.. {
        // Budget relu à chaque étape : une tâche passée en fond en cours de
        // route gagne aussitôt le sien (`step_budget`).
        let (max_iterations, time_budget) = step_budget(foreground_iterations, deps.in_background());
        if iteration >= max_iterations {
            break;
        }
        if over_budget(started.elapsed(), paused, time_budget) {
            emit(
                &mut events,
                AgentEvent::Notice {
                    message: "limite de temps atteinte pour cette demande".into(),
                },
            )
            .await;
            break;
        }

        // Lot 1 (contexte) : les vieux résultats d'outils sont compactés avant
        // l'appel si le contexte dépasse son budget. Sans ça, 80 étapes de
        // sorties à 12 000 caractères débordaient la fenêtre du modèle.
        compact_before_call(&mut messages);

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
        // Coupure réseau en pleine tâche, même lente (14 octobre : capot
        // fermé, veille, réseau coupé ; au réveil la tâche mourait avec tout
        // son travail). Le contexte est intact : on attend que le réseau
        // revienne et on rejoue l'appel, sans flux. Le temps perdu est mis de
        // côté (`paused`) pour ne pas consommer le budget de la tâche.
        let mut waits = OUTAGE_RETRY_DELAYS.iter();
        let mut outage = false;
        while let Err(error) = &first_try {
            if !resumes_after_outage(error, iteration, deps.voice, deps.in_background()) {
                break;
            }
            let Some(wait) = waits.next() else { break };
            log::warn!(
                "[agent] coupure pendant l'appel au modèle {model} après {} ms ({error}) : reprise dans {} s",
                call_started.elapsed().as_millis(),
                wait.as_secs()
            );
            outage = true;
            tokio::time::sleep(*wait).await;
            first_try = call_model(&deps, &model, &messages, &specs, &settings, budget).await;
        }
        if outage {
            // Appel perdu, attentes et reprise : rien de tout ça n'est du travail.
            paused += call_started.elapsed();
            if first_try.is_ok() {
                log::info!("[agent] reprise après coupure réussie");
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
                messages = vec![system, user_message.clone()];
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
            "[agent] appel au modèle n°{} en {} ms ({} outil(s), {} caractères ; contexte envoyé {} caractères ; jetons : {} en entrée, {} en sortie)",
            iteration + 1,
            call_started.elapsed().as_millis(),
            reply.tool_calls.len(),
            reply.content.chars().count(),
            crate::core::compact::context_chars(&messages),
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
                images: Vec::new(),
            })
            .catch();

        // À voix haute : dire où on en est (la boucle vocale n'en dit qu'une
        // phrase de temps en temps).
        if deps.voice {
            if let Some(text) = progress_phrase(&reply.content, &reply.tool_calls) {
                emit(&mut events, AgentEvent::Progress { text }).await;
            }
        }

        let mut step_reads = 0usize;
        let mut step_acted = false;
        for call in &reply.tool_calls {
            if is_read_call(&call.name, &call.arguments) {
                step_reads += 1;
                // Relecture en boucle (13 octobre) : on retient quoi, pas qui.
                // Une vraie action vide la liste (le contexte a changé).
                read_targets.extend(reread_targets(&call.name, &call.arguments));
            } else {
                step_acted = true;
                read_targets.clear();
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
                    // Arguments masqués (HANDOFF « Ensuite » 3) : une clé
                    // MCP ou un jeton ne doit jamais partir en clair.
                    let args: String = crate::sensitive::mask_json(&call.arguments)
                        .to_string()
                        .chars()
                        .take(200)
                        .collect();
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
                        consec_errors = 0;
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
                        // Trop d'erreurs d'affilée sans inflexion (13 octobre :
                        // 6 requêtes en 404) : le rappel suit après la boucle.
                        consec_errors += 1;
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

            // Clés connues masquées (coffre, fournisseurs, serveurs MCP) : une
            // liste de processus ou un fichier de config lu ne les livre plus.
            let text = crate::sensitive::redact_known(&text);
            messages.push(Message::tool_result(call.id.clone(), call.name.clone(), text.clone()));
            deps.history
                .append(&session_id, &Message::tool_result(call.id, call.name, text))
                .catch();
            // Webcam (Lot 2) : la photo capturée par `webcam_capture` rejoint
            // le prochain appel au modèle comme image (même pipeline que
            // `screen.rs`), puis le panier est vidé — nettoyage automatique :
            // rien ne persiste au-delà du tour, ni sur disque ni en mémoire.
            if let Ok(mut pending) = tool_context.pending_images.lock() {
                if !pending.is_empty() {
                    let images: Vec<crate::core::types::Image> = std::mem::take(&mut *pending);
                    let note = webcam_note(&images).unwrap_or_else(|| "(Photo webcam jointe.)".to_string());
                    messages.push(Message::user_with_images(note.clone(), images));
                    deps.history.append(&session_id, &Message::user(note)).catch();
                }
            }
        }

        // Cas réel du 4 octobre : 20 lectures en 11 étapes, « j'ai tout lu, rien
        // d'écrit ». Après beaucoup de lectures sans aucune action, un rappel
        // (non enregistré dans l'historique) pousse à agir ou à conclure.
        // Mesuré : un seul rappel ne suffisait pas à MiMo (écriture à l'étape
        // 15) ; un second, plus ferme, suit à l'étape 10 si rien n'a bougé.
        // 14 octobre : compté depuis la dernière action, et répété (voir
        // `ExplorationWatch`) — une action d'entrée désarmait tout.
        if let Some(text) = watch.observe(!step_acted, step_reads) {
            log::info!(
                "[agent] point d'étape n°{} : {} lectures en {} étapes sans action, rappel d'agir",
                watch.nudges,
                watch.idle_reads,
                watch.idle_steps
            );
            messages.push(Message::user(text.to_string()));
        }
        // Relecture en boucle et erreurs en chaîne (13 octobre) : rappels
        // ciblés, une fois par fenêtre (la liste/le compteur repartent après).
        if let Some(text) = reread_notice(&read_targets) {
            log::info!("[agent] relecture en boucle : {text}");
            messages.push(Message::user(text));
            read_targets.clear();
        }
        // Point de budget (14 octobre) : le modèle ne savait pas où il en
        // était ; il explorait jusqu'à la coupure (60 étapes, 20 min) sans
        // rien livrer. À mi-budget puis à 80 %, il est prévenu.
        let active = started.elapsed().saturating_sub(paused);
        if let Some(text) = budget_watch.observe(iteration + 1, max_iterations, active, time_budget) {
            log::info!("[agent] point de budget : {}", text.chars().take(90).collect::<String>());
            messages.push(Message::user(text));
        }
        if let Some(text) = consecutive_error_notice(consec_errors) {
            log::info!("[agent] {consec_errors} erreurs d'outil consécutives, rappel de méthode");
            messages.push(Message::user(text.to_string()));
            consec_errors = 0;
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
        messages.push(Message::user(CONCLUDE_NOTE.to_string()));
        compact_before_call(&mut messages);
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

    // Carnet de travail (14 octobre), après la réponse et en tâche de fond :
    // un tour qui a beaucoup exploré laisse ses acquis (résultats d'outils
    // compris) au tour suivant. Indépendant de la mémoire : c'est le fil de la
    // tâche, pas un souvenir. Le tour = de la demande à la réponse finale.
    if tools_used.len() >= crate::core::notes::NOTES_MIN_TOOLS {
        let start = messages
            .iter()
            .rposition(|m| m.role == crate::core::types::Role::User && m.content == user_message.content)
            .unwrap_or(0);
        let mut turn = messages[start..].to_vec();
        turn.push(Message::assistant(final_text.clone()));
        let llm = deps.llm.clone();
        let settings = settings.clone();
        let history = deps.history.clone();
        let session = session_id.clone();
        let model = model.clone();
        tokio::spawn(async move {
            crate::core::notes::update(&llm, &settings, &history, &session, &model, &turn).await;
        });
    }

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
        let data_dir = deps.data_dir.clone();
        let settings = settings.clone();
        let request = request.clone();
        let answer = final_text.clone();
        let tool_errors = tool_errors.clone();
        let tools_used = tools_used.clone();
        let weak = events.downgrade();
        let history = deps.history.clone();
        let session = session_id.clone();
        tokio::spawn(async move {
            // Carnet de bord d'abord : c'est lui que le prochain tour relit.
            crate::core::recap::update(&llm, &settings, &history, &session).await;
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
                // Même souvenir, côté vault Obsidian : une puce dans l'inbox
                // du jour, rangée ensuite par le tri PARA. Le vault est un
                // complément : son échec n'annule pas le souvenir en base.
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
                // Barrière 2e occurrence : un besoin vu une seule fois se
                // note et ne capture pas. Sans elle, ~15 skills par jour en
                // usage réel, sans qu'aucun besoin ne revienne.
                if !crate::skills::demand::note_request(&data_dir, &request) {
                    log::info!("[skills] besoin inédit, pas de capture");
                } else if let Some(proposal) = crate::skills::capture::capture(
                    &llm, &settings, &request, &answer, &tools_used, &skills,
                )
                .await
                {
                    // Cible d'écriture, dans l'ordre : désignation du modèle
                    // (`proche:`, vérifiée), fusion forcée sur similarité
                    // stricte (0,6 calibré), création sinon.
                    enum Target {
                        Honed(String),
                        Forced(String),
                        New,
                    }
                    let target = match &proposal.close_to {
                        Some(name) if skills.load(name).is_ok() => Target::Honed(name.clone()),
                        _ => match skills.find_similar(&proposal.name, &proposal.description) {
                            Some(known) => {
                                log::info!(
                                    "[skills] fusion forcée : « {} » rejoint « {} »",
                                    proposal.name, known.name
                                );
                                Target::Forced(known.name)
                            }
                            None => Target::New,
                        },
                    };
                    let (name, honed) = match target {
                        Target::Honed(name) | Target::Forced(name) => (name, true),
                        Target::New => (proposal.name.clone(), false),
                    };
                    let written = if honed {
                        skills.update_body(&name, &proposal.body)
                    } else {
                        skills.write(&proposal.name, &proposal.description, &proposal.body)
                    };
                    match written {
                        Ok(path) => {
                            log::info!(
                                "[skills] compétence {} : {} ({})",
                                if honed { "aiguisée" } else { "capturée" },
                                name,
                                path.display()
                            );
                            if let Some(events) = weak.upgrade() {
                                let _ = events
                                    .send(AgentEvent::Memory {
                                        action: "skill".into(),
                                        detail: name.clone(),
                                    })
                                    .await;
                            }
                        }
                        Err(error) => log::warn!("[skills] capture non écrite : {error}"),
                    }
                }
            }
            // Tri de l'inbox du vault par lots (PARA), en dernier : il peut
            // enchaîner plusieurs appels au modèle et ne doit retarder ni
            // l'apprentissage ni la capture de compétence.
            if let Some(vault) = &vault {
                crate::memory::sort::sort_if_needed(vault, &llm, settings.memory_model()).await;
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

/// Consigne de la conclusion forcée (limite d'étapes, de temps, ou modèle
/// muet). Cas réel du 14 octobre : après « démarre par… », la conclusion
/// finissait par « à corriger après ton accord » — l'utilisateur avait déjà
/// validé, il a dû redire « ok continue ».
const CONCLUDE_NOTE: &str = "Tu as atteint la limite d'étapes ou de temps pour cette demande. Réponds maintenant, \
sans appeler d'outil : d'abord ce qui est fait (fichiers écrits, changements appliqués), puis ce qu'il reste à \
faire, en étapes concrètes. Si l'utilisateur avait déjà validé la tâche, ne lui redemande pas son accord : dis \
que tu reprends la suite à son prochain « continue ». Sois bref.";

/// Points de budget envoyés au modèle pendant une tâche outillée : à 50 %
/// puis à 80 % du budget (étapes ou temps, le plus avancé des deux), une fois
/// chacun.
#[derive(Default)]
struct BudgetWatch {
    half: bool,
    late: bool,
}

impl BudgetWatch {
    /// Après l'étape `step` sur `max_steps`, `elapsed` (hors coupures) sur
    /// `time_budget` : le point à envoyer, s'il y en a un.
    fn observe(&mut self, step: u32, max_steps: u32, elapsed: Duration, time_budget: Duration) -> Option<String> {
        let by_steps = step as f64 / max_steps.max(1) as f64;
        let by_time = elapsed.as_secs_f64() / time_budget.as_secs_f64().max(1.0);
        let used = by_steps.max(by_time);
        let steps_left = max_steps.saturating_sub(step);
        let minutes_left = time_budget.saturating_sub(elapsed).as_secs().div_ceil(60);
        if used >= 0.8 && !self.late {
            self.late = true;
            self.half = true;
            return Some(format!(
                "Fin de budget proche (message automatique) : il te reste {steps_left} étape(s) et environ \
                 {minutes_left} min. Plus d'exploration : écris ou applique maintenant ce qui est demandé, avec ce \
                 que tu sais ; puis conclus en disant ce qui est fait et ce qui reste."
            ));
        }
        if used >= 0.5 && !self.half {
            self.half = true;
            return Some(format!(
                "Point de budget (message automatique) : étape {step} sur {max_steps}, environ {minutes_left} min \
                 restantes. Si la demande est de créer ou de modifier quelque chose et que rien n'est encore écrit, \
                 écris maintenant un premier jet, puis complète-le."
            ));
        }
        None
    }
}

/// Une lecture vue par le détecteur de relecture en boucle : `key` identifie
/// ce qui a été lu, `label` le dit au modèle (« lu « campaigns.py » »).
#[derive(Debug, Clone, PartialEq)]
struct ReadTarget {
    key: String,
    label: String,
}

/// Cibles d'une lecture pour le détecteur de relecture en boucle :
/// `read_file` et `list_directory` avec leur chemin (`start_line` inclus :
/// paginer un gros fichier n'est pas relire), `mcp_list_tools` par serveur,
/// un `mcp_call` de consultation par outil et arguments, et chaque commande
/// d'une ligne lancée (`run_command`, `vps_exec`) — 14 octobre : la même
/// inspection revenait habillée d'`echo` et de `| head` différents.
fn reread_targets(name: &str, arguments: &serde_json::Value) -> Vec<ReadTarget> {
    let text = |key: &str| arguments.get(key).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let target = |key: String, label: String| vec![ReadTarget { key, label }];
    match name {
        "read_file" => {
            let start = arguments.get("start_line").and_then(|v| v.as_u64()).unwrap_or(0);
            let path = text("path");
            target(format!("read_file|{path}|{start}"), format!("lu « {path} »"))
        }
        "list_directory" => target(format!("list_directory|{}", text("path")), format!("listé « {} »", text("path"))),
        "mcp_list_tools" => target(
            format!("mcp_list_tools|{}|{}", text("server"), text("filter")),
            format!("listé les outils de « {} »", text("server")),
        ),
        "run_command" => command_targets(&text("command")),
        "mcp_call" => {
            let inner = match arguments.get("arguments") {
                Some(serde_json::Value::String(raw)) => serde_json::from_str(raw).unwrap_or(serde_json::Value::Null),
                Some(other) => other.clone(),
                None => serde_json::Value::Null,
            };
            match inner.get("command").and_then(|v| v.as_str()) {
                Some(command) => command_targets(command),
                None => target(
                    format!("mcp_call|{}|{}|{inner}", text("server"), text("tool")),
                    format!("appelé « {} » avec les mêmes arguments", text("tool")),
                ),
            }
        }
        _ => Vec::new(),
    }
}

/// Les commandes d'une ligne, normalisées : découpe sur `;`, `&&`, `||` et
/// les retours à la ligne, partie avant le premier tube, redirections et
/// `echo` de séparation retirés. Une commande n'est comptée qu'une fois par
/// ligne.
fn command_targets(command: &str) -> Vec<ReadTarget> {
    let mut out: Vec<ReadTarget> = Vec::new();
    for piece in command.split(['\n', ';']).flat_map(|p| p.split("&&")).flat_map(|p| p.split("||")) {
        let head = piece.split('|').next().unwrap_or("");
        let words: Vec<&str> = head
            .split_whitespace()
            .filter(|w| !(w.starts_with("2>") || w.starts_with('>') || w.starts_with("1>") || w.starts_with("&>")))
            .collect();
        let Some(first) = words.first() else { continue };
        if ["echo", "write-host", "write-output", "printf", "cd"].contains(&first.to_lowercase().as_str()) {
            continue;
        }
        let normalized = words.join(" ");
        let key = format!("cmd|{normalized}");
        if !out.iter().any(|t| t.key == key) {
            let short: String = normalized.chars().take(80).collect();
            out.push(ReadTarget { key, label: format!("lancé « {short} »") });
        }
    }
    out
}

/// Seuil de relecture en boucle : à la 3e lecture identique sans action
/// entre-temps, un rappel ciblé (cas réel du 13 octobre).
const REREAD_THRESHOLD: usize = 3;

/// Rappel de relecture en boucle : `Some(texte)` quand une même cible a été
/// lue `REREAD_THRESHOLD` fois sans action entre-temps (l'appelant vide la
/// liste à chaque action et après chaque rappel).
fn reread_notice(targets: &[ReadTarget]) -> Option<String> {
    use std::collections::HashMap;
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for target in targets {
        let n = counts.entry(target.key.as_str()).or_insert(0);
        *n += 1;
        if *n >= REREAD_THRESHOLD {
            return Some(format!(
                "Rappel automatique : tu as déjà {} {n} fois sans rien en faire. \
                 Ne le refais plus : le résultat est plus haut, agis avec ce que tu sais, ou conclus.",
                target.label
            ));
        }
    }
    None
}

/// Rappel « agis ou conclus », compté **depuis la dernière action** : une
/// étape qui agit remet tout à zéro. Premier rappel après
/// `EXPLORATION_NUDGE_AT` étapes de lecture (et au moins `EXPLORATION_READS`
/// lectures), second à `EXPLORATION_NUDGE_AGAIN_AT`, puis le second revient
/// toutes les `EXPLORATION_NUDGE_REPEAT` étapes. Avant le 14 octobre, le
/// compte était global : une seule action en début de tâche (ou un `mcp_call`,
/// toujours pris pour une action) désarmait le rappel pour toute la tâche.
#[derive(Default)]
struct ExplorationWatch {
    idle_steps: u32,
    idle_reads: usize,
    nudges: u32,
}

impl ExplorationWatch {
    /// Une étape vient de finir : `only_reads` si elle n'a fait que lire,
    /// `reads` le nombre de lectures. Renvoie le rappel à envoyer, s'il y en a un.
    fn observe(&mut self, only_reads: bool, reads: usize) -> Option<&'static str> {
        if !only_reads {
            *self = Self::default();
            return None;
        }
        self.idle_steps += 1;
        self.idle_reads += reads;
        let due = match self.nudges {
            0 => self.idle_steps >= EXPLORATION_NUDGE_AT && self.idle_reads >= EXPLORATION_READS,
            n => self.idle_steps >= EXPLORATION_NUDGE_AGAIN_AT + (n - 1) * EXPLORATION_NUDGE_REPEAT,
        };
        if !due {
            return None;
        }
        self.nudges += 1;
        Some(if self.nudges == 1 { EXPLORATION_NUDGE } else { EXPLORATION_NUDGE_AGAIN })
    }
}

/// Seuil d'erreurs d'outil consécutives avant de changer de méthode (cas réel
/// du 13 octobre : 6 requêtes en 404 d'affilée sans inflexion).
const CONSEC_ERRORS_THRESHOLD: u32 = 4;

/// Rappel après trop d'erreurs d'outil consécutives (les refus d'accord ne
/// comptent pas : ils ne passent jamais par ici).
fn consecutive_error_notice(consec: u32) -> Option<&'static str> {
    if consec >= CONSEC_ERRORS_THRESHOLD {
        Some(
            "Rappel automatique : 4 outils d'affilée en erreur sans changer de méthode. \
             Relis le message d'erreur au lieu de réessayer pareil : vérifie les arguments \
             attendus (schéma de l'outil, `mcp_list_tools`), ou conclus avec ce qui marche.",
        )
    } else {
        None
    }
}

/// Étape à partir de laquelle on rappelle d'agir, si rien n'a été fait.
const EXPLORATION_NUDGE_AT: u32 = 6;
/// Lectures minimales avant ce rappel.
const EXPLORATION_READS: usize = 6;
const EXPLORATION_NUDGE: &str = "Point d'étape (message automatique) : tu as déjà beaucoup lu sans rien \
faire. Si la demande est d'écrire, de modifier ou d'exécuter, fais-le maintenant avec ce que tu sais \
(tu pourras corriger ensuite) ; s'il ne s'agit que de répondre, conclus.";

/// Second rappel, plus ferme, si le premier n'a rien changé.
const EXPLORATION_NUDGE_AGAIN_AT: u32 = 10;
/// Ensuite, le second rappel revient toutes les N étapes sans action.
const EXPLORATION_NUDGE_REPEAT: u32 = 8;
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
/// Note gardée dans l'historique à la place des images : le modèle sait au
/// tour suivant qu'une capture a été vue, sans que l'image soit stockée.
fn screen_note(images: &[crate::core::types::Image]) -> Option<String> {
    if images.is_empty() {
        return None;
    }
    let labels: Vec<String> = images.iter().map(|image| format!("« {} »", image.label)).collect();
    Some(format!("(Capture d'écran jointe : {}.)", labels.join(", ")))
}

/// Note gardée dans l'historique à la place d'une photo webcam : l'image ne
/// vit qu'en mémoire le temps du tour (nettoyage automatique), seule cette
/// note persiste — pas de stockage qui grossit, pas de jetons brûlés ensuite.
fn webcam_note(images: &[crate::core::types::Image]) -> Option<String> {
    if images.is_empty() {
        return None;
    }
    let labels: Vec<String> = images.iter().map(|image| format!("« {} »", image.label)).collect();
    Some(format!("(Photo webcam jointe : {}.)", labels.join(", ")))
}

/// Budget d'une demande : étapes et durée totale. Au premier plan, celui des
/// réglages (25 en vocal) et 10 min — même durée qu'à l'écrit : MiMo met 3 à
/// 4 minutes pour une tâche de code, 180 s coupaient avant l'écriture. En
/// fond, au moins `tasks::BACKGROUND_MAX_ITERATIONS` étapes et 20 min.
/// Compacte le passé si le contexte dépasse son budget, et le note au journal.
fn compact_before_call(messages: &mut [Message]) {
    let before = crate::core::compact::context_chars(messages);
    let reduced = crate::core::compact::fit(messages, crate::core::compact::BUDGET_CHARS, crate::core::compact::KEEP_ROUNDS);
    if reduced > 0 {
        log::info!(
            "[contexte] compaction : {reduced} élément(s) réduit(s), {before} → {} caractères",
            crate::core::compact::context_chars(messages)
        );
    }
}

fn step_budget(foreground_iterations: u32, background: bool) -> (u32, Duration) {
    if background {
        (
            foreground_iterations.max(crate::tasks::BACKGROUND_MAX_ITERATIONS),
            crate::tasks::BACKGROUND_DEADLINE,
        )
    } else {
        (foreground_iterations, Duration::from_secs(600))
    }
}

/// Un appel qui ne fait que regarder : outil de lecture, ou commande
/// d'inspection (`Get-ChildItem`, `Test-Path`, `ssh … docker ps`). Sert au
/// rappel « agis ou conclus » : avant, toute commande comptait comme une
/// action et le rappel ne partait jamais (cas réel du 7 octobre : 25 étapes
/// d'inspection SSH, rien de fait, deux demandes de suite).
fn is_read_call(name: &str, arguments: &serde_json::Value) -> bool {
    is_read_tool(name)
        // 14 octobre : un `mcp_call` de consultation (stats, `vps_exec cat`)
        // comptait comme une action et désarmait le rappel.
        || (name == "mcp_call" && crate::sensitive::mcp_call_is_read(arguments))
        || (name == "run_command"
            && arguments
                .get("command")
                .and_then(|c| c.as_str())
                .is_some_and(crate::sensitive::command_is_read_only))
}

fn is_read_tool(name: &str) -> bool {
    matches!(
        name,
        "read_file" | "search_files" | "list_directory" | "vault_read" | "vault_search" | "search_memory"
            | "list_skills" | "read_skill" | "mcp_list_tools" | "http_request"
            | "system_info" | "process_list" | "window_list" | "webcam_list"
    )
}

/// Attentes avant chaque reprise après une coupure réseau en cours de tâche
/// (au réveil du PC, le réseau met parfois quelques minutes à revenir :
/// mesuré le 14 octobre, échecs jusqu'à ~4 min après le réveil).
const OUTAGE_RETRY_DELAYS: [Duration; 3] = [Duration::from_secs(15), Duration::from_secs(60), Duration::from_secs(180)];

/// Une coupure qui mérite d'attendre et de rejouer l'appel : réseau ou flux
/// interrompu, en cours de tâche (étape > 0 : il y a du travail à sauver),
/// sauf en vocal au premier plan (l'utilisateur attend la réponse : la
/// conclusion actuelle est préférable à trois minutes de silence).
fn resumes_after_outage(error: &Error, iteration: u32, voice: bool, background: bool) -> bool {
    let network = match error {
        Error::Provider { message, .. } => {
            is_transient(error) || message.contains("error decoding response body") || message.contains("timed out")
        }
        Error::Http(_) => true,
        _ => false,
    };
    network && iteration > 0 && (background || !voice)
}

/// Budget de temps dépassé, hors temps passé en coupure.
fn over_budget(elapsed: Duration, paused: Duration, budget: Duration) -> bool {
    elapsed.saturating_sub(paused) > budget
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
    fn l_historique_garde_une_note_pas_l_image() {
        let image = crate::core::types::Image { media_type: "image/jpeg".into(), base64: "QUJD".into(), label: "Visual Studio Code".into() };
        assert_eq!(screen_note(&[]), None);
        assert_eq!(screen_note(&[image]).as_deref(), Some("(Capture d'écran jointe : « Visual Studio Code ».)"));
    }

    #[test]
    fn une_tache_de_fond_a_un_budget_elargi() {
        assert_eq!(step_budget(25, false), (25, Duration::from_secs(600)));
        assert_eq!(step_budget(25, true), (60, Duration::from_secs(1200)));
        // Un réglage plus généreux que le fond n'est jamais réduit.
        assert_eq!(step_budget(80, true).0, 80);
    }

    #[test]
    fn une_commande_d_inspection_compte_comme_une_lecture() {
        let run = |command: &str| serde_json::json!({ "command": command });
        assert!(is_read_call("run_command", &run("Get-ChildItem $env:USERPROFILE | Format-Table Name")));
        assert!(!is_read_call("run_command", &run("Remove-Item build -Recurse")));
        assert!(is_read_call("read_file", &serde_json::json!({ "path": "a.txt" })));
        assert!(!is_read_call("write_file", &serde_json::json!({ "path": "a.txt" })));
    }

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

    /// Cas réel du 14 octobre : `vps_exec` de `cat`/`grep` et
    /// `get_campaign_stats` comptaient comme des actions.
    #[test]
    fn un_appel_mcp_de_consultation_compte_comme_une_lecture() {
        let mcp = |tool: &str, args: serde_json::Value| serde_json::json!({"server": "vps", "tool": tool, "arguments": args});
        assert!(is_read_call("mcp_call", &mcp("vps_exec", serde_json::json!({"command": "cat ~/app/docker-compose.yml | head -80"}))));
        assert!(is_read_call("mcp_call", &mcp("get_campaign_stats", serde_json::json!({"campaign_id": 22}))));
        assert!(!is_read_call("mcp_call", &mcp("vps_exec", serde_json::json!({"command": "docker compose up -d"}))));
        assert!(!is_read_call("mcp_call", &mcp("vps_upload", serde_json::json!({}))));
    }

    /// Cas réel du 14 octobre : 43 étapes d'inspection, aucun rappel. Le
    /// rappel compte les étapes **depuis la dernière action** (avant : une
    /// seule action en début de tâche le désarmait pour toujours) et revient
    /// toutes les 8 étapes tant que rien ne bouge.
    #[test]
    fn le_rappel_d_agir_se_rearme_et_revient() {
        let mut watch = ExplorationWatch::default();
        // Une action d'entrée (cas réel : `python --version; tailscale …`).
        assert_eq!(watch.observe(false, 1), None);
        let mut rappels = Vec::new();
        for step in 1..=43 {
            if watch.observe(true, 1).is_some() {
                rappels.push(step);
            }
        }
        assert_eq!(rappels, vec![6, 10, 18, 26, 34, 42]);
        // Une vraie action remet à zéro ; le premier rappel revient 6 étapes plus tard.
        assert_eq!(watch.observe(false, 0), None);
        let reprise: Vec<u32> = (1..=6).filter(|_| watch.observe(true, 2).is_some()).collect();
        assert_eq!(reprise, vec![6]);
        // Moins de 6 lectures en 6 étapes : pas encore.
        let mut calme = ExplorationWatch::default();
        assert!((1..=5).all(|_| calme.observe(true, 1).is_none()));
    }

    /// Cas réels du 14 octobre : `mcp_list_tools vps` 4 fois, `tailscale serve
    /// --help` 4 fois (avec des `| head` et des `echo` différents autour), et
    /// un chemin Windows qui donnait « C » dans le rappel.
    #[test]
    fn les_relectures_mcp_et_commandes_declenchent_un_rappel() {
        let cibles = |calls: &[(&str, serde_json::Value)]| -> Vec<ReadTarget> {
            calls.iter().flat_map(|(name, args)| reread_targets(name, args)).collect()
        };
        let outils = serde_json::json!({"server": "vps"});
        let rappel = reread_notice(&cibles(&[("mcp_list_tools", outils.clone()), ("mcp_list_tools", outils.clone()), ("mcp_list_tools", outils)]))
            .expect("3e liste identique : rappel");
        assert!(rappel.contains("vps"), "{rappel}");

        let exec = |command: &str| serde_json::json!({"server": "vps", "tool": "vps_exec", "arguments": {"command": command}});
        let boucle = cibles(&[
            ("mcp_call", exec("tailscale serve --help 2>&1; echo ===FUNNEL===; tailscale funnel --help 2>&1 | head -40")),
            ("mcp_call", exec("tailscale serve --help 2>&1 | head -100; echo ===STATUS===; tailscale status | head -20")),
            ("mcp_call", exec("tailscale serve --help 2>&1; echo ===VERSION===; tailscale version")),
        ]);
        let rappel = reread_notice(&boucle).expect("3e lancement identique : rappel");
        assert!(rappel.contains("tailscale serve --help"), "{rappel}");
        // Des requêtes différentes ne sont pas des relectures.
        let stats = |id: u32| serde_json::json!({"server": "emile", "tool": "get_campaign_stats", "arguments": {"campaign_id": id}});
        assert_eq!(reread_notice(&cibles(&[("mcp_call", stats(21)), ("mcp_call", stats(22)), ("mcp_call", stats(23))])), None);

        let lire = serde_json::json!({"path": "C:\\p\\api\\campaigns.py"});
        let rappel = reread_notice(&cibles(&[("read_file", lire.clone()), ("read_file", lire.clone()), ("read_file", lire)])).unwrap();
        assert!(rappel.contains("campaigns.py"), "{rappel}");
    }

    #[test]
    fn les_lectures_ne_comptent_pas_comme_des_actions() {
        assert!(is_read_tool("read_file") && is_read_tool("search_files") && is_read_tool("mcp_list_tools"));
        assert!(!is_read_tool("write_file") && !is_read_tool("run_command") && !is_read_tool("mcp_call"));
    }

    /// Cas réel du 13 octobre : `campaigns.py` relu 4 fois et `mcp_server.py`
    /// 5 fois en 48 appels, sans rien écrire. À la 3e lecture identique sans
    /// action entre-temps, un rappel ciblé nomme le fichier. Paginer un gros
    /// fichier (`start_line` différent) n'est pas relire.
    #[test]
    fn la_meme_lecture_trois_fois_declenche_un_rappel_cible() {
        let target = |name: &str, args: serde_json::Value| reread_targets(name, &args).remove(0);
        assert_eq!(reread_notice(&[]), None);
        let lus = vec![target("read_file", serde_json::json!({"path": "a.py"})), target("read_file", serde_json::json!({"path": "b.py"}))];
        assert_eq!(reread_notice(&lus), None);
        // Pagination : suite du même fichier, pas une relecture.
        assert_ne!(
            target("read_file", serde_json::json!({"path": "gros.py"})),
            target("read_file", serde_json::json!({"path": "gros.py", "start_line": 101}))
        );
        // `list_directory` compte aussi ; un outil d'écriture, non.
        assert!(target("list_directory", serde_json::json!({"path": "d"})).label.contains('d'));
        assert!(reread_targets("write_file", &serde_json::json!({"path": "a.py"})).is_empty());
        let boucle = vec![
            target("read_file", serde_json::json!({"path": "campaigns.py"})),
            target("read_file", serde_json::json!({"path": "mcp_server.py"})),
            target("read_file", serde_json::json!({"path": "campaigns.py"})),
            target("read_file", serde_json::json!({"path": "campaigns.py"})),
        ];
        let rappel = reread_notice(&boucle).expect("3e lecture identique : rappel");
        assert!(rappel.contains("campaigns.py"), "{rappel}");
    }

    /// Cas réel du 13 octobre : 6 requêtes `/stats` en 404 d'affilée sans
    /// inflexion. Après 4 erreurs d'outil consécutives, un rappel demande de
    /// changer de méthode ou de conclure (les refus d'accord ne comptent pas :
    /// ils ne passent jamais par ici).
    #[test]
    fn quatre_erreurs_d_affilee_demandent_de_changer_de_methode() {
        assert_eq!(consecutive_error_notice(0), None);
        assert_eq!(consecutive_error_notice(3), None);
        assert!(consecutive_error_notice(4).is_some());
        assert!(consecutive_error_notice(9).is_some());
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

    /// Cas réels du 14 octobre : capot fermé en pleine tâche de fond, veille,
    /// réseau coupé ; au réveil (21 et 27 min plus tard) l'appel échoue et la
    /// tâche mourait. Une coupure réseau en cours de tâche se reprend, même
    /// lente ; pas au premier appel (rien à sauver) ni en vocal au premier
    /// plan (l'utilisateur attend), et jamais une requête refusée.
    #[test]
    fn une_coupure_reseau_en_cours_de_tache_se_reprend() {
        let decodage = Error::provider("OpenCode Go", "error decoding response body");
        let envoi = Error::provider("OpenCode Go", "error sending request for url (https://opencode.ai/zen/go/v1/responses)");
        // Arguments : erreur, étape, vocal, en fond.
        assert!(resumes_after_outage(&decodage, 43, false, true));
        assert!(resumes_after_outage(&envoi, 19, true, true), "fond lancé à la voix");
        assert!(resumes_after_outage(&envoi, 5, false, false), "Chat au premier plan");
        assert!(!resumes_after_outage(&envoi, 0, false, true), "premier appel : rien à sauver");
        assert!(!resumes_after_outage(&envoi, 5, true, false), "vocal au premier plan");
        let refus = Error::provider("OpenCode Go", "HTTP 400 Bad Request — invalid_request_error");
        assert!(!resumes_after_outage(&refus, 5, false, true));
    }

    /// Le temps passé en coupure (veille, réseau) ne compte pas dans le budget
    /// de 20 min : sinon la tâche reprise mourait à l'étape suivante.
    #[test]
    fn le_temps_de_coupure_ne_compte_pas_dans_le_budget() {
        let budget = Duration::from_secs(1200);
        assert!(!over_budget(Duration::from_secs(2400), Duration::from_secs(1500), budget));
        assert!(over_budget(Duration::from_secs(2400), Duration::from_secs(600), budget));
    }

    /// Cas réel du 14 octobre (campagne à réparer) : 60 étapes sur 60 sans savoir où il
    /// en était. Le point part à mi-budget puis à 80 %, une fois chacun,
    /// selon les étapes ou le temps (le plus avancé).
    #[test]
    fn le_modele_est_prevenu_a_mi_budget_puis_en_fin_de_budget() {
        let min = |m: u64| Duration::from_secs(m * 60);
        let mut watch = BudgetWatch::default();
        let points: Vec<(u32, String)> = (1..=60)
            .filter_map(|step| watch.observe(step, 60, Duration::from_secs(10 * step as u64), min(20)).map(|t| (step, t)))
            .collect();
        assert_eq!(points.iter().map(|(s, _)| *s).collect::<Vec<_>>(), vec![30, 48]);
        assert!(points[0].1.contains("étape 30 sur 60") && points[0].1.contains("premier jet"), "{}", points[0].1);
        assert!(points[1].1.contains("12 étape(s)") && points[1].1.contains("Plus d'exploration"), "{}", points[1].1);
        // Le temps peut passer avant les étapes (appels lents) : 17 min sur 20
        // à l'étape 10 → directement le point de fin, sans le premier.
        let mut lent = BudgetWatch::default();
        let texte = lent.observe(10, 60, min(17), min(20)).unwrap();
        assert!(texte.starts_with("Fin de budget proche"), "{texte}");
        assert_eq!(lent.observe(11, 60, min(18), min(20)), None);
    }

    /// Cas réel du 14 octobre (campagne à réparer) : la conclusion forcée finissait par
    /// « après ton accord » alors que la tâche était validée.
    #[test]
    fn la_conclusion_forcee_ne_redemande_pas_l_accord() {
        assert!(CONCLUDE_NOTE.contains("ne lui redemande pas son accord"));
        assert!(CONCLUDE_NOTE.contains("ce qui est fait") && CONCLUDE_NOTE.contains("reste"));
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

