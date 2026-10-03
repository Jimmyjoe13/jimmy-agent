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
pub mod memory;
pub mod mcp;
pub mod paths;
pub mod permissions;
pub mod providers;
pub mod skills;
pub mod synaptiq;
pub mod tools;
pub mod voice;

use std::sync::{Arc, RwLock};

pub use error::{Error, Result};

use config::{Secrets, Settings};
use core::history::{History, Shared};
use db::Db;
use memory::MemoryStore;
use paths::Paths;
use permissions::Permissions;
use providers::{AvatarClient, LlmClient, Stt, Tts, TtsVoice};
use skills::SkillStore;
use synaptiq::SynaptiqClient;
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
    /// Serveurs MCP : leurs outils sont ajoutés au registre par [`App::start_mcp`].
    pub mcp: Arc<mcp::McpRegistry>,
    pub synaptiq: Option<Arc<SynaptiqClient>>,
    /// Verrous asynchrones : ces deux champs sont utilisés à travers des
    /// `await`, un `std::sync::Mutex` rendrait la future non `Send`.
    pub stt: tokio::sync::Mutex<Option<Stt>>,
    /// Second serveur, modèle précis, réservé à la commande. `None` = la
    /// commande passe par `stt` (repli).
    pub stt_command: tokio::sync::Mutex<Option<Stt>>,
    /// Dernier seuil de VAD utilisé par l'écoute (diagnostic, vue Voix).
    pub last_vad_threshold: Mutex<f32>,
    pub godot: tokio::sync::Mutex<Option<tokio::process::Child>>,
}

/// Contexte donné à Whisper : le nom de l'assistant, capitalisé, en début
/// de phrase. « jimmy » → « Jimmy, ».
pub fn whisper_prompt(wake_word: &str) -> String {
    let word = wake_word.trim();
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => format!("{}{}, ", first.to_uppercase(), chars.as_str()),
        None => String::new(),
    }
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

        let synaptiq = if settings.synaptiq.enabled && !secrets.synaptiq_api_key.is_empty() {
            Some(Arc::new(SynaptiqClient::new(
                &settings.synaptiq.base_url,
                &secrets.synaptiq_api_key,
                &secrets.synaptiq_agent_id,
                settings.synaptiq.timeout_ms,
            )?))
        } else {
            None
        };

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
            mcp,
            synaptiq,
            stt: tokio::sync::Mutex::new(None),
            stt_command: tokio::sync::Mutex::new(None),
            last_vad_threshold: Mutex::new(0.0),
            godot: tokio::sync::Mutex::new(None),
        }))
    }

    /// Connecte les serveurs MCP configurés et ajoute leurs outils au
    /// registre. Renvoie le nombre d'outils ajoutés.
    pub async fn start_mcp(&self) -> usize {
        tools::mcp::connect_all(&self.mcp, &self.registry).await
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
    pub async fn start_avatar(&self) -> Result<()> {
        let settings = self.settings();
        if !settings.avatar.enabled {
            return Ok(());
        }
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
        {
            let mut guard = self.godot.lock().await;
            *guard = Some(child);
        }

        // Godot a besoin d'une seconde pour ouvrir son port.
        let avatar = &self.avatar;
        for _ in 0..40 {
            if avatar.is_up().await {
                avatar.set_quality(&settings.avatar.quality).await;
                // Le skin est déjà passé en argument (`--skin`) : rien à renvoyer.
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        Ok(())
    }

    pub async fn stop_avatar(&self) {
        let mut guard = self.godot.lock().await;
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
                    .with_prompt(&whisper_prompt(&settings.stt.wake_word));
                stt.ensure_server(&server_exe, &models.join(&settings.stt.model), settings.stt.threads)
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
                .with_prompt(&whisper_prompt(&settings.stt.wake_word));
            stt.ensure_server(&server_exe, &models.join(command_model), settings.stt.threads)
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

    /// Transcrit une commande avec le modèle précis, ou le modèle du wake
    /// word si le second serveur n'est pas disponible.
    pub async fn transcribe_command(&self, wav: Vec<u8>) -> Result<String> {
        {
            let guard = self.stt_command.lock().await;
            if let Some(stt) = guard.as_ref() {
                return stt.transcribe_wav(wav).await;
            }
        }
        let guard = self.stt.lock().await;
        let stt = guard
            .as_ref()
            .ok_or_else(|| Error::Stt("la reconnaissance vocale n'est pas démarrée".into()))?;
        stt.transcribe_wav(wav).await
    }

    /// Contexte d'outils prêt à l'emploi pour une demande.
    pub fn tool_context(&self) -> tools::ToolContext {
        let settings = self.settings();
        tools::ToolContext {
            workspace: std::path::PathBuf::from(&settings.workspace),
            permissions: self.permissions(),
            memory: self.memory.clone(),
            skills: self.skills.clone(),
            synaptiq: self.synaptiq.clone(),
        }
    }

    pub fn deps(&self) -> Arc<core::agent::AgentDeps> {
        Arc::new(core::agent::AgentDeps {
            llm: self.llm.clone(),
            history: self.history.clone(),
            memory: self.memory.clone(),
            skills: self.skills.clone(),
            synaptiq: self.synaptiq.clone(),
            registry: self.registry.clone(),
        })
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
                "base_url": settings.llm.base_url,
                "has_key": !self.secrets.opencode_api_key.is_empty(),
            },
            "tts": {
                "enabled": settings.tts.enabled,
                "model": settings.tts.model,
                "voice": settings.tts.voice,
                "has_key": !self.secrets.openrouter_api_key.is_empty(),
                "voices": TtsVoice::PRESETS.iter().map(|v| serde_json::json!({
                    "id": v.id, "label": v.label, "description": v.description
                })).collect::<Vec<_>>(),
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
                "host": settings.avatar.host,
                "port": settings.avatar.port,
                "running": self.godot.try_lock().map(|g| g.is_some()).unwrap_or(false),
            },
            "memory": {
                "enabled": settings.memory.enabled,
                "count": self.memory.count().unwrap_or(0),
                "semantic_model": self.memory.semantic_model(),
                "has_fts": self.db.lock().map(|db| db.has_fts()).unwrap_or(false),
            },
            "synaptiq": {
                "enabled": settings.synaptiq.enabled,
                "configured": self.synaptiq.is_some(),
                "base_url": settings.synaptiq.base_url,
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