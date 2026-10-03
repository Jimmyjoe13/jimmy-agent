//! Configuration de Jimmy, séparée en deux.
//!
//! * [`Settings`] — tout ce que l'utilisateur peut changer depuis l'interface.
//!   Persisté dans `data/config.json`.
//! * [`Secrets`] — clés API et mots de passe. Chargés depuis `.env` et
//!   l'environnement uniquement : jamais écrits sur disque par Jimmy, jamais
//!  /journalisés.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::paths::Paths;

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmSettings {
    /// Identifiant complet du modèle chez OpenCode Go, ex. `space-bunny-free`.
    pub model: String,
    pub base_url: String,
    /// Température ; `None` = réglage par défaut du fournisseur.
    pub temperature: Option<f32>,
    pub max_tokens: u32,
    /// Nombre maximum d'allers-retours agent/outils sur une demande.
    pub max_iterations: u32,
    /// Préfixe d'identifiant de session envoyé dans `x-opencode-session`.
    pub session_prefix: String,
}

impl Default for LlmSettings {
    fn default() -> Self {
        LlmSettings {
            model: "space-bunny-free".into(),
            base_url: "https://opencode.ai/zen/go/v1".into(),
            temperature: None,
            max_tokens: 4096,
            max_iterations: 12,
            session_prefix: "jimmy".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TtsSettings {
    pub enabled: bool,
    pub provider: String,
    pub model: String,
    /// Identifiant de voix Fish Audio.
    pub voice: String,
    /// Vitesse de lecture simulée (utilisée pour l'estimation de durée).
    pub chars_per_minute: f32,
    /// Sons d'état (« Oui ? », « C'est prêt. », « Oups… »), voir `voice::cues`.
    #[serde(default = "default_true")]
    pub cues: bool,
}

impl Default for TtsSettings {
    fn default() -> Self {
        TtsSettings {
            enabled: true,
            provider: "fish-audio".into(),
            model: "fish-audio/s2.1-pro-free:free".into(),
            voice: "5567200c7d8341738f0892bbacd3be3c".into(),
            chars_per_minute: 1000.0,
            cues: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SttSettings {
    pub enabled: bool,
    pub language: String,
    /// Modèle whisper.cpp à charger.
    pub model: String,
    /// `whisper-server.exe` : chemin explicite ou nom à trouver dans le dossier
    /// des composants.
    pub server_exe: String,
    pub port: u16,
    /// Fenêtre glissante d'écoute du wake word, en millisecondes.
    pub wake_window_ms: u64,
    /// Mot(s) déclenchant(s).
    pub wake_word: String,
    /// Nombre de threads envoyés à whisper.cpp (0 = automatique).
    pub threads: u32,
    /// Modèle dédié à la **commande** (la phrase après « Jimmy »). Le wake
    /// word reste sur `model`, rapide ; la commande, transcrite une seule
    /// fois, mérite un modèle plus précis. Vide ou identique à `model` = un
    /// seul serveur.
    #[serde(default = "default_command_model")]
    pub command_model: String,
    /// Port du second `whisper-server` (commande).
    #[serde(default = "default_command_port")]
    pub command_port: u16,
}

fn default_command_model() -> String {
    "ggml-small-q5_1.bin".into()
}

fn default_command_port() -> u16 {
    8179
}

impl Default for SttSettings {
    fn default() -> Self {
        SttSettings {
            enabled: true,
            language: "fr".into(),
            model: "ggml-base-q5_1.bin".into(),
            server_exe: "whisper-server.exe".into(),
            port: 8178,
            wake_window_ms: 2400,
            wake_word: "jimmy".into(),
            threads: 0,
            command_model: default_command_model(),
            command_port: default_command_port(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceSettings {
    pub enabled: bool,
    /// Volume de lecture, 0.0 à 1.0.
    pub volume: f32,
    /// Échantillonnage du flux micro (le STT travaille en 16 kHz).
    pub input_sample_rate: u32,
    /// Seuil de détection d'activité vocale (0.0 à 1.0).
    pub vad_threshold: f32,
    /// Nombre de millisecondes de silence qui clôturent une phrase.
    pub end_of_speech_ms: u64,
    /// L'écoute reprend au lancement de Jimmy. Mis à jour quand l'utilisateur
    /// active ou coupe l'écoute : Jimmy reste comme on l'a laissé.
    #[serde(default)]
    pub listen_on_start: bool,
}

impl Default for VoiceSettings {
    fn default() -> Self {
        VoiceSettings {
            enabled: true,
            volume: 0.9,
            input_sample_rate: 16_000,
            vad_threshold: 0.012,
            end_of_speech_ms: 900,
            listen_on_start: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvatarSettings {
    pub enabled: bool,
    pub skin: String,
    /// `low` | `medium` | `high`
    pub quality: String,
    pub host: String,
    pub port: u16,
    pub bridge_host: String,
    pub bridge_port: u16,
    pub godot_exe: String,
    /// Lancement automatique de l'avatar au démarrage de Jimmy.
    pub autostart: bool,
    /// Jimmy s'écarte quand le curseur approche, puis revient à sa place.
    #[serde(default = "default_true")]
    pub dodge: bool,
}

impl Default for AvatarSettings {
    fn default() -> Self {
        AvatarSettings {
            enabled: true,
            skin: "renard".into(),
            quality: "medium".into(),
            host: "127.0.0.1".into(),
            port: 8787,
            bridge_host: "127.0.0.1".into(),
            bridge_port: 8790,
            godot_exe: String::new(),
            autostart: true,
            dodge: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySettings {
    pub enabled: bool,
    /// Nombre de souvenirs injectés dans le contexte à chaque tour.
    pub recall_limit: u32,
    /// Seuil de similarité cosine minimal pour considérer un souvenir pertinent.
    pub min_score: f32,
    /// Nombre de conversations analysées pour construire la mémoire.
    pub auto_learn_every: u32,
    /// Extraction automatique des préférences à la fin d'une session.
    pub auto_learn: bool,
    /// API d'embeddings compatible OpenAI (LM Studio). Vide = hachage seul.
    #[serde(default = "default_embedding_url")]
    pub embedding_url: String,
    /// Même modèle que SynaptiQ : multilingue, bon en français, 384 dim.
    #[serde(default = "default_embedding_model")]
    pub embedding_model: String,
}

fn default_embedding_url() -> String {
    "http://localhost:1234/v1".into()
}

fn default_embedding_model() -> String {
    "text-embedding-paraphrase-multilingual-minilm-l12-v2.gguf".into()
}

impl Default for MemorySettings {
    fn default() -> Self {
        MemorySettings {
            enabled: true,
            recall_limit: 6,
            min_score: 0.12,
            auto_learn_every: 4,
            auto_learn: true,
            embedding_url: default_embedding_url(),
            embedding_model: default_embedding_model(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SynaptiqSettings {
    pub enabled: bool,
    pub base_url: String,
    /// Seuil de longueur de demande au-delà duquel Jimmy peut consulter
    /// Synaptiq (0 = jamais automatiquement).
    pub min_request_chars: u32,
    pub timeout_ms: u64,
}

impl Default for SynaptiqSettings {
    fn default() -> Self {
        SynaptiqSettings {
            enabled: true,
            base_url: "http://127.0.0.1:8000".into(),
            min_request_chars: 180,
            timeout_ms: 20_000,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum StartupMode {
    /// Ne démarre pas avec Windows.
    Manual,
    /// Démarre avec Windows, fenêtre visible.
    WithWindows,
    /// Démarre avec Windows, masqué dans la zone de notification.
    WithWindowsHidden,
}

impl Default for StartupMode {
    fn default() -> Self {
        StartupMode::Manual
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiSettings {
    pub theme: String,
    pub first_run_done: bool,
    pub user_name: String,
    /// Position de la fenêtre principale, mémorisée pour ne pas la perdre.
    pub window: WindowState,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WindowState {
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub width: u32,
    pub height: u32,
}

impl Default for UiSettings {
    fn default() -> Self {
        UiSettings {
            theme: "sombre".into(),
            first_run_done: false,
            user_name: "Jimmy".into(),
            window: WindowState {
                x: None,
                y: None,
                width: 1040,
                height: 720,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub name: String,
    pub transport: String,
    #[serde(default)]
    pub command: Vec<String>,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub llm: LlmSettings,
    pub tts: TtsSettings,
    pub stt: SttSettings,
    pub voice: VoiceSettings,
    pub avatar: AvatarSettings,
    pub memory: MemorySettings,
    pub synaptiq: SynaptiqSettings,
    pub ui: UiSettings,
    pub startup: StartupMode,
    #[serde(default)]
    pub mcp_servers: Vec<McpServerConfig>,
    /// Racine de travail par défaut des tools (dossier analysé par défaut).
    pub workspace: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            llm: LlmSettings::default(),
            tts: TtsSettings::default(),
            stt: SttSettings::default(),
            voice: VoiceSettings::default(),
            avatar: AvatarSettings::default(),
            memory: MemorySettings::default(),
            synaptiq: SynaptiqSettings::default(),
            ui: UiSettings::default(),
            startup: StartupMode::default(),
            mcp_servers: Vec::new(),
            workspace: "C:\\Users\\jimmy\\Projet".into(),
        }
    }
}

impl Settings {
    pub fn load(paths: &Paths) -> Self {
        let path = paths.config_file();
        if !path.is_file() {
            return Settings::default();
        }
        match std::fs::read_to_string(&path) {
            Ok(raw) => match serde_json::from_str::<Settings>(&raw) {
                Ok(mut s) => {
                    s.apply_env();
                    s
                }
                Err(err) => {
                    log::warn!("config illisible ({}), retour aux valeurs par défaut", err);
                    Settings::default()
                }
            },
            Err(_) => Settings::default(),
        }
    }

    pub fn save(&self, paths: &Paths) -> Result<()> {
        let path = paths.config_file();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let raw = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, raw)?;
        Ok(())
    }

    /// L'environnement peut surcharger quelques valeurs (utile en dev et pour
    /// l'installateur). La configuration utilisateur reste prioritaire pour
    /// tout ce qui est choix d'expérience (modèle, voix, qualité).
    fn apply_env(&mut self) {
        if let Ok(v) = std::env::var("JIMMY_LLM_MODEL") {
            if !v.is_empty() {
                self.llm.model = v;
            }
        }
        if let Ok(v) = std::env::var("JIMMY_TTS_MODEL") {
            if !v.is_empty() {
                self.tts.model = v;
            }
        }
        if let Ok(v) = std::env::var("JIMMY_TTS_VOICE") {
            if !v.is_empty() {
                self.tts.voice = v;
            }
        }
        if let Ok(v) = std::env::var("JIMMY_STT_LANGUAGE") {
            if !v.is_empty() {
                self.stt.language = v;
            }
        }
        if let Ok(v) = std::env::var("JIMMY_GODOT_PORT") {
            if let Ok(p) = v.parse() {
                self.avatar.port = p;
            }
        }
        if let Ok(v) = std::env::var("JIMMY_BRIDGE_PORT") {
            if let Ok(p) = v.parse() {
                self.avatar.bridge_port = p;
            }
        }
        if let Ok(v) = std::env::var("JIMMY_GODOT_EXE") {
            if !v.is_empty() {
                self.avatar.godot_exe = v;
            }
        }
    }
}

/// Secrets lus depuis l'environnement et le fichier `.env`.
#[derive(Debug, Clone, Default)]
pub struct Secrets {
    pub opencode_api_key: String,
    pub openrouter_api_key: String,
    pub synaptiq_api_key: String,
    pub synaptiq_agent_id: String,
}

impl Secrets {
    pub fn from_env() -> Self {
        Secrets {
            opencode_api_key: env("OPENCODE_API_KEY"),
            openrouter_api_key: env("OPENROUTER_API_KEY"),
            synaptiq_api_key: env("SYNAPTIQ_API_KEY"),
            synaptiq_agent_id: match env("SYNAPTIQ_AGENT_ID") {
                v if v.is_empty() => "jimmy".to_string(),
                v => v,
            },
        }
    }

    pub fn load_from_dotenv(path: &Path) -> Result<Self> {
        crate::paths::load_dotenv(path)?;
        Ok(Secrets::from_env())
    }
}

fn env(key: &str) -> String {
    std::env::var(key).unwrap_or_default()
}

/// Liste des clés manquantes, pour l'onboarding. Ne renvoie jamais les valeurs.
pub fn missing_secrets(secrets: &Secrets) -> Vec<&'static str> {
    let mut missing = Vec::new();
    if secrets.opencode_api_key.is_empty() {
        missing.push("OPENCODE_API_KEY");
    }
    if secrets.openrouter_api_key.is_empty() {
        missing.push("OPENROUTER_API_KEY");
    }
    missing
}

/// Erreur explicite quand un service obligatoire n'a pas de clé.
pub fn require(value: &str, name: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(Error::Config(format!("{name} n'est pas configuré")));
    }
    Ok(())
}