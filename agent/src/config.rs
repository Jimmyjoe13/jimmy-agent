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

fn default_opencode_id() -> String {
    PROVIDER_OPENCODE.into()
}

/// Méthodes d'accès d'un fournisseur.
pub const AUTH_API_KEY: &str = "api-key";
/// Abonnement Claude (Pro/Max) : jetons OAuth relus depuis la session de
/// Claude Code, rafraîchis et réécrits par Jimmy (`providers::claude_plan`).
pub const AUTH_CLAUDE_PLAN: &str = "claude-plan";

fn default_auth_api_key() -> String {
    AUTH_API_KEY.into()
}

/// Un fournisseur de modèles de langage : URL de base, clé, format d'API.
///
/// Les clés sont saisies dans l'interface (Paramètres → LLM) et stockées dans
/// `data/config.json` — jamais commité, jamais journalisé. La clé de
/// l'environnement (`OPENCODE_API_KEY`…) ne sert que de **valeur de repli**
/// quand aucune clé n'est saisie : un choix de l'utilisateur ne doit jamais
/// être écrasé (piège 62).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderConfig {
    /// Identifiant stable : `opencode`, `openrouter`, `deepseek`, `alibaba`,
    /// `anthropic`, ou `custom-N` pour ceux ajoutés par l'utilisateur.
    pub id: String,
    pub label: String,
    pub base_url: String,
    /// Clé d'API saisie dans l'interface. Vide = repli sur l'environnement.
    #[serde(default)]
    pub api_key: String,
    /// Format d'API : `"chat"`, `"responses"` ou `"messages"`. `None` =
    /// automatique (catalogue OpenCode Go, puis essai des trois formats).
    #[serde(default)]
    pub protocol: Option<String>,
    /// La clé voyage dans l'en-tête `x-api-key` (Anthropic) au lieu de
    /// `Authorization: Bearer`.
    #[serde(default)]
    pub x_api_key: bool,
    /// Méthode d'accès : `"api-key"` (défaut) ou `"claude-plan"` — abonnement
    /// Claude réutilisé depuis la session de Claude Code (jetons OAuth, voir
    /// `providers::claude_plan`). Réservé au fournisseur Anthropic.
    #[serde(default = "default_auth_api_key")]
    pub auth: String,
    /// Ce fournisseur exige l'en-tête `x-opencode-session` (OpenCode Go,
    /// `400 MissingSessionID` sans lui).
    #[serde(default)]
    pub session_header: bool,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Fournisseur intégré : non supprimable, sa clé peut venir de
    /// l'environnement.
    #[serde(default)]
    pub builtin: bool,
}

/// Identifiants des fournisseurs intégrés (stables : ils vivent dans
/// `config.json` et dans les choix de l'utilisateur).
pub const PROVIDER_OPENCODE: &str = "opencode";
pub const PROVIDER_OPENROUTER: &str = "openrouter";
pub const PROVIDER_DEEPSEEK: &str = "deepseek";
pub const PROVIDER_ALIBABA: &str = "alibaba";
pub const PROVIDER_ANTHROPIC: &str = "anthropic";

impl ProviderConfig {
    /// Vrai si ce fournisseur est en mode abonnement Claude.
    pub fn is_claude_plan(&self) -> bool {
        self.auth == AUTH_CLAUDE_PLAN
    }

    /// Les cinq fournisseurs proposés d'office. OpenCode Go est le seul au
    /// format « automatique » : c'est le catalogue qui dit quel modèle parle
    /// quel dialecte (piège 78). Les autres ont un format fixe, vérifié par
    /// leur documentation (API compatible OpenAI pour les trois du milieu,
    /// Messages pour Claude).
    pub fn presets() -> Vec<ProviderConfig> {
        let preset = |id: &str, label: &str, base_url: &str, protocol: Option<&str>, x_api_key: bool, session_header: bool| ProviderConfig {
            id: id.into(),
            label: label.into(),
            base_url: base_url.into(),
            api_key: String::new(),
            protocol: protocol.map(String::from),
            x_api_key,
            auth: AUTH_API_KEY.into(),
            session_header,
            enabled: true,
            builtin: true,
        };
        vec![
            preset(PROVIDER_OPENCODE, "OpenCode Go", "https://opencode.ai/zen/go/v1", None, false, true),
            preset(PROVIDER_OPENROUTER, "OpenRouter", "https://openrouter.ai/api/v1", Some("chat"), false, false),
            preset(PROVIDER_DEEPSEEK, "DeepSeek", "https://api.deepseek.com/v1", Some("chat"), false, false),
            preset(PROVIDER_ALIBABA, "Alibaba (Qwen)", "https://dashscope.aliyuncs.com/compatible-mode/v1", Some("chat"), false, false),
            preset(PROVIDER_ANTHROPIC, "Anthropic (Claude)", "https://api.anthropic.com/v1", Some("messages"), true, false),
        ]
    }
}

/// Message du serveur : « Third-party apps now draw from extra usage, not
/// plan limits. Ask your workspace admin to add more and keep going. » Mesuré
/// le 9 octobre (`--test claude_plan`, bissection des tailles) : le plan
/// accepte les petites requêtes (les 21 621 jetons d'un gros prompt passent),
/// mais refuse dès que la déclaration d'outils devient substantielle (~3 000
/// jetons d'outils réels = 400, 12 outils minuscules = OK). C'est la grille
/// de facturation d'Anthropic, pas une erreur de Jimmy : à traduire en conseil
/// actionnable. (piège 90) — résolu depuis par la signature Claude Code
/// (`claude_plan::inject_billing_block`), ce message ne revient qu'en cas de
/// durcissement serveur.
pub const CLAUDE_PLAN_EXTRA_USAGE_MSG: &str =
    "HTTP 400 — Anthropic ne facture plus les requêtes d'agent (avec outils) d'une app tierce sur l'abonnement : elles puisent dans le solde « extra usage », non activé sur ce compte. Trois issues : activer « extra usage » dans les réglages de Claude (facturation à l'usage), saisir une clé API chez Anthropic, ou laisser Jimy sur OpenCode Go.";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmSettings {
    /// Identifiant complet du modèle chez OpenCode Go, ex. `space-bunny-free`.
    pub model: String,
    /// @deprecated : source de vérité = `providers[].base_url`. Conservé pour
    /// lire les anciennes configurations (la migration le recopie dans le
    /// fournisseur OpenCode Go).
    pub base_url: String,
    /// Fournisseur du modèle principal (identifiant dans `providers`).
    #[serde(default = "default_opencode_id")]
    pub provider: String,
    /// Fournisseur du modèle vocal. Vide = celui du modèle principal.
    #[serde(default)]
    pub voice_provider: String,
    /// Fournisseurs configurés (les cinq intégrés + ceux ajoutés).
    #[serde(default)]
    pub providers: Vec<ProviderConfig>,
    /// Température ; `None` = réglage par défaut du fournisseur.
    pub temperature: Option<f32>,
    pub max_tokens: u32,
    /// Modèle utilisé pour les échanges vocaux (vide = le modèle principal).
    /// Utile pour un modèle plus rapide à voix haute, plus puissant à l'écrit.
    #[serde(default)]
    pub voice_model: String,
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
            provider: PROVIDER_OPENCODE.into(),
            voice_provider: String::new(),
            providers: ProviderConfig::presets(),
            temperature: None,
            // 4096 coupait l'écriture d'un fichier : les jetons de raisonnement
            // d'un modèle comme MiMo comptent dans ce budget (mesuré le 4
            // octobre : 4 096 jetons pour 4 473 caractères visibles).
            max_tokens: DEFAULT_MAX_TOKENS,
            voice_model: String::new(),
            // 12 étapes ne suffisaient pas à une tâche de code réelle (lire les
            // fichiers utiles, écrire, tester) : mesuré le 4 octobre, 18 outils
            // en 12 étapes et toujours rien d'écrit.
            max_iterations: DEFAULT_MAX_ITERATIONS,
            session_prefix: "jimmy".into(),
        }
    }
}

impl LlmSettings {
    /// Migration et normalisation, à chaque chargement de configuration.
    ///
    /// Une configuration d'avant le multi-fournisseur n'a pas de liste
    /// `providers` : on enregistre les cinq intégrés, et l'ancien `base_url`
    /// devient l'URL du fournisseur OpenCode Go (l'utilisateur avait peut-être
    /// pointé vers un proxy). La liste devient la source de vérité ; `base_url`
    /// n'est plus lu ailleurs.
    pub fn migrate_providers(&mut self) {
        if self.provider.trim().is_empty() {
            self.provider = PROVIDER_OPENCODE.into();
        }
        if self.providers.is_empty() {
            self.providers = ProviderConfig::presets();
            if !self.base_url.trim().is_empty() {
                if let Some(opencode) = self.providers.iter_mut().find(|p| p.id == PROVIDER_OPENCODE) {
                    opencode.base_url = self.base_url.clone();
                }
            }
            return;
        }
        // Liste déjà présente : on ajoute les intégrés qui manqueraient (un
        // preset introduit dans une version future, par exemple).
        for preset in ProviderConfig::presets() {
            if !self.providers.iter().any(|p| p.id == preset.id) {
                self.providers.push(preset);
            }
        }
    }

    pub fn provider(&self, id: &str) -> Option<&ProviderConfig> {
        self.providers.iter().find(|p| p.id == id)
    }

    /// Le fournisseur actif du modèle principal. Un identifiant inconnu ou
    /// désactivé (config éditée à la main) retombe sur le premier fournisseur
    /// activé, jamais sur une panne.
    pub fn active_provider(&self) -> Option<&ProviderConfig> {
        self.provider(&self.provider)
            .filter(|p| p.enabled)
            .or_else(|| self.providers.iter().find(|p| p.enabled))
    }

    /// Le fournisseur actif du modèle vocal : `voice_provider` s'il est
    /// renseigné et valide, sinon celui du principal.
    pub fn voice_active_provider(&self) -> Option<&ProviderConfig> {
        if self.voice_provider.trim().is_empty() {
            return self.active_provider();
        }
        self.provider(&self.voice_provider)
            .filter(|p| p.enabled)
            .or_else(|| self.active_provider())
    }

    /// Clé effective d'un fournisseur : celle saisie dans l'interface, sinon
    /// repli sur la variable d'environnement du même service (jamais l'inverse :
    /// le choix de l'utilisateur gagne, piège 62).
    pub fn resolve_key(&self, provider: &ProviderConfig, secrets: &Secrets) -> String {
        if !provider.api_key.trim().is_empty() {
            return provider.api_key.clone();
        }
        match provider.id.as_str() {
            PROVIDER_OPENCODE => secrets.opencode_api_key.clone(),
            PROVIDER_OPENROUTER => secrets.openrouter_api_key.clone(),
            _ => String::new(),
        }
    }

    /// Vrai si la clé effective vient de l'environnement et non de la saisie.
    pub fn key_from_env(&self, provider: &ProviderConfig, secrets: &Secrets) -> bool {
        provider.api_key.trim().is_empty() && !self.resolve_key(provider, secrets).is_empty()
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
    /// Voix ajoutées depuis le catalogue Fish Audio (bibliothèque de voix),
    /// en plus des voix prédéfinies de `TtsVoice::PRESETS`.
    #[serde(default)]
    pub library: Vec<VoiceInfo>,
}

/// Une voix Fish Audio telle que la bibliothèque l'affiche et la garde.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VoiceInfo {
    /// Identifiant Fish Audio (`reference_id`, 32 caractères hexadécimaux).
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub languages: Vec<String>,
    /// Nombre d'utilisations sur Fish Audio : un repère de qualité.
    #[serde(default)]
    pub uses: u64,
}

impl Default for TtsSettings {
    fn default() -> Self {
        TtsSettings {
            enabled: true,
            provider: "fish-audio".into(),
            model: "fish-audio/s2.1-pro-free:free".into(),
            // « Le narrateur » (Fish Audio), voix par défaut depuis le 4 octobre
            // 2026 : voir `TtsVoice::NARRATEUR`.
            voice: "4f2a0684dd0247dda68f339738c780e6".into(),
            chars_per_minute: 1000.0,
            cues: true,
            library: Vec::new(),
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
    /// Contexte audio du serveur de commande (voir `Stt::ensure_server`).
    /// 640 = 12,8 s, juste au-dessus de la phrase la plus longue acceptée
    /// (12 s + marges) : jamais tronquée. 0 = contexte complet (plus lent).
    #[serde(default = "default_command_audio_ctx")]
    pub command_audio_ctx: u32,
}

fn default_command_audio_ctx() -> u32 {
    640
}

fn default_follow_up_ms() -> u64 {
    8000
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
            // Nom de l'agent : « Jimy ». La détection compare la prononciation
            // (`voice::phonetic`) : « Jimmy », forme que whisper écrit le plus
            // souvent, déclenche aussi.
            wake_word: "jimy".into(),
            threads: 0,
            command_model: default_command_model(),
            command_port: default_command_port(),
            command_audio_ctx: default_command_audio_ctx(),
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
    /// Conversation continue : après une réponse, Jimmy écoute encore ce
    /// nombre de millisecondes **sans** qu'il faille redire son nom
    /// (0 = désactivée). Sans elle, chaque phrase demandait « Jimmy » puis
    /// « Oui ? » : ce n'était pas une conversation.
    #[serde(default = "default_follow_up_ms")]
    pub follow_up_ms: u64,
    /// Garde chaque extrait audio transcrit dans `data/audio/debug/` (les 40
    /// derniers, en local) : indispensable pour comprendre pourquoi une
    /// phrase est mal transcrite.
    #[serde(default)]
    pub debug_audio: bool,
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
            end_of_speech_ms: 700,
            follow_up_ms: default_follow_up_ms(),
            debug_audio: false,
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
    /// Racine du vault Obsidian : la mémoire persistante de Jimmy y est
    /// écrite en notes Markdown. Vide ou inexistant = vault inutilisé.
    #[serde(default = "default_vault_path")]
    pub vault_path: String,
    /// Active la connexion au vault (indépendant de la mémoire SQLite).
    #[serde(default = "default_true")]
    pub vault_enabled: bool,
    /// Sous-dossier du vault où Jimmy écrit ses souvenirs.
    #[serde(default = "default_vault_folder")]
    pub vault_folder: String,
    /// Longueur de demande (caractères) au-delà de laquelle Jimmy consulte
    /// spontanément le vault. Une demande plus courte, sans marqueur de
    /// contexte, se contente de la mémoire locale (0 = jamais automatiquement).
    #[serde(default = "default_vault_min_request_chars")]
    pub vault_min_request_chars: u32,
}

fn default_embedding_url() -> String {
    "http://localhost:1234/v1".into()
}

fn default_embedding_model() -> String {
    "text-embedding-paraphrase-multilingual-minilm-l12-v2.gguf".into()
}

fn default_vault_path() -> String {
    r"C:\Obsidian\Jimmy".into()
}

fn default_vault_folder() -> String {
    "0_Inbox/Jimmy".into()
}

fn default_vault_min_request_chars() -> u32 {
    180
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
            vault_path: default_vault_path(),
            vault_enabled: true,
            vault_folder: default_vault_folder(),
            vault_min_request_chars: default_vault_min_request_chars(),
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
pub struct SkillsSettings {
    /// Capture de compétence (mécanisme « Voyager ») : après une trajectoire
    /// réussie qui a mobilisé plusieurs outils, Jimmy condense la démarche en
    /// un skill réutilisable. Le vault note ; ici, la mémoire devient
    /// exécutable.
    #[serde(default = "default_true")]
    pub auto_capture: bool,
}

impl Default for SkillsSettings {
    fn default() -> Self {
        SkillsSettings { auto_capture: true }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrowthSettings {
    /// Intervalle de la revue périodique, en jours (0 = jamais). La revue
    /// relit les leçons récentes et propose des améliorations dans une
    /// session « Revue » du Chat ; rien n'est appliqué sans l'utilisateur.
    #[serde(default = "default_review_days")]
    pub review_days: u32,
}

fn default_review_days() -> u32 {
    7
}

impl Default for GrowthSettings {
    fn default() -> Self {
        GrowthSettings { review_days: default_review_days() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub llm: LlmSettings,
    pub tts: TtsSettings,
    pub stt: SttSettings,
    pub voice: VoiceSettings,
    pub avatar: AvatarSettings,
    pub memory: MemorySettings,
    #[serde(default)]
    pub skills: SkillsSettings,
    #[serde(default)]
    pub growth: GrowthSettings,
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
            skills: SkillsSettings::default(),
            growth: GrowthSettings::default(),
            ui: UiSettings::default(),
            startup: StartupMode::default(),
            mcp_servers: Vec::new(),
            // Dossier de projets par défaut : %USERPROFILE%\Projet.
            workspace: std::env::var("USERPROFILE")
                .map(|home| format!("{home}\\Projet"))
                .unwrap_or_else(|_| "C:\\Projet".into()),
        }
    }
}

/// Étapes (appels au modèle) par demande écrite. Voir aussi
/// `VOICE_MAX_ITERATIONS` pour la voix.
pub const DEFAULT_MAX_ITERATIONS: u32 = 25;

/// Longueur maximale d'une réponse du modèle (jetons, raisonnement compris).
pub const DEFAULT_MAX_TOKENS: u32 = 16_384;

impl Settings {
    pub fn load(paths: &Paths) -> Self {
        let path = paths.config_file();
        let env = |key: &str| std::env::var(key).ok().filter(|v| !v.is_empty());
        if !path.is_file() {
            // Premier lancement : l'environnement donne les valeurs de départ.
            let mut s = Settings::default();
            s.apply_env_with(env, true);
            s.llm.migrate_providers();
            return s;
        }
        match std::fs::read_to_string(&path) {
            Ok(raw) => match serde_json::from_str::<Settings>(&raw) {
                Ok(mut s) => {
                    // Config existante : les choix de l'utilisateur gagnent.
                    s.apply_env_with(env, false);
                    s.llm.migrate_providers();
                    // 900 ms était l'ancien défaut de fin de phrase : mesuré
                    // trop long (Jimmy semblait ne rien faire après qu'on a
                    // fini de parler). Migré vers 700 ms.
                    // 12 était l'ancien défaut, trop court pour une vraie tâche :
                    // migré vers le nouveau (un réglage choisi à la main, autre
                    // que 12, est conservé).
                    if s.llm.max_iterations == 12 {
                        s.llm.max_iterations = DEFAULT_MAX_ITERATIONS;
                    }
                    // Même logique pour l'ancien défaut de longueur de réponse.
                    if s.llm.max_tokens == 4096 {
                        s.llm.max_tokens = DEFAULT_MAX_TOKENS;
                    }
                    if s.voice.end_of_speech_ms == 900 {
                        s.voice.end_of_speech_ms = 700;
                    }
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

    /// L'environnement peut fixer quelques valeurs (utile en dev et pour
    /// l'installateur). `lookup` lit une variable (non vide).
    ///
    /// * **Choix d'expérience** (modèle, modèle et voix de synthèse, langue) :
    ///   seulement au premier lancement (`first_run`), comme valeur de départ.
    ///   Avant, `JIMMY_LLM_MODEL` du `.env` écrasait le modèle choisi dans les
    ///   Paramètres à chaque lancement, puis le premier enregistrement le
    ///   réécrivait dans la config : MiMo choisi, `space-bunny-free` utilisé
    ///   (piège 62).
    /// * **Réglages techniques** (ports, Godot, vault) : toujours.
    fn apply_env_with(&mut self, lookup: impl Fn(&str) -> Option<String>, first_run: bool) {
        if first_run {
            if let Some(v) = lookup("JIMMY_LLM_MODEL") {
                self.llm.model = v;
            }
            if let Some(v) = lookup("JIMMY_TTS_MODEL") {
                self.tts.model = v;
            }
            if let Some(v) = lookup("JIMMY_TTS_VOICE") {
                self.tts.voice = v;
            }
            if let Some(v) = lookup("JIMMY_STT_LANGUAGE") {
                self.stt.language = v;
            }
        }
        if let Some(p) = lookup("JIMMY_GODOT_PORT").and_then(|v| v.parse().ok()) {
            self.avatar.port = p;
        }
        if let Some(p) = lookup("JIMMY_BRIDGE_PORT").and_then(|v| v.parse().ok()) {
            self.avatar.bridge_port = p;
        }
        if let Some(v) = lookup("JIMMY_GODOT_EXE") {
            self.avatar.godot_exe = v;
        }
        if let Some(v) = lookup("JIMMY_VAULT_PATH") {
            self.memory.vault_path = v;
        }
    }
}

/// Secrets lus depuis l'environnement et le fichier `.env`.
#[derive(Debug, Clone, Default)]
pub struct Secrets {
    pub opencode_api_key: String,
    pub openrouter_api_key: String,
}

impl Secrets {
    pub fn from_env() -> Self {
        Secrets {
            opencode_api_key: env("OPENCODE_API_KEY"),
            openrouter_api_key: env("OPENROUTER_API_KEY"),
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

#[cfg(test)]
mod env_tests {
    use super::*;

    fn env(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |key| pairs.iter().find(|(k, _)| *k == key).map(|(_, v)| v.to_string())
    }

    const ENV: &[(&str, &str)] = &[
        ("JIMMY_LLM_MODEL", "space-bunny-free"),
        ("JIMMY_TTS_VOICE", "voix-du-env"),
        ("JIMMY_GODOT_PORT", "9999"),
    ];

    /// Cas réel du 4 octobre : MiMo choisi dans les Paramètres, mais le `.env`
    /// (`JIMMY_LLM_MODEL=space-bunny-free`) l'écrasait à chaque lancement.
    #[test]
    fn le_modele_choisi_par_l_utilisateur_gagne_sur_le_env() {
        let mut s = Settings::default();
        s.llm.model = "mimo-v2.6-flash".into();
        s.tts.voice = "voix-choisie".into();
        s.apply_env_with(env(ENV), false);
        assert_eq!(s.llm.model, "mimo-v2.6-flash");
        assert_eq!(s.tts.voice, "voix-choisie");
        // Les réglages techniques restent surchargeables.
        assert_eq!(s.avatar.port, 9999);
    }

    /// L'ancien défaut (12 étapes) est migré ; un réglage choisi à la main reste.
    #[test]
    fn l_ancien_budget_d_etapes_est_migre() {
        let dir = std::env::temp_dir().join(format!("jimmy-cfg-iter-{}", std::process::id()));
        let paths = Paths { data: dir.clone(), app: dir.clone(), dev: false };
        std::fs::create_dir_all(&dir).unwrap();
        for (avant, apres) in [(12, DEFAULT_MAX_ITERATIONS), (40, 40)] {
            let mut s = Settings::default();
            s.llm.max_iterations = avant;
            s.save(&paths).unwrap();
            assert_eq!(Settings::load(&paths).llm.max_iterations, apres);
        }
        for (avant, apres) in [(4096, DEFAULT_MAX_TOKENS), (8000, 8000)] {
            let mut s = Settings::default();
            s.llm.max_tokens = avant;
            s.save(&paths).unwrap();
            assert_eq!(Settings::load(&paths).llm.max_tokens, apres);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn au_premier_lancement_le_env_donne_les_valeurs_de_depart() {
        let mut s = Settings::default();
        s.apply_env_with(env(&[("JIMMY_LLM_MODEL", "modele-de-depart")]), true);
        assert_eq!(s.llm.model, "modele-de-depart");
    }

    // ── Multi-fournisseurs (chantier d'octobre 2026) ─────────────────────────

    /// Une config d'avant le multi-fournisseur n'a pas de liste `providers` :
    /// le chargement doit la créer avec les cinq intégrés et reprendre l'ancien
    /// `base_url` (le pointeur d'un proxy ne doit pas se perdre).
    #[test]
    fn une_ancienne_config_recoit_les_cinq_fournisseurs_integres() {
        let dir = std::env::temp_dir().join(format!("jimmy-cfg-prov-{}", std::process::id()));
        let paths = Paths { data: dir.clone(), app: dir.clone(), dev: false };
        std::fs::create_dir_all(&dir).unwrap();
        // Une config « d'avant » : pas de liste providers, un base_url hérité.
        let mut avant = Settings::default();
        avant.llm.providers.clear();
        avant.llm.base_url = "https://proxy.perso/v1".into();
        avant.llm.provider = String::new();
        avant.save(&paths).unwrap();
        let s = Settings::load(&paths);
        assert_eq!(s.llm.providers.len(), 5);
        assert_eq!(s.llm.provider, PROVIDER_OPENCODE);
        let opencode = s.llm.provider(PROVIDER_OPENCODE).unwrap();
        assert_eq!(opencode.base_url, "https://proxy.perso/v1");
        assert!(opencode.session_header);
        let anthropic = s.llm.provider(PROVIDER_ANTHROPIC).unwrap();
        assert_eq!(anthropic.protocol.as_deref(), Some("messages"));
        assert!(anthropic.x_api_key);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// La liste `providers` est la source de vérité : un enregistrement puis
    /// rechargement doit garder les fournisseurs ajoutés à la main.
    #[test]
    fn un_fournisseur_personnalise_survit_a_l_enregistrement() {
        let dir = std::env::temp_dir().join(format!("jimmy-cfg-prov2-{}", std::process::id()));
        let paths = Paths { data: dir.clone(), app: dir.clone(), dev: false };
        std::fs::create_dir_all(&dir).unwrap();
        let mut s = Settings::default();
        s.llm.providers.push(ProviderConfig {
            id: "custom-1".into(),
            label: "LM Studio".into(),
            base_url: "http://127.0.0.1:1234/v1".into(),
            api_key: "cle-test".into(),
            protocol: Some("chat".into()),
            x_api_key: false,
            auth: AUTH_API_KEY.into(),
            session_header: false,
            enabled: true,
            builtin: false,
        });
        s.llm.provider = "custom-1".into();
        s.save(&paths).unwrap();
        let reloaded = Settings::load(&paths);
        assert_eq!(reloaded.llm.provider, "custom-1");
        assert_eq!(reloaded.llm.providers.len(), 6);
        assert_eq!(reloaded.llm.provider("custom-1").unwrap().api_key, "cle-test");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// La clé de l'environnement ne sert que de repli : une clé saisie dans
    /// l'interface gagne, et l'environnement ne doit jamais l'écraser (piège 62).
    #[test]
    fn la_cle_saisie_gagne_sur_la_cle_d_environnement() {
        let s = Settings::default();
        let secrets = Secrets {
            opencode_api_key: "cle-du-env".into(),
            openrouter_api_key: String::new(),
        };
        let opencode = s.llm.provider(PROVIDER_OPENCODE).unwrap().clone();
        // Rien saisi : repli sur l'environnement.
        assert_eq!(s.llm.resolve_key(&opencode, &secrets), "cle-du-env");
        assert!(s.llm.key_from_env(&opencode, &secrets));
        // Saisie dans l'interface : elle gagne.
        let mut avec_saisie = opencode.clone();
        avec_saisie.api_key = "cle-saisie".into();
        assert_eq!(s.llm.resolve_key(&avec_saisie, &secrets), "cle-saisie");
        assert!(!s.llm.key_from_env(&avec_saisie, &secrets));
        // Un fournisseur sans variable d'environnement (DeepSeek) n'a que sa clé saisie.
        let deepseek = s.llm.provider(PROVIDER_DEEPSEEK).unwrap().clone();
        assert!(s.llm.resolve_key(&deepseek, &secrets).is_empty());
    }

    /// Un fournisseur en mode abonnement garde sa méthode d'accès à travers
    /// l'enregistrement : le choix « plan Claude » est un réglage durable.
    #[test]
    fn le_mode_abonnement_survit_a_l_enregistrement() {
        let dir = std::env::temp_dir().join(format!("jimmy-cfg-auth-{}", std::process::id()));
        let paths = Paths { data: dir.clone(), app: dir.clone(), dev: false };
        std::fs::create_dir_all(&dir).unwrap();
        let mut s = Settings::default();
        if let Some(p) = s.llm.providers.iter_mut().find(|p| p.id == PROVIDER_ANTHROPIC) {
            p.auth = AUTH_CLAUDE_PLAN.into();
        }
        s.save(&paths).unwrap();
        let reloaded = Settings::load(&paths);
        let anthropic = reloaded.llm.provider(PROVIDER_ANTHROPIC).unwrap();
        assert_eq!(anthropic.auth, AUTH_CLAUDE_PLAN);
        // Les autres fournisseurs restent en clé API.
        assert_eq!(reloaded.llm.provider(PROVIDER_OPENCODE).unwrap().auth, AUTH_API_KEY);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Un identifiant de fournisseur inconnu (config éditée à la main, ou
    /// fournisseur désactivé pendant que le modèle vocal s'en servait) ne doit
    /// jamais laisser Jimmy sans fournisseur : repli sur le premier activé.
    #[test]
    fn un_identifiant_de_fournisseur_inconnu_ne_laisse_jamais_sans_fournisseur() {
        let mut s = Settings::default();
        s.llm.provider = "n-existe-pas".into();
        assert_eq!(s.llm.active_provider().unwrap().id, PROVIDER_OPENCODE);
        // Désactivé : on retombe aussi.
        s.llm.provider = PROVIDER_ANTHROPIC.into();
        for p in &mut s.llm.providers {
            if p.id == PROVIDER_ANTHROPIC {
                p.enabled = false;
            }
        }
        assert_eq!(s.llm.active_provider().unwrap().id, PROVIDER_OPENCODE);
        // Le vocal suit son propre fournisseur, sinon celui du principal.
        s.llm.voice_provider = PROVIDER_DEEPSEEK.into();
        assert_eq!(s.llm.voice_active_provider().unwrap().id, PROVIDER_DEEPSEEK);
        s.llm.voice_provider = String::new();
        assert_eq!(s.llm.voice_active_provider().unwrap().id, PROVIDER_OPENCODE);
    }
}
