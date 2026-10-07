//! Cœur de Jimmy : agent, outils, mémoire, skills, fournisseurs.
//!
//! Ce crate ne dépend **pas** de Tauri. Toute la logique exécutable de Jimmy
//! vit ici, ce qui la rend testable avec `cargo test` sans démarrer
//! d'application, et réutilisable si l'interface évolue.
//!
//! [`App`] assemble les pièces et sert de point d'entrée unique à l'interface.

pub mod config;
pub mod core;
pub mod db;
pub mod error;
pub mod growth;
pub mod memory;
pub mod mcp;
pub mod paths;
pub mod permissions;
pub mod providers;
pub mod sensitive;
pub mod skills;
pub mod tasks;
pub mod tools;
pub mod voice;

use std::sync::{Arc, RwLock};

pub use error::{Error, Result};

use config::{Secrets, Settings};
use core::history::{History, Shared};
use core::types::AgentEvent;
use db::Db;
use memory::MemoryStore;
use paths::Paths;
use permissions::Permissions;
use providers::{AvatarClient, LlmClient, Stt, Tts, TtsVoice};
use skills::SkillStore;
use tools::{ToolDeps, ToolRegistry};

/// Assembleur de Jimmy. Detient l'état long.
pub struct App {
    pub paths: Paths,
    pub settings: Arc<RwLock<Settings>>,
    pub secrets: Arc<Secrets>,
    pub db: Shared<Db>,
    pub history: Arc<History>,
    pub memory: Arc<MemoryStore>,
    pub skills: Arc<SkillStore>,
    pub llm: Arc<LlmClient>,
    pub tts: Tts,
    pub avatar: AvatarClient,
    pub registry: Arc<ToolRegistry>,
    /// Demandes d'autorisation en attente (fichiers sensibles) : la boucle
    /// d'agent y attend le clic « Autoriser / Refuser » du Chat.
    pub approvals: Arc<sensitive::Approvals>,
    /// Serveurs MCP : leurs outils sont ajoutés au registre par [`App::start_mcp`].
    pub mcp: Arc<mcp::McpRegistry>,
    /// Mémoire persistante dans le vault Obsidian : `None` si le chemin est
    /// vide ou inexistant.
    pub vault: Option<Arc<memory::vault::Vault>>,
    /// Verrous asynchrones : ces deux champs sont utilisés à travers des
    /// `await`, un `std::sync::Mutex` rendrait la future non `Send`.
    pub stt: tokio::sync::Mutex<Option<Stt>>,
    /// Second serveur, modèle précis, réservé à la commande. `None` = la
    /// commande passe par `stt` (repli).
    pub stt_command: tokio::sync::Mutex<Option<Stt>>,
    /// Dernier seuil de VAD utilisé par l'écoute (diagnostic, vue Voix).
    pub last_vad_threshold: Mutex<f32>,
    /// Vrai pendant que Jimmy parle : l'écoute ignore alors le micro.
    speaking: std::sync::atomic::AtomicBool,
    pub godot: tokio::sync::Mutex<Option<tokio::process::Child>>,
    /// L'avatar doit tourner : vrai dès qu'un démarrage est demandé, faux
    /// après un arrêt explicite. Le chien de garde s'en sert pour relancer
    /// Godot quand il disparaît sans trace.
    avatar_desired: std::sync::atomic::AtomicBool,
    /// Un seul lancement d'avatar à la fois : deux lancements simultanés, et
    /// le second écrasait le premier dans `godot` — `kill_on_drop` tuait alors
    /// l'avatar VIVANT, tandis que le second mourait sur le port déjà pris.
    avatar_spawning: tokio::sync::Mutex<()>,
    /// Le chien de garde de l'avatar n'est démarré qu'une fois.
    avatar_watchdog: std::sync::atomic::AtomicBool,
    /// Session vocale courante et instant du dernier échange. Portée par
    /// `App`, et non par la boucle d'écoute : un arrêt puis une reprise de
    /// l'écoute (bouton, réglage, micro) créait auparavant une boucle neuve
    /// dont la session repartait de zéro — Jimmy « oubliait » la conversation
    /// en cours. Voir `VOICE_SESSION_IDLE` (10 min).
    pub voice_session: std::sync::Mutex<Option<(String, std::time::Instant)>>,
    /// Arrêt d'urgence (« STOP », bouton « Arrêter ») : chaque demande
    /// incrémente le compteur, les tâches en cours l'observent.
    stop_signal: tokio::sync::watch::Sender<u64>,
    /// Tâches de l'agent en cours (Chat et voix), premier plan et fond.
    running: std::sync::atomic::AtomicUsize,
    /// Registre des tâches : passage en arrière-plan, arrêt ciblé, bandeau
    /// du Chat et bloc du prompt (voir `tasks`).
    pub tasks: Arc<tasks::Tasks>,
}

/// Compte une tâche en cours le temps de son exécution.
struct RunningGuard<'a>(&'a std::sync::atomic::AtomicUsize);

impl<'a> RunningGuard<'a> {
    fn new(counter: &'a std::sync::atomic::AtomicUsize) -> Self {
        counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        RunningGuard(counter)
    }
}

impl Drop for RunningGuard<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Issue d'une tâche pour son appelant (voir [`App::start_task`]).
pub enum TaskOutcome {
    /// Finie au premier plan : l'appelant traite la réponse comme avant.
    Done(Result<core::types::AgentAnswer>),
    /// Passée en arrière-plan : l'appelant rend la main. La fin (réponse,
    /// arrêt, échec) part en `AgentEvent::Background` et est annoncée.
    Detached,
}

/// Poignée d'une tâche lancée par [`App::start_task`].
pub struct TaskTicket {
    pub id: String,
    detached: tokio::sync::oneshot::Receiver<()>,
    done: tokio::task::JoinHandle<(Result<core::types::AgentAnswer>, bool)>,
}

impl TaskTicket {
    /// Attend la fin de la tâche **ou** son passage en arrière-plan.
    pub async fn wait(self) -> TaskOutcome {
        let TaskTicket { detached, mut done, .. } = self;
        tokio::select! {
            biased;
            Ok(()) = detached => TaskOutcome::Detached,
            joined = &mut done => match joined {
                // Passée en fond puis finie aussitôt : déjà traitée par la tâche.
                Ok((_, true)) => TaskOutcome::Detached,
                Ok((outcome, false)) => TaskOutcome::Done(outcome),
                Err(error) => TaskOutcome::Done(Err(Error::Tool(format!("tâche interrompue : {error}")))),
            },
        }
    }
}

/// Relais des événements d'une tâche : suit ses étapes, décide de son
/// passage en arrière-plan, puis enveloppe ses événements.
async fn relay_task(
    app: Arc<App>,
    task_id: String,
    session_id: String,
    mut rx: tokio::sync::mpsc::Receiver<AgentEvent>,
    events: tokio::sync::mpsc::Sender<AgentEvent>,
    detach_tx: tokio::sync::oneshot::Sender<()>,
    detached: Arc<std::sync::atomic::AtomicBool>,
) {
    let mut first_tool: Option<std::time::Instant> = None;
    let mut tools = 0usize;
    // Demandes d'autorisation ouvertes (carte du Chat) : pas de passage en
    // fond pendant que l'utilisateur y répond.
    let mut approvals = 0usize;
    let mut detach_tx = Some(detach_tx);
    loop {
        // Après le premier outil, tant que la tâche peut passer en fond, on se
        // réveille aussi sans événement : un appel au modèle peut durer une minute.
        let wait = match (first_tool, detach_tx.is_some()) {
            (Some(at), true) => tasks::DETACH_AFTER.saturating_sub(at.elapsed()).max(std::time::Duration::from_millis(500)),
            _ => std::time::Duration::from_secs(3600),
        };
        let event = match tokio::time::timeout(wait, rx.recv()).await {
            Ok(Some(event)) => Some(event),
            Ok(None) => break,
            Err(_) => None,
        };
        match &event {
            Some(AgentEvent::ToolStart { name, arguments, .. }) => {
                tools += 1;
                first_tool.get_or_insert_with(std::time::Instant::now);
                app.tasks.set_step(&task_id, &tasks::step_label(name, arguments));
            }
            Some(AgentEvent::Progress { text }) => app.tasks.set_step(&task_id, text),
            Some(AgentEvent::Approval { .. }) => approvals += 1,
            Some(AgentEvent::ApprovalResolved { .. }) => approvals = approvals.saturating_sub(1),
            _ => {}
        }
        let is_final = matches!(event, Some(AgentEvent::Final { .. }) | Some(AgentEvent::Failed { .. }));
        if detach_tx.is_some()
            && !is_final
            && tasks::should_detach(first_tool.map(|at| at.elapsed()), tools, approvals > 0)
            && app.tasks.try_detach(&task_id)
        {
            detached.store(true, std::sync::atomic::Ordering::SeqCst);
            let title = app.tasks.get(&task_id).map(|t| t.title).unwrap_or_default();
            log::info!(
                "[agent] tâche passée en arrière-plan ({tools} outil(s), {} s après le premier) : « {title} »",
                first_tool.map(|at| at.elapsed().as_secs()).unwrap_or(0)
            );
            let _ = events
                .send(AgentEvent::Detached { task_id: task_id.clone(), session_id: session_id.clone(), title })
                .await;
            if let Some(tx) = detach_tx.take() {
                let _ = tx.send(());
            }
        }
        if let Some(event) = event {
            let event = if detached.load(std::sync::atomic::Ordering::SeqCst) {
                AgentEvent::Background { task_id: task_id.clone(), session_id: session_id.clone(), event: Box::new(event) }
            } else {
                event
            };
            let _ = events.send(event).await;
        }
    }
}

/// Prompts Whisper, un par rôle — mesurés, les deux serveurs ont des besoins
/// opposés :
/// - **mot d'éveil** (`base`, fenêtres de 2,4 s) : « Jimmy, » pousse le modèle
///   à écrire le nom (« et Jimmy. ») ; un prompt descriptif le faisait
///   halluciner sur un appel court (« Je sais pas, je sais pas ») ;
/// - **commande** (`small`, phrase entière) : avec « Jimmy, », le modèle
///   croyait le nom déjà dit et l'omettait (« dis-moi bonjour ») ; un prompt
///   qui cite le nom sans être la phrase précédente le conserve.
pub fn whisper_prompt(wake_word: &str, role: SttRole) -> String {
    let word = wake_word.trim();
    let mut chars = word.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    let name = format!("{}{}", first.to_uppercase(), chars.as_str());
    match role {
        SttRole::Wake => format!("{name}, "),
        SttRole::Command => format!("Discussion avec {name}, l'assistant vocal."),
    }
}

/// Rôle d'un serveur whisper (voir [`whisper_prompt`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SttRole {
    Wake,
    Command,
}

/// Mutex synchrone, pour les champs qui ne franchissent jamais un `await`.
pub type Mutex<T> = std::sync::Mutex<T>;

impl App {
    /// Construit Jimmy. Ne démarre aucun processus externe : la voix et
    /// l'avatar ne démarrent qu'à la demande via [`App::start_voice`] et
    /// [`App::start_avatar`].
    pub fn new(paths: Paths, secrets: Secrets) -> Result<Arc<Self>> {
        paths.ensure()?;
        let settings = Settings::load(&paths);
        let db: Shared<Db> = Arc::new(std::sync::Mutex::new(Db::open(&paths.db_file())?));
        let history = Arc::new(History::new(db.clone()));
        let memory = Arc::new(MemoryStore::with_semantic(
            db.clone(),
            memory::semantic::SemanticEmbedder::new(
                &settings.memory.embedding_url,
                &settings.memory.embedding_model,
            ),
        ));
        let skills = Arc::new(SkillStore::new(paths.skills_dir())?);

        let session_id = uuid::Uuid::new_v4().to_string();
        let llm = Arc::new(LlmClient::new(
            &settings.llm.base_url,
            &secrets.opencode_api_key,
            &format!("{}-{}", settings.llm.session_prefix, session_id),
        )?);
        let tts = Tts::new(providers::tts::TtsProvider::FishAudio)?;
        let avatar = AvatarClient::new(&settings.avatar.host, settings.avatar.port)?;

        let vault = if settings.memory.enabled && settings.memory.vault_enabled {
            memory::vault::Vault::open(&settings.memory.vault_path, &settings.memory.vault_folder)
                .map(Arc::new)
        } else {
            None
        };
        // Log clair : le vault est un chemin choisi par l'utilisateur, son
        // absence doit se comprendre immédiatement.
        match &vault {
            Some(v) => {
                log::info!("[vault] ouvert : {} ({} notes)", v.root().display(), v.count());
            }
            None => {
                if settings.memory.enabled && !settings.memory.vault_path.trim().is_empty() {
                    log::warn!(
                        "[vault] introuvable : {} — souvenirs écrits dans le vault ignorés",
                        settings.memory.vault_path
                    );
                }
            }
        }

        let registry = Arc::new(ToolRegistry::new());
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .user_agent("jimmy/0.1 (desktop agent)")
            .build()?;
        registry.register_defaults(&ToolDeps { http });

        // MCP : seuls les serveurs activés sont connus du registre. Leurs
        // outils n'arrivent qu'au démarrage effectif (`start_mcp`), car il
        // faut lancer chaque processus pour les lister.
        let mcp = mcp::McpRegistry::new(
            settings
                .mcp_servers
                .iter()
                .filter(|s| s.enabled)
                .map(tools::mcp::server_from_settings)
                .collect(),
        );
        // Les paramètres sont partagés dès maintenant : `mcp_add_server`
        // persiste les serveurs qu'il ajoute.
        let shared_settings = Arc::new(RwLock::new(settings.clone()));
        // Outils MCP à la demande (voir `tools::mcp`) : leurs définitions ne
        // partent plus à chaque appel au modèle.
        registry.register(Arc::new(tools::mcp::McpListTools::new(Arc::downgrade(&registry))));
        registry.register(Arc::new(tools::mcp::McpCall::new(Arc::downgrade(&registry))));
        registry.register(Arc::new(tools::mcp::McpAddServer::new(
            mcp.clone(),
            Arc::downgrade(&registry),
            shared_settings.clone(),
            paths.clone(),
        )));

        Ok(Arc::new(App {
            paths,
            settings: shared_settings,
            secrets: Arc::new(secrets),
            db,
            history,
            memory,
            skills,
            llm,
            tts,
            avatar,
            registry,
            approvals: Arc::new(sensitive::Approvals::default()),
            mcp,
            vault,
            stt: tokio::sync::Mutex::new(None),
            stt_command: tokio::sync::Mutex::new(None),
            last_vad_threshold: Mutex::new(0.0),
            speaking: std::sync::atomic::AtomicBool::new(false),
            godot: tokio::sync::Mutex::new(None),
            avatar_desired: std::sync::atomic::AtomicBool::new(false),
            avatar_spawning: tokio::sync::Mutex::new(()),
            avatar_watchdog: std::sync::atomic::AtomicBool::new(false),
            voice_session: std::sync::Mutex::new(None),
            stop_signal: tokio::sync::watch::channel(0).0,
            running: std::sync::atomic::AtomicUsize::new(0),
            tasks: Arc::new(tasks::Tasks::default()),
        }))
    }

    /// Arrêt d'urgence (« STOP », bouton « Arrêter ») : la tâche **au premier
    /// plan** est abandonnée — ses commandes en cours sont tuées
    /// (`kill_on_drop`) — et Jimmy se tait. Une tâche de fond continue : elle
    /// s'arrête par `stop_task` ou `request_stop_all` (décision du 7 octobre).
    /// Renvoie `true` s'il y avait quelque chose à arrêter.
    pub fn request_stop(&self) -> bool {
        let busy = self.is_busy();
        self.stop_signal.send_modify(|generation| *generation += 1);
        voice::interrupt_playback();
        log::info!("[agent] arrêt d'urgence demandé (tâche ou parole en cours : {busy})");
        busy
    }

    /// « Arrête tout » : premier plan, tâche de fond et parole.
    pub fn request_stop_all(&self) -> bool {
        let tasks = self.tasks.cancel_all();
        let foreground = self.request_stop();
        log::info!("[agent] arrêt de toutes les tâches ({tasks} en cours)");
        tasks > 0 || foreground
    }

    /// Arrête une tâche précise (bouton de la tâche de fond dans le Chat).
    pub fn stop_task(&self, id: &str) -> bool {
        let found = self.tasks.cancel(id);
        log::info!("[agent] arrêt de la tâche {id} demandé (trouvée : {found})");
        found
    }

    /// Une tâche tourne au premier plan, ou Jimmy parle. Une tâche de fond ne
    /// compte pas : Jimmy est disponible pendant qu'elle tourne.
    pub fn is_busy(&self) -> bool {
        self.running.load(std::sync::atomic::Ordering::Relaxed) > self.tasks.background_count()
            || self.speaking.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Nombre d'arrêts demandés depuis le lancement (sert à savoir si un arrêt
    /// a eu lieu pendant une opération).
    pub fn stop_generation(&self) -> u64 {
        *self.stop_signal.borrow()
    }

    /// Exécute `work` en l'exposant aux arrêts : `Error::Cancelled` sur son
    /// arrêt propre (`own`), ou sur « STOP » tant que la tâche est au premier
    /// plan. Passée en fond, elle ignore « STOP ».
    async fn guarded<T>(
        &self,
        task_id: &str,
        mut own: tokio::sync::watch::Receiver<bool>,
        work: impl std::future::Future<Output = Result<T>>,
    ) -> Result<T> {
        let _running = RunningGuard::new(&self.running);
        let mut stop = self.stop_signal.subscribe();
        stop.borrow_and_update();
        tokio::pin!(work);
        loop {
            tokio::select! {
                biased;
                _ = async {
                    // Émetteur disparu (tâche retirée) : ce n'est pas un arrêt.
                    if own.wait_for(|cancelled| *cancelled).await.is_err() {
                        std::future::pending::<()>().await;
                    }
                } => return Err(Error::Cancelled),
                changed = stop.changed() => {
                    if changed.is_err() || !self.tasks.is_background(task_id) {
                        return Err(Error::Cancelled);
                    }
                    log::info!("[agent] « STOP » ignoré par la tâche de fond {task_id}");
                }
                result = &mut work => return result,
            }
        }
    }

    /// Lance un tour de l'agent comme **tâche suivie** (Chat et voix).
    ///
    /// `work` reçoit l'émetteur d'événements de la tâche (typiquement
    /// `|tx| core::agent::run(…, tx)`). Ses événements sont relayés vers
    /// `events` tels quels tant qu'elle est au premier plan. Si elle dure (voir
    /// `tasks::DETACH_AFTER*`) et que la place de fond est libre, elle passe en
    /// arrière-plan : `AgentEvent::Detached`, puis tous ses événements
    /// enveloppés dans `AgentEvent::Background`, et sa fin (réponse, arrêt,
    /// échec) est traitée ici — annonce à voix haute comprise. L'appelant le
    /// sait par `TaskTicket::wait` → `TaskOutcome::Detached`.
    pub fn start_task<F, Fut>(
        self: &Arc<Self>,
        session_id: &str,
        request: &str,
        events: tokio::sync::mpsc::Sender<AgentEvent>,
        work: F,
    ) -> TaskTicket
    where
        F: FnOnce(tokio::sync::mpsc::Sender<AgentEvent>) -> Fut,
        Fut: std::future::Future<Output = Result<core::types::AgentAnswer>> + Send + 'static,
    {
        let (id, own) = self.tasks.begin(session_id, request);
        let (inner_tx, inner_rx) = tokio::sync::mpsc::channel::<AgentEvent>(64);
        let (detach_tx, detach_rx) = tokio::sync::oneshot::channel::<()>();
        let detached = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let relay = tokio::spawn(relay_task(
            self.clone(),
            id.clone(),
            session_id.to_string(),
            inner_rx,
            events.clone(),
            detach_tx,
            detached.clone(),
        ));
        let work = work(inner_tx);

        let app = self.clone();
        let task_id = id.clone();
        let session = session_id.to_string();
        let done = tokio::spawn(async move {
            let outcome = app.guarded(&task_id, own, work).await;
            // L'émetteur de la tâche est fermé : le relais se vide, ce qui
            // garde l'ordre (la réponse avant l'annonce de fin).
            let _ = relay.await;
            let title = app.tasks.get(&task_id).map(|t| t.title).unwrap_or_default();
            app.tasks.end(&task_id);
            let was_detached = detached.load(std::sync::atomic::Ordering::SeqCst);
            if was_detached {
                app.finish_background(&task_id, &session, &title, &events, &outcome).await;
            }
            (outcome, was_detached)
        });
        TaskTicket { id, detached: detach_rx, done }
    }

    /// Fin d'une tâche de fond : l'appelant a déjà rendu la main, c'est donc
    /// ici que l'arrêt est noté, l'échec signalé et la fin annoncée.
    async fn finish_background(
        &self,
        task_id: &str,
        session_id: &str,
        title: &str,
        events: &tokio::sync::mpsc::Sender<AgentEvent>,
        outcome: &Result<core::types::AgentAnswer>,
    ) {
        let wrap = |event: AgentEvent| AgentEvent::Background {
            task_id: task_id.to_string(),
            session_id: session_id.to_string(),
            event: Box::new(event),
        };
        match outcome {
            Ok(answer) => {
                log::info!("[agent] tâche de fond terminée : « {title} » ({} ms)", answer.duration_ms);
                // La réponse est déjà partie (`Final` enveloppé) ; on l'annonce
                // sans couper la parole : on attend que Jimmy soit libre.
                self.wait_until_free(tasks::ANNOUNCE_WAIT_MAX).await;
                self.speak(&format!("Tâche de fond terminée. {}", answer.text)).await;
            }
            // « Arrête tout » ou bouton de la tâche : le modèle doit savoir au
            // tour suivant que ce travail n'est pas fini.
            Err(Error::Cancelled) => {
                log::info!("[agent] tâche de fond arrêtée : « {title} »");
                let _ = self.history.append(
                    session_id,
                    &core::types::Message::assistant("(Tâche de fond arrêtée à la demande de l'utilisateur, avant la fin.)"),
                );
                let _ = events.send(wrap(AgentEvent::Final { text: tasks::BACKGROUND_STOPPED.into() })).await;
            }
            Err(error) => {
                log::error!("[agent] tâche de fond « {title} » en échec : {error}");
                let _ = events.send(wrap(AgentEvent::Failed { message: error.to_string() })).await;
                self.wait_until_free(tasks::ANNOUNCE_WAIT_MAX).await;
                self.speak("La tâche de fond a échoué. Le détail est dans le Chat.").await;
            }
        }
    }

    /// Attend que Jimmy ne parle plus et que rien ne tourne au premier plan
    /// (au plus `max`) : une annonce de fin ne coupe pas une conversation.
    async fn wait_until_free(&self, max: std::time::Duration) {
        let started = std::time::Instant::now();
        while self.is_busy() && started.elapsed() < max {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    }

    /// Connecte les serveurs MCP configurés et ajoute leurs outils au
    /// registre. Renvoie le nombre d'outils ajoutés.
    pub async fn start_mcp(&self) -> usize {
        tools::mcp::connect_all(&self.mcp, &self.registry).await
    }

    /// Serveurs MCP pour l'interface (Skills → Serveurs MCP) : ceux du
    /// registre avec leur état réel et leurs outils, puis ceux désactivés dans
    /// la configuration (absents du registre, mais l'utilisateur doit les voir).
    pub fn mcp_overview(&self) -> Vec<mcp::McpServerStatus> {
        let tools: Vec<(String, String)> = self
            .registry
            .specs()
            .into_iter()
            .filter(|t| t.name.starts_with("mcp_"))
            .map(|t| (t.name, t.description))
            .collect();
        let mut servers = self.mcp.status(&tools);
        for config in self.settings().mcp_servers.iter().filter(|s| !s.enabled) {
            if servers.iter().any(|s| s.name == config.name) {
                continue;
            }
            servers.push(mcp::McpServerStatus {
                name: config.name.clone(),
                transport: config.transport.clone(),
                launch: if config.command.is_empty() {
                    mcp::mask_command(std::slice::from_ref(&config.url))
                } else {
                    mcp::mask_command(&config.command)
                },
                env_keys: config.env.keys().cloned().collect(),
                state: "disabled",
                tools: Vec::new(),
            });
        }
        servers
    }

    pub fn settings(&self) -> Settings {
        self.settings.read().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn save_settings(&self, settings: Settings) -> Result<()> {
        *self.settings.write().map_err(|_| Error::Config("verrou poisoned".into()))? = settings;
        self.settings().save(&self.paths)
    }

    pub fn permissions(&self) -> Arc<RwLock<Permissions>> {
        // Les permissions sont stockées à côté du reste de la configuration ;
        // on les recharge à la demande pour ne pas avoir deux sources de vérité.
        Arc::new(RwLock::new(self.load_permissions()))
    }

    pub fn permissions_path(&self) -> std::path::PathBuf {
        self.paths.data.join("permissions.json")
    }

    pub fn load_permissions(&self) -> Permissions {
        let path = self.permissions_path();
        if let Ok(raw) = std::fs::read_to_string(&path) {
            if let Ok(parsed) = serde_json::from_str::<Permissions>(&raw) {
                return parsed;
            }
        }
        Permissions::default()
    }

    pub fn save_permissions(&self, permissions: &Permissions) -> Result<()> {
        std::fs::write(
            self.permissions_path(),
            serde_json::to_string_pretty(permissions)?,
        )?;
        Ok(())
    }

    /// Démarre l'avatar Godot comme processus annexe, s'il ne répond pas déjà.
    ///
    /// Le mode développement lance l'application depuis le dossier de projet ;
    /// en installation, l'exécutable exporté est utilisé. Les deux passent par
    /// la même commande : seule la résolution du binaire change.
    ///
    /// Si l'avatar meurt ensuite sans trace (processus tué, fenêtre fermée),
    /// le chien de garde démarré ici le relance, tant que l'utilisateur n'a
    /// pas demandé son arrêt explicitement.
    pub async fn start_avatar(self: &Arc<Self>) -> Result<()> {
        let settings = self.settings();
        if !settings.avatar.enabled {
            return Ok(());
        }
        self.avatar_desired
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let result = self.spawn_avatar(&settings).await;
        self.start_avatar_watchdog();
        result
    }

    /// Un seul lancement d'avatar à la fois : c'est la course entre deux
    /// lancements simultanés qui tuait l'avatar vivant (voir le champ
    /// `avatar_spawning`).
    async fn spawn_avatar(&self, settings: &Settings) -> Result<()> {
        let _spawn = self.avatar_spawning.lock().await;
        if self.avatar.is_up().await {
            return Ok(());
        }
        let exe = paths::find_godot_exe(&settings.avatar.godot_exe)?;
        let project = self.paths.godot_project();
        if !project.join("project.godot").is_file() {
            log::warn!("[avatar] projet Godot absent : {}", project.display());
            return Ok(());
        }

        let mut command = tokio::process::Command::new(&exe);
        command
            .arg("--path")
            .arg(&project)
            .arg("--rendering-driver")
            .arg("vulkan")
            .arg("--")
            .arg(format!("--port={}", settings.avatar.port))
            .arg(format!("--bridge-port={}", settings.avatar.bridge_port))
            .arg(format!("--skin={}", settings.avatar.skin))
            .arg(format!("--quality={}", settings.avatar.quality))
            .arg(format!("--dodge={}", u8::from(settings.avatar.dodge)))
            .env("JIMMY_GODOT_PORT", settings.avatar.port.to_string())
            .env("JIMMY_BRIDGE_PORT", settings.avatar.bridge_port.to_string())
            .env("JIMMY_SKIN", &settings.avatar.skin)
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true);
        #[cfg(windows)]
        {
            command.creation_flags(0x08000000);
        }

        let child = command
            .spawn()
            .map_err(|e| Error::Avatar(format!("lancement de Godot impossible : {e}")))?;
        log::info!("[avatar] Godot lancé ({})", exe.display());
        *self.godot.lock().await = Some(child);

        // Godot a besoin d'une seconde pour ouvrir son port. On sort tôt si le
        // processus meurt immédiatement (port déjà occupé, mauvais argument) :
        // attendre 10 s sur un mort ne servait qu'à masquer la cause.
        for _ in 0..40 {
            if self.avatar.is_up().await {
                self.avatar.set_quality(&settings.avatar.quality).await;
                // Le skin est déjà passé en argument (`--skin`) : rien à renvoyer.
                return Ok(());
            }
            if let Some(status) = self.godot_exit().await {
                self.godot.lock().await.take();
                return Err(Error::Avatar(format!(
                    "Godot s'est arrêté dès son lancement ({status}) : le port {} était peut-être déjà occupé",
                    settings.avatar.port
                )));
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        // Toujours en vie mais muet après 10 s (premier lancement, cache de
        // shaders) : on laisse tourner, le chien de garde confirmera.
        log::info!("[avatar] Godot vivant mais silencieux après 10 s");
        Ok(())
    }

    /// Statut du processus Godot s'il est terminé, `None` s'il tourne ou
    /// n'est pas lancé.
    async fn godot_exit(&self) -> Option<std::process::ExitStatus> {
        self.godot
            .lock()
            .await
            .as_mut()?
            .try_wait()
            .ok()
            .flatten()
    }

    /// Vrai si le processus Godot tient encore. Avant, un enfant terminé
    /// restait stocké dans `godot` : l'interface affichait « avatar en
    /// marche » pour un renard absent.
    pub fn avatar_running(&self) -> bool {
        self.godot
            .try_lock()
            .ok()
            .and_then(|mut g| {
                g.as_mut().map(|c| {
                    c.try_wait()
                        .map(|état| état.is_none())
                        .unwrap_or(true)
                })
            })
            .unwrap_or(false)
    }

    /// Relance l'avatar tant que l'utilisateur le veut et qu'il a disparu.
    /// Démarré une seule fois, au premier démarrage d'avatar.
    fn start_avatar_watchdog(self: &Arc<Self>) {
        if self
            .avatar_watchdog
            .swap(true, std::sync::atomic::Ordering::Relaxed)
        {
            return;
        }
        let app = Arc::clone(self);
        tokio::spawn(async move {
            let mut échecs = 0u32;
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                if !app
                    .avatar_desired
                    .load(std::sync::atomic::Ordering::Relaxed)
                {
                    continue;
                }
                // L'avatar en service est la vérité, pas l'enfant stocké : un
                // Godot orphelin d'une précédente instance répond déjà, et un
                // lancement peut être en cours. Sans ce test, le chien de
                // garde loguait « absent » toutes les 5 s pour un avatar
                // parfaitement visible.
                if app.avatar.is_up().await {
                    échecs = 0;
                    continue;
                }
                // Enfant vivant mais muet (premier lancement, cache de
                // shaders) : on patiente, on ne le tue pas.
                let enfant_vivant = {
                    let mut guard = app.godot.lock().await;
                    guard
                        .as_mut()
                        .map(|c| c.try_wait().ok().flatten().is_none())
                        .unwrap_or(false)
                };
                if enfant_vivant {
                    continue;
                }
                let mut message =
                    "Godot absent alors qu'il est demandé ; lancement".to_string();
                if let Some(status) = app.godot_exit().await {
                    message =
                        format!("Godot s'est arrêté ({status}) ; relance automatique");
                }
                log::warn!("[avatar] {message}");
                let settings = app.settings();
                match app.spawn_avatar(&settings).await {
                    Ok(()) => échecs = 0,
                    Err(error) => {
                        échecs = (échecs + 1).min(12);
                        log::warn!(
                            "[avatar] relance impossible (essai {}) : {error}",
                            échecs
                        );
                        // Retour croissant : une configuration cassée ne doit
                        // pas inonder le journal toutes les 5 s.
                        tokio::time::sleep(std::time::Duration::from_secs(
                            5u64.saturating_mul(échecs as u64),
                        ))
                        .await;
                    }
                }
            }
        });
    }

    pub async fn stop_avatar(&self) {
        self.avatar_desired
            .store(false, std::sync::atomic::Ordering::Relaxed);
        let mut guard = self.godot.lock().await;
        if guard.is_some() {
            log::info!("[avatar] arrêt demandé");
        }
        if let Some(mut child) = guard.take() {
            let _ = child.kill().await;
        }
    }

    /// Démarre whisper.cpp et la capture microphone.
    /// Démarre les serveurs de reconnaissance (si besoin) puis le micro.
    ///
    /// Renvoie une erreur si la voix est désactivée : avant, la fonction
    /// réussissait sans rien ouvrir et l'interface affichait « active ».
    pub async fn start_voice(&self, runtime: &voice::VoiceRuntime) -> Result<()> {
        let settings = self.settings();
        if !settings.voice.enabled || !settings.stt.enabled {
            return Err(Error::Voice(
                "la voix ou la reconnaissance vocale est désactivée dans les paramètres".into(),
            ));
        }
        self.ensure_stt().await?;
        runtime.start()
    }

    /// Démarre les serveurs whisper s'ils ne tournent pas déjà. Idempotent.
    ///
    /// Piège corrigé : remplacer un client `Stt` existant le détruisait, et
    /// son `kill_on_drop` tuait le serveur que le nouveau client venait de
    /// juger « déjà actif ». Au second démarrage de l'écoute, plus de
    /// reconnaissance. On garde désormais le client tant qu'il répond.
    pub async fn ensure_stt(&self) -> Result<()> {
        let settings = self.settings();
        if !settings.stt.enabled {
            return Err(Error::Stt("la reconnaissance vocale est désactivée".into()));
        }
        let server_exe = self.paths.whisper_dir().join("Release").join(&settings.stt.server_exe);
        let models = self.paths.whisper_dir().join("models");

        {
            let mut guard = self.stt.lock().await;
            let healthy = match guard.as_ref() {
                Some(stt) => stt.health().await,
                None => false,
            };
            if !healthy {
                if let Some(mut old) = guard.take() {
                    old.shutdown().await;
                }
                let mut stt = providers::Stt::new(settings.stt.port, &settings.stt.language)?
                    .with_prompt(&whisper_prompt(&settings.stt.wake_word, SttRole::Wake));
                // Serveur du mot d'éveil : contexte complet. Réduit, il
                // reconnaît mal les mots courts (« Dis-moi bonjour » →
                // « D'y ma bonjour », mesuré).
                stt.ensure_server(&server_exe, &models.join(&settings.stt.model), settings.stt.threads, 0)
                    .await?;
                *guard = Some(stt);
            }
        }

        // Second serveur pour la commande. Son échec n'est pas bloquant : la
        // commande retombe sur le modèle du wake word.
        let command_model = settings.stt.command_model.trim();
        if command_model.is_empty() || command_model == settings.stt.model {
            return Ok(());
        }
        let mut guard = self.stt_command.lock().await;
        let healthy = match guard.as_ref() {
            Some(stt) => stt.health().await,
            None => false,
        };
        if healthy {
            return Ok(());
        }
        if let Some(mut old) = guard.take() {
            old.shutdown().await;
        }
        let started = async {
            let mut stt = providers::Stt::new(settings.stt.command_port, &settings.stt.language)?
                .with_prompt(&whisper_prompt(&settings.stt.wake_word, SttRole::Command));
            stt.ensure_server(
                &server_exe,
                &models.join(command_model),
                settings.stt.threads,
                settings.stt.command_audio_ctx,
            )
                .await?;
            Ok::<_, Error>(stt)
        }
        .await;
        match started {
            Ok(stt) => {
                log::info!("[stt] commande : {command_model} (port {})", settings.stt.command_port);
                *guard = Some(stt);
            }
            Err(error) => log::warn!(
                "[stt] modèle de commande indisponible, repli sur {} : {error}",
                settings.stt.model
            ),
        }
        Ok(())
    }

    /// Jimmy est-il en train de parler ?
    pub fn is_speaking(&self) -> bool {
        self.speaking.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Dit `text` à voix haute (synthèse + lecture) et l'affiche dans la bulle
    /// de l'avatar. Utilisé par l'écoute **et** par le chat. Ne renvoie pas
    /// d'erreur : une voix indisponible ne doit pas faire échouer la réponse.
    pub async fn speak(&self, text: &str) {
        let settings = self.settings();
        if !settings.tts.enabled {
            return;
        }
        let cleaned = providers::tts::prepare_for_speech(text);
        if cleaned.is_empty() {
            return;
        }
        // La voix ne récite pas toute la réponse : on ne dit que le début utile
        // (borne), l'essentiel étant placé en tête par la consigne « Sortie » du
        // prompt ; le reste demeure dans la bulle. Une analyse d'un tour a déjà
        // été lue en 9 000 caractères, soit plusieurs minutes.
        let spoken = providers::tts::limit_for_speech(&cleaned, providers::tts::SPOKEN_MAX_CHARS);
        if spoken.is_empty() {
            return;
        }
        // Phrase par phrase : la première est jouée pendant que la suivante
        // est synthétisée. Avant, toute la réponse était synthétisée avant le
        // premier son — plusieurs secondes de silence pour une réponse longue.
        let parts = providers::tts::split_for_speech(&spoken, 220);
        log::info!(
            "[tts] lecture de {} caractères (prose nettoyée : {}) en {} morceau(x)",
            spoken.chars().count(),
            cleaned.chars().count(),
            parts.len()
        );
        let estimated = providers::tts::estimate_ms(&spoken, settings.tts.chars_per_minute);
        let _ = self.avatar.say(text, estimated).await;

        let synth = |part: String| {
            let tts = self.tts.clone();
            let key = self.secrets.openrouter_api_key.clone();
            let model = settings.tts.model.clone();
            let voice_id = settings.tts.voice.clone();
            let cpm = settings.tts.chars_per_minute;
            tokio::spawn(async move { tts.speak(&key, &model, &voice_id, &part, cpm).await })
        };

        self.speaking.store(true, std::sync::atomic::Ordering::Relaxed);
        let started = std::time::Instant::now();
        // Un « STOP » pendant la lecture coupe le reste de la réponse.
        let generation = self.stop_generation();
        let mut next = parts.first().cloned().map(&synth);
        for index in 0..parts.len() {
            if self.stop_generation() != generation {
                log::info!("[tts] lecture arrêtée (arrêt d'urgence)");
                break;
            }
            let Some(pending) = next.take() else { break };
            let speech = match pending.await {
                Ok(Ok(speech)) => speech,
                Ok(Err(error)) => {
                    log::warn!("[tts] {error}");
                    break;
                }
                Err(_) => break,
            };
            // Synthèse du morceau suivant pendant la lecture de celui-ci.
            next = parts.get(index + 1).cloned().map(&synth);
            if index == 0 {
                log::info!("[tts] premier son après {} ms", started.elapsed().as_millis());
            }
            if speech.bytes.is_empty() {
                continue;
            }
            if self.stop_generation() != generation {
                break;
            }
            let bytes = speech.bytes;
            let rate = speech.sample_rate;
            let outcome = tokio::task::spawn_blocking(move || voice::play_bytes(&bytes, rate)).await;
            if let Ok(Err(error)) = outcome {
                log::warn!("[voice] lecture impossible : {error}");
                break;
            }
        }
        self.speaking.store(false, std::sync::atomic::Ordering::Relaxed);
    }

    /// Transcrit une commande avec le modèle précis, ou le modèle du wake
    /// word si le second serveur n'est pas disponible.
    ///
    /// Auto-réparation : si la requête échoue (serveur arrêté, planté, ou tué
    /// par un autre processus qui le possédait), les serveurs sont vérifiés
    /// et relancés, puis la transcription est retentée une fois. Avant, un
    /// serveur mort rendait l'écoute muette jusqu'au redémarrage de Jimmy.
    pub async fn transcribe_command(&self, wav: Vec<u8>) -> Result<String> {
        match self.transcribe_command_once(wav.clone()).await {
            Ok(text) => Ok(text),
            Err(error) => {
                log::warn!("[stt] commande : {error} — vérification des serveurs et nouvel essai");
                self.ensure_stt().await?;
                self.transcribe_command_once(wav).await
            }
        }
    }

    async fn transcribe_command_once(&self, wav: Vec<u8>) -> Result<String> {
        {
            let guard = self.stt_command.lock().await;
            if let Some(stt) = guard.as_ref() {
                return stt.transcribe_wav(wav).await;
            }
        }
        self.transcribe_wake_once(wav).await
    }

    /// Transcription rapide (fenêtre du mot d'éveil), auto-réparée comme
    /// [`App::transcribe_command`].
    pub async fn transcribe_wake(&self, wav: Vec<u8>) -> Result<String> {
        match self.transcribe_wake_once(wav.clone()).await {
            Ok(text) => Ok(text),
            Err(error) => {
                log::warn!("[stt] mot d'éveil : {error} — vérification des serveurs et nouvel essai");
                self.ensure_stt().await?;
                self.transcribe_wake_once(wav).await
            }
        }
    }

    async fn transcribe_wake_once(&self, wav: Vec<u8>) -> Result<String> {
        let guard = self.stt.lock().await;
        let stt = guard
            .as_ref()
            .ok_or_else(|| Error::Stt("la reconnaissance vocale n'est pas démarrée".into()))?;
        stt.transcribe_wav(wav).await
    }

    /// Contexte d'outils prêt à l'emploi pour une demande.
    /// Réglages et contexte d'outils pour une conversation : si elle est
    /// rattachée à un projet (onglet Chat) qui existe, l'agent y travaille
    /// (dossier de travail du prompt et des outils). Partagé par le Chat écrit
    /// et la boucle vocale : avant, la voix ignorait le projet.
    pub fn session_context(&self, session_id: &str) -> (Settings, tools::ToolContext) {
        let mut settings = self.settings();
        let mut context = self.tool_context();
        if let Some(project) = self
            .history
            .project(session_id)
            .ok()
            .flatten()
            .filter(|p| std::path::Path::new(p).is_dir())
        {
            settings.workspace = project.clone();
            context.workspace = std::path::PathBuf::from(project);
        }
        (settings, context)
    }

    pub fn tool_context(&self) -> tools::ToolContext {
        let settings = self.settings();
        tools::ToolContext {
            workspace: std::path::PathBuf::from(&settings.workspace),
            permissions: self.permissions(),
            memory: self.memory.clone(),
            skills: self.skills.clone(),
            vault: self.vault.clone(),
        }
    }

    pub fn deps(&self) -> Arc<core::agent::AgentDeps> {
        Arc::new(core::agent::AgentDeps {
            llm: self.llm.clone(),
            history: self.history.clone(),
            memory: self.memory.clone(),
            skills: self.skills.clone(),
            vault: self.vault.clone(),
            registry: self.registry.clone(),
            data_dir: self.paths.data.clone(),
            approvals: self.approvals.clone(),
            tasks: self.tasks.clone(),
            voice: false,
        })
    }

    /// Dépendances d'une demande vocale (voir `AgentDeps::voice`).
    pub fn deps_voice(&self) -> Arc<core::agent::AgentDeps> {
        let mut deps = (*self.deps()).clone();
        deps.voice = true;
        Arc::new(deps)
    }

    /// État affiché dans l'interface : ce qui est prêt, ce qui manque.
    pub fn status(&self) -> serde_json::Value {
        let settings = self.settings();
        serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            "dev": self.paths.dev,
            "data_dir": self.paths.data,
            "workspace": settings.workspace,
            "llm": {
                "model": settings.llm.model,
                "voice_model": settings.llm.voice_model,
                "base_url": settings.llm.base_url,
                "has_key": !self.secrets.opencode_api_key.is_empty(),
            },
            "tts": {
                "enabled": settings.tts.enabled,
                "model": settings.tts.model,
                "voice": settings.tts.voice,
                "has_key": !self.secrets.openrouter_api_key.is_empty(),
                // Voix prédéfinies puis celles de la bibliothèque.
                "voices": TtsVoice::PRESETS.iter().map(|v| v.info())
                    .chain(settings.tts.library.iter().cloned())
                    .map(|v| serde_json::json!({ "id": v.id, "label": v.label, "description": v.description }))
                    .collect::<Vec<_>>(),
            },
            "stt": {
                "enabled": settings.stt.enabled,
                "model": settings.stt.model,
                "command_model": settings.stt.command_model,
                "language": settings.stt.language,
                "wake_word": settings.stt.wake_word,
                "models": providers::stt::MODELS.iter().map(|(id, label, note)| serde_json::json!({
                    "id": id, "label": label, "note": note
                })).collect::<Vec<_>>(),
            },
            "avatar": {
                "enabled": settings.avatar.enabled,
                "skin": settings.avatar.skin,
                "skins": providers::avatar::SKINS.iter().map(|(id, label)| serde_json::json!({
                    "id": id, "label": label
                })).collect::<Vec<_>>(),
                "quality": settings.avatar.quality,
                "dodge": settings.avatar.dodge,
                "host": settings.avatar.host,
                "port": settings.avatar.port,
                "running": self.avatar_running(),
            },
            "memory": {
                "enabled": settings.memory.enabled,
                "count": self.memory.count().unwrap_or(0),
                "semantic_model": self.memory.semantic_model(),
                "has_fts": self.db.lock().map(|db| db.has_fts()).unwrap_or(false),
            },
            "vault": {
                "enabled": settings.memory.enabled && settings.memory.vault_enabled && self.vault.is_some(),
                "path": settings.memory.vault_path,
                "folder": settings.memory.vault_folder,
                "notes": self.vault.as_ref().map(|v| v.count()).unwrap_or(0),
            },
            "permissions": self.load_permissions().summary()
                .iter()
                .map(|(capability, granted)| serde_json::json!({
                    "capability": capability.as_str(), "granted": granted
                }))
                .collect::<Vec<_>>(),
            "tools": self.registry.names(),
            "mcp": {
                "servers": self.mcp.server_names(),
                "tools": self.registry.names().iter().filter(|n| n.starts_with("mcp_")).count(),
            },
            "skills": self.skills.list().map(|s| s.len()).unwrap_or(0),
            "first_run_done": settings.ui.first_run_done,
        })
    }
}