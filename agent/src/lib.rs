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
    pub synaptiq: Option<Arc<SynaptiqClient>>,
    /// Verrous asynchrones : ces deux champs sont utilisés à travers des
    /// `await`, un `std::sync::Mutex` rendrait la future non `Send`.
    pub stt: tokio::sync::Mutex<Option<Stt>>,
    pub godot: tokio::sync::Mutex<Option<tokio::process::Child>>,
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
        let memory = Arc::new(MemoryStore::new(db.clone()));
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

        let mut registry = ToolRegistry::new();
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .user_agent("jimmy/0.1 (desktop agent)")
            .build()?;
        registry.register_defaults(&ToolDeps { http });

        Ok(Arc::new(App {
            paths,
            settings: Arc::new(RwLock::new(settings)),
            secrets: Arc::new(secrets),
            db,
            history,
            memory,
            skills,
            llm,
            tts,
            avatar,
            registry: Arc::new(registry),
            synaptiq,
            stt: tokio::sync::Mutex::new(None),
            godot: tokio::sync::Mutex::new(None),
        }))
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
    pub async fn start_voice(&self, runtime: &voice::VoiceRuntime) -> Result<()> {
        let settings = self.settings();
        if !settings.voice.enabled || !settings.stt.enabled {
            return Ok(());
        }
        let server_exe = self.paths.whisper_dir().join("Release").join(&settings.stt.server_exe);
        let model = self
            .paths
            .whisper_dir()
            .join("models")
            .join(&settings.stt.model);
        let mut stt = providers::Stt::new(settings.stt.port, &settings.stt.language)?;
        stt.ensure_server(&server_exe, &model, settings.stt.threads).await?;
        {
            let mut guard = self.stt.lock().await;
            *guard = Some(stt);
        }
        runtime.start()
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
            "skills": self.skills.list().map(|s| s.len()).unwrap_or(0),
            "first_run_done": settings.ui.first_run_done,
        })
    }
}