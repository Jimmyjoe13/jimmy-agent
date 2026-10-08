//! Client LLM multi-fournisseurs.
//!
//! Jimmy parle à plusieurs fournisseurs de modèles : OpenCode Go (le compte
//! par défaut), OpenRouter, DeepSeek, Alibaba (Qwen), Anthropic (Claude), et
//! n'importe quel fournisseur compatible ajouté à la main. Chaque fournisseur
//! a son URL de base, sa clé, et le format d'API qu'il attend
//! (`/chat/completions`, `/responses`, `/messages` — voir [`super::protocol`]).
//!
//! Le routing est fait par **modèle** : l'appelant ne choisit qu'un
//! identifiant ; [`LlmClient`] retrouve le fournisseur qui le sert (principal
//! ou vocal, selon la configuration). [`LlmClient::update_providers`] recueille
//! cette carte à chaque enregistrement des paramètres.
//!
//! Propres à OpenCode Go, donc réservés au fournisseur marqué
//! `session_header` :
//!
//! * l'en-tête `x-opencode-session` est **obligatoire** ; sans lui l'API répond
//!   `400 MissingSessionID` car elle ne peut pas router la requête ;
//! * le catalogue des modèles (nom, contexte, prix, capacités, format) vit à
//!   part : récupéré une fois, gardé 1 h en mémoire.
//!
//! Anthropic veut la clé dans `x-api-key` (et `anthropic-version`), les autres
//! en `Authorization: Bearer` : c'est le drapeau `x_api_key` du fournisseur.

use std::collections::HashMap;
use std::time::Duration;

use super::protocol::{Protocol, StreamAccum};
use crate::config::{LlmSettings, ProviderConfig, Secrets, AUTH_CLAUDE_PLAN, PROVIDER_OPENCODE};
use crate::core::types::{Message, ToolCall, ToolSpec};
use crate::error::{Error, Result};

#[derive(Debug, Clone, serde::Serialize)]
pub struct ModelInfo {
    pub id: String,
    /// Identifiant complet `fournisseur/modèle` (compatibilité, affichage).
    pub full_id: String,
    /// Fournisseur qui sert ce modèle (pour relancer les tests sur le bon).
    pub provider: String,
    pub name: String,
    pub description: String,
    pub family: String,
    /// Fenêtre de contexte, en jetons (0 = inconnue).
    pub context: u64,
    pub free: bool,
    pub reasoning: bool,
    /// `None` : modèle absent du catalogue, capacité inconnue.
    pub tool_call: Option<bool>,
    pub vision: bool,
    /// Prix en dollars par million de jetons.
    pub cost_input: f64,
    pub cost_output: f64,
    pub released: String,
    /// Présent dans le catalogue public (sinon : nom et capacités inconnus).
    pub in_catalog: bool,
}

/// Résultat du test fonctionnel d'un modèle (voir [`LlmClient::test_model`]).
#[derive(Debug, Clone, serde::Serialize)]
pub struct ModelTest {
    pub model: String,
    /// Répond à une requête simple.
    pub ok: bool,
    /// Accepte une requête avec outils (ce dont Jimmy a besoin pour agir).
    pub tools: bool,
    pub latency_ms: u64,
    pub tools_latency_ms: u64,
    pub reply: String,
    /// Vide si tout va bien.
    pub error: String,
    pub tested_at: String,
}

#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

/// Modèles Claude connus pour le repli de l'abonnement (le catalogue public
/// ne les décrit pas), du plus courant au plus ancien.
const CLAUDE_PLAN_MODELS: &[&str] = &[
    "claude-sonnet-4-5",
    "claude-haiku-4-5",
    "claude-opus-4-1",
    "claude-sonnet-4",
];

#[derive(Debug, Clone)]
pub struct LlmReply {
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
    /// Réponse coupée par la limite de longueur (`finish_reason = "length"`).
    pub truncated: bool,
    pub usage: Usage,
}

/// Un fournisseur prêt à appeler : réglages + clé effective déjà résolue.
#[derive(Debug, Clone)]
struct ProviderRuntime {
    id: String,
    label: String,
    base_url: String,
    api_key: String,
    /// Format imposé par la configuration (`None` = catalogue puis essais).
    protocol: Option<Protocol>,
    x_api_key: bool,
    session_header: bool,
    /// Abonnement Claude : le jeton vient de la session Claude Code
    /// (`claude_plan`), pas de `api_key`.
    claude_plan: bool,
}

impl ProviderRuntime {
    fn from_config(config: &ProviderConfig, api_key: &str) -> Self {
        ProviderRuntime {
            id: config.id.clone(),
            label: config.label.clone(),
            base_url: config.base_url.trim().trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            protocol: config.protocol.as_deref().and_then(Protocol::from_label),
            x_api_key: config.x_api_key,
            session_header: config.session_header,
            claude_plan: config.auth == AUTH_CLAUDE_PLAN,
        }
    }
}

pub struct LlmClient {
    http: reqwest::Client,
    session_id: String,
    /// Catalogue public OpenCode Go (plusieurs Mo) gardé 1 h en mémoire.
    catalog: std::sync::Mutex<Option<(std::time::Instant, serde_json::Value)>>,
    /// Format d'API retenu pour chaque couple fournisseur/modèle (catalogue,
    /// configuration, ou essai réussi).
    protocols: std::sync::Mutex<HashMap<(String, String), Protocol>>,
    /// Fournisseurs activés, par identifiant.
    providers: std::sync::Mutex<HashMap<String, ProviderRuntime>>,
    /// Quel fournisseur sert quel modèle (principal et vocal).
    routes: std::sync::Mutex<HashMap<String, String>>,
    /// Fournisseur par défaut : celui du modèle principal.
    main_provider: std::sync::Mutex<String>,
    /// Session de l'abonnement Claude (jetons Claude Code), créée dès qu'un
    /// fournisseur est en mode « claude-plan ».
    plan: std::sync::Mutex<Option<std::sync::Arc<super::claude_plan::PlanSession>>>,
}

impl LlmClient {
    /// Client mono-fournisseur OpenCode Go — le comportement d'avant le
    /// multi-fournisseur. [`update_providers`] prend ensuite le relais.
    pub fn new(base_url: &str, api_key: &str, session_id: &str) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(180))
            .user_agent("jimmy/0.1 (desktop agent)")
            .build()?;
        let runtime = ProviderRuntime {
            id: PROVIDER_OPENCODE.into(),
            label: "OpenCode Go".into(),
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            protocol: None,
            x_api_key: false,
            session_header: true,
            claude_plan: false,
        };
        let mut providers = HashMap::new();
        providers.insert(runtime.id.clone(), runtime);
        Ok(LlmClient {
            http,
            session_id: session_id.to_string(),
            catalog: std::sync::Mutex::new(None),
            protocols: std::sync::Mutex::new(HashMap::new()),
            providers: std::sync::Mutex::new(providers),
            routes: std::sync::Mutex::new(HashMap::new()),
            main_provider: std::sync::Mutex::new(PROVIDER_OPENCODE.into()),
            plan: std::sync::Mutex::new(None),
        })
    }

    /// Applique la configuration : fournisseurs activés (clés effectives
    /// résolues avec le repli d'environnement) et routing modèle → fournisseur
    /// (principal, vocal). Appelé au lancement et à chaque enregistrement.
    pub fn update_providers(&self, settings: &LlmSettings, secrets: &Secrets) {
        let mut providers = HashMap::new();
        for config in settings.providers.iter().filter(|p| p.enabled) {
            let key = settings.resolve_key(config, secrets);
            providers.insert(config.id.clone(), ProviderRuntime::from_config(config, &key));
        }
        let main = settings
            .active_provider()
            .map(|p| p.id.clone())
            .filter(|id| providers.contains_key(id))
            .unwrap_or_else(|| PROVIDER_OPENCODE.to_string());
        let voice = settings
            .voice_active_provider()
            .map(|p| p.id.clone())
            .filter(|id| providers.contains_key(id))
            .unwrap_or_else(|| main.clone());

        let mut routes = HashMap::new();
        if !settings.model.trim().is_empty() {
            routes.insert(settings.model.trim().to_string(), main.clone());
        }
        let voice_model = settings.voice_model.trim();
        if !voice_model.is_empty() && voice_model != settings.model.trim() {
            routes.insert(voice_model.to_string(), voice);
        }

        log::info!(
            "[llm] fournisseurs : {} activé(s), principal « {} » chez {main}, routeur {} modèle(s)",
            providers.len(),
            settings.model,
            routes.len(),
        );
        // Un fournisseur en mode abonnement Claude ? La session lit les jetons
        // de Claude Code (réflexe : jamais le jeton dans le journal).
        if providers.values().any(|p| p.claude_plan) {
            if let Some(path) = super::claude_plan::credentials_path() {
                if let Ok(mut slot) = self.plan.lock() {
                    *slot = Some(std::sync::Arc::new(super::claude_plan::PlanSession::new(self.http.clone(), path)));
                }
            }
        } else if let Ok(mut slot) = self.plan.lock() {
            *slot = None;
        }
        if let (Ok(mut guard), Ok(mut routes_guard), Ok(mut main_guard)) =
            (self.providers.lock(), self.routes.lock(), self.main_provider.lock())
        {
            *guard = providers;
            *routes_guard = routes;
            *main_guard = main;
        }
    }

    fn provider_runtime(&self, id: &str) -> Option<ProviderRuntime> {
        self.providers.lock().ok().and_then(|p| p.get(id).cloned())
    }

    fn main_runtime(&self) -> Option<ProviderRuntime> {
        let id = self.main_provider.lock().ok().map(|m| m.clone())?;
        self.provider_runtime(&id)
            .or_else(|| self.providers.lock().ok().and_then(|p| p.values().next().cloned()))
    }

    /// Le fournisseur qui sert ce modèle (routing principal/vocal, sinon
    /// principal).
    fn runtime_for(&self, model: &str) -> ProviderRuntime {
        let route = self
            .routes
            .lock()
            .ok()
            .and_then(|r| r.get(model).cloned())
            .and_then(|id| self.provider_runtime(&id));
        route.or_else(|| self.main_runtime()).unwrap_or_else(|| ProviderRuntime {
            // Jamais de panique ni de modèle sans fournisseur : avec une
            // configuration vide, l'appel échouera sur la clé absente, ce qui
            // est un message compréhensible.
            id: PROVIDER_OPENCODE.into(),
            label: "OpenCode Go".into(),
            base_url: String::new(),
            api_key: String::new(),
            protocol: None,
            x_api_key: false,
            session_header: true,
            claude_plan: false,
        })
    }

    pub fn has_key(&self) -> bool {
        self.main_runtime().is_some_and(|p| self.runtime_ready(&p))
    }

    /// Un fournisseur particulier a-t-il de quoi appeler ? (status, cartes de
    /// l'interface) L'abonnement Claude compte comme une clé : la session
    /// Claude Code est l'credential.
    pub fn provider_has_key(&self, provider_id: &str) -> bool {
        self.provider_runtime(provider_id).is_some_and(|p| self.runtime_ready(&p))
    }

    fn runtime_ready(&self, provider: &ProviderRuntime) -> bool {
        if provider.claude_plan {
            return super::claude_plan::credentials_path()
                .is_some_and(|path| super::claude_plan::read_tokens(&path).is_ok());
        }
        !provider.api_key.trim().is_empty()
    }

    /// En-têtes d'authentification du fournisseur. Anthropic lit la clé dans
    /// `x-api-key` (sinon `401 Missing API key`) ; les autres en `Bearer`.
    /// Le format Messages exige aussi `anthropic-version`.
    fn auth_headers(&self, provider: &ProviderRuntime) -> reqwest::header::HeaderMap {
        let mut headers = reqwest::header::HeaderMap::new();
        if provider.x_api_key {
            if let Ok(value) = reqwest::header::HeaderValue::from_str(&provider.api_key) {
                headers.insert("x-api-key", value);
            }
        } else if let Ok(value) = reqwest::header::HeaderValue::from_str(&format!("Bearer {}", provider.api_key)) {
            headers.insert(reqwest::header::AUTHORIZATION, value);
        }
        if provider.session_header {
            if let Ok(value) = reqwest::header::HeaderValue::from_str(&self.session_id) {
                headers.insert("x-opencode-session", value);
            }
        }
        headers
    }

    /// En-têtes d'un appel dans le format `protocol`.
    fn protocol_headers(&self, provider: &ProviderRuntime, protocol: Protocol) -> reqwest::header::HeaderMap {
        let mut headers = self.auth_headers(provider);
        if protocol == Protocol::Messages {
            // OpenCode Go sert les modèles Messages en `x-api-key` **et** en
            // Bearer selon le routeur : envoyer les deux est sans risque, et
            // Anthropic natif refuse sans `x-api-key`.
            if let Ok(value) = reqwest::header::HeaderValue::from_str(&provider.api_key) {
                headers.insert("x-api-key", value);
            }
            headers.insert("anthropic-version", reqwest::header::HeaderValue::from_static("2023-06-01"));
        }
        headers
    }

    /// En-têtes d'un appel, en résolvant d'abord le jeton d'abonnement Claude
    /// si le fournisseur est en mode « claude-plan » (peut déclencher un
    /// rafraîchissement réseau du jeton Claude Code).
    async fn request_headers(&self, provider: &ProviderRuntime, protocol: Protocol) -> Result<reqwest::header::HeaderMap> {
        if provider.claude_plan {
            let session = self
                .plan
                .lock()
                .ok()
                .and_then(|p| p.clone())
                .ok_or_else(|| Error::provider("Abonnement Claude", "aucune session — ouvre Claude Code (ou reconnecte ton abonnement)"))?;
            let token = session.access_token().await?;
            return Ok(super::claude_plan::oauth_headers(&token));
        }
        Ok(self.protocol_headers(provider, protocol))
    }

    /// Format d'API du modèle : celui imposé par la configuration du
    /// fournisseur, sinon celui du catalogue public (OpenCode Go), sinon Chat.
    /// Mis en cache par couple fournisseur/modèle.
    async fn protocol(&self, provider: &ProviderRuntime, model: &str) -> Protocol {
        let key = (provider.id.clone(), model.to_string());
        if let Some(known) = self.protocols.lock().ok().and_then(|p| p.get(&key).copied()) {
            return known;
        }
        let protocol = if let Some(fixed) = provider.protocol {
            fixed
        } else if provider.id == PROVIDER_OPENCODE {
            let Ok(catalog) = self.catalog_models(false).await else { return Protocol::Chat };
            let npm = catalog.get(model).and_then(|e| e.pointer("/provider/npm")).and_then(|v| v.as_str());
            let from_catalog = Protocol::from_npm(npm);
            log::info!("[llm] « {model} » : format {}", from_catalog.label());
            from_catalog
        } else {
            Protocol::Chat
        };
        self.remember(provider, model, protocol);
        protocol
    }

    fn remember(&self, provider: &ProviderRuntime, model: &str, protocol: Protocol) {
        if let Ok(mut known) = self.protocols.lock() {
            known.insert((provider.id.clone(), model.to_string()), protocol);
        }
    }

    /// Envoie la requête au format du modèle. Si le fournisseur répond
    /// `ModelProtocolUnsupported` (format « automatique » qui se trompe), les
    /// autres formats sont essayés et le bon est retenu. Renvoie le format
    /// utilisé et la réponse HTTP réussie.
    async fn send(
        &self,
        model: &str,
        build: &(dyn Fn(Protocol) -> serde_json::Value + Sync),
        timeout: Option<Duration>,
    ) -> Result<(Protocol, reqwest::Response)> {
        self.send_on(None, model, build, timeout).await
    }

    /// [`send`] avec fournisseur imposé (bibliothèque qui parcourt un
    /// fournisseur autre que celui du modèle actif).
    async fn send_on(
        &self,
        provider: Option<&str>,
        model: &str,
        build: &(dyn Fn(Protocol) -> serde_json::Value + Sync),
        timeout: Option<Duration>,
    ) -> Result<(Protocol, reqwest::Response)> {
        let provider = match provider {
            Some(id) => self.provider_runtime(id).ok_or_else(|| Error::provider(id, "fournisseur absent ou désactivé"))?,
            None => self.runtime_for(model),
        };
        let service = provider.label.clone();
        if !provider.claude_plan && provider.api_key.trim().is_empty() {
            return Err(Error::provider(&service, "clé absente — la saisir dans Paramètres → LLM"));
        }
        if provider.base_url.is_empty() {
            return Err(Error::provider(&service, "URL de base absente"));
        }
        let first = self.protocol(&provider, model).await;
        let order = std::iter::once(first).chain(Protocol::ALL.into_iter().filter(move |p| *p != first));
        let mut last_error = None;
        for protocol in order {
            let mut body = build(protocol);
            // Signature Claude Code sur l'abonnement : le serveur juge la
            // requête « tierce » d'apres le bloc de facturation en tete du
            // systeme et les noms d'outils ; sans eux, premium et gros appels
            // outilles sont renvoyes en 400/429 « extra usage » (piège 90,
            // mesures du 9 octobre : facturation + aliases de noms suffisent).
            if provider.claude_plan && protocol == Protocol::Messages {
                super::claude_plan::inject_billing_block(&mut body);
                super::claude_plan::map_plan_tool_names(&mut body);
            }
            // Diagnostic : `JIMMY_LLM_DUMP=<dossier>` enregistre chaque requête
            // (corps JSON, sans la clé) pour la rejouer à la main et comparer
            // des latences. Sans cette variable, rien n'est écrit.
            if let Some(dir) = std::env::var_os("JIMMY_LLM_DUMP") {
                let n = std::fs::read_dir(&dir).map(|d| d.count()).unwrap_or(0);
                let _ = std::fs::write(std::path::Path::new(&dir).join(format!("requete-{n:02}.json")), body.to_string());
            }
            // L'abonnement Claude peut rafraîchir son jeton ici : les en-têtes
            // s'obtiennent en async.
            let headers = self.request_headers(&provider, protocol).await?;
            let mut request = self
                .http
                .post(format!("{}{}", provider.base_url, protocol.path()))
                .headers(headers)
                .json(&body);
            if let Some(timeout) = timeout {
                request = request.timeout(timeout);
            }
            let response = request.send().await.map_err(|e| {
                let message = match timeout {
                    Some(t) if e.is_timeout() => format!("pas de réponse en {} s", t.as_secs()),
                    _ => e.to_string(),
                };
                Error::provider(&service, message)
            })?;
            let status = response.status();
            if status.is_success() {
                if protocol != first {
                    log::info!("[llm] « {model} » : format {} retenu après essai", protocol.label());
                    self.remember(&provider, model, protocol);
                }
                return Ok((protocol, response));
            }
            let raw = response.text().await.map_err(|e| Error::provider(&service, e.to_string()))?;
            // Grille de facturation Anthropic sur l'abonnement (piège 90) :
            // le message brut (« extra usage ») ne dit rien à l'utilisateur.
            if let Some(hint) = plan_extra_usage(&provider, status.as_u16(), &raw) {
                return Err(Error::provider(&service, hint));
            }
            // Le corps peut contenir un secret d'authentification en théorie :
            // on ne renvoie jamais la clé, seulement le message du serveur.
            let error = Error::provider(&service, format!("HTTP {status} — {}", truncate(&raw, 400)));
            if !raw.contains("ModelProtocolUnsupported") {
                return Err(error);
            }
            last_error = Some(error);
        }
        Err(last_error.unwrap_or_else(|| Error::provider(&service, "aucun format d'API accepté")))
    }

    /// Un appel de complétion, avec outils optionnels.
    pub async fn chat(
        &self,
        model: &str,
        messages: &[Message],
        tools: &[ToolSpec],
        temperature: Option<f32>,
        max_tokens: u32,
    ) -> Result<LlmReply> {
        let build = |p: Protocol| p.body(model, messages, tools, temperature, max_tokens, false);
        let provider = self.runtime_for(model);
        let service = provider.label;
        let (protocol, response) = self.send(model, &build, None).await?;
        let raw = response.text().await.map_err(|e| Error::provider(&service, e.to_string()))?;
        let value: serde_json::Value = serde_json::from_str(&raw)
            .map_err(|e| Error::provider(&service, format!("réponse illisible : {e}")))?;
        let mut reply = protocol.parse(&value)?;
        if provider.claude_plan {
            super::claude_plan::unmap_plan_tool_calls(&mut reply.tool_calls);
        }
        Ok(reply)
    }

    /// Une complétion **en flux** (SSE) : même requête et même [`LlmReply`]
    /// que [`chat`], mais chaque fragment du contenu visible est passé à
    /// `on_delta` au fil de la génération — sans cet affichage en direct, le
    /// chat montre des pointillés pendant toute la génération (appel réel
    /// mesuré à 22 s de silence). Le **raisonnement caché** est compté mais
    /// jamais passé à `on_delta`. Chaque format a ses événements ; les trois
    /// envoient des lignes `data: {…}` (voir [`StreamAccum`]).
    pub async fn chat_stream(
        &self,
        model: &str,
        messages: &[Message],
        tools: &[ToolSpec],
        temperature: Option<f32>,
        max_tokens: u32,
        mut on_delta: impl FnMut(&str) + Send,
    ) -> Result<LlmReply> {
        let build = |p: Protocol| p.body(model, messages, tools, temperature, max_tokens, true);
        let provider = self.runtime_for(model);
        let service = provider.label;
        let (protocol, mut response) = self.send(model, &build, None).await?;

        let mut acc = StreamAccum::new(protocol);
        let started = std::time::Instant::now();
        let mut first_fragment = 0u64;
        // Tampon d'octets : un fragment réseau peut couper une ligne SSE au
        // milieu. Les lignes finissent par \n, qui n'est jamais un octet de
        // continuation UTF-8 : couper sur lui est sans risque d'encodage.
        let mut buffer: Vec<u8> = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| Error::provider(&service, e.to_string()))?
        {
            buffer.extend_from_slice(&chunk);
            while let Some(pos) = buffer.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = buffer.drain(..=pos).collect();
                let line = String::from_utf8_lossy(&line);
                // Les lignes `event: …` (Responses, Messages) répètent le
                // `type` déjà présent dans `data` : seules les données comptent.
                let data = match line.trim().strip_prefix("data:") {
                    Some(data) => data.trim(),
                    None => continue,
                };
                if data == "[DONE]" {
                    continue;
                }
                // Une ligne illisible ne doit jamais tuer le flux : il y a des
                // battements du protocole qui ne sont pas des fragments.
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(data) {
                    // Métrique du streaming : le délai du premier fragment est
                    // ce que l'utilisateur vit à l'écran (22 s de silence avant
                    // ce chantier). Une seule ligne, au premier contenu.
                    acc.ingest(&value, &mut |text| {
                        if first_fragment == 0 {
                            first_fragment = started.elapsed().as_millis() as u64;
                            log::info!("[llm] premier fragment après {first_fragment} ms");
                        }
                        on_delta(text);
                    });
                }
            }
        }
        let mut reply = acc.into_reply()?;
        if provider.claude_plan {
            super::claude_plan::unmap_plan_tool_calls(&mut reply.tool_calls);
        }
        Ok(reply)
    }

    /// Complétion simple, sans outil (mémoire, onboarding, titres).
    pub async fn complete(
        &self,
        model: &str,
        messages: &[Message],
        max_tokens: u32,
        temperature: Option<f32>,
    ) -> Result<String> {
        let reply = self
            .chat(model, messages, &[], temperature, max_tokens)
            .await?;
        Ok(reply.content.trim().to_string())
    }

    /// Modèles utilisables d'un fournisseur, enrichis par le catalogue public
    /// (OpenCode Go uniquement).
    ///
    /// La **liste vient du compte** (`GET {base_url}/models`) : c'est ce que le
    /// fournisseur accepte réellement avec cette clé. Le catalogue public
    /// (`models.opencode.ai/api.json`, organisé par fournisseur) n'ajoute que
    /// le nom, le contexte, le prix et les capacités. L'ancienne version lisait
    /// une clé `models` à la racine du catalogue, qui n'existe pas : la liste
    /// était toujours vide (piège 48).
    pub async fn list_models(&self, provider: Option<&str>, refresh: bool) -> Result<Vec<ModelInfo>> {
        let provider = match provider {
            Some(id) => self.provider_runtime(id).ok_or_else(|| Error::provider(id, "fournisseur absent ou désactivé"))?,
            None => self.main_runtime().ok_or_else(|| Error::Config("aucun fournisseur activé".into()))?,
        };
        let service = provider.label.clone();
        let account = self.account_models(&provider).await;
        let catalog = if provider.id == PROVIDER_OPENCODE { self.catalog_models(refresh).await.ok() } else { None };
        // Abonnement Claude : `/models` peut être refusé (ou vide) alors que
        // `/messages` fonctionne — une liste connue vaut mieux qu'une erreur.
        let fallback: Vec<String> = if provider.claude_plan { CLAUDE_PLAN_MODELS.iter().map(|s| s.to_string()).collect() } else { Vec::new() };
        let ids: Vec<String> = match &account {
            Ok(list) if !list.is_empty() => list.clone(),
            Ok(_) if !fallback.is_empty() => fallback.clone(),
            _ if !fallback.is_empty() => fallback,
            Err(error) => return Err(Error::provider(&service, error.to_string())),
            _ => match &catalog {
                Some(models) => models.keys().cloned().collect(),
                None => return Err(Error::provider(&service, "aucun modèle accessible avec cette clé")),
            },
        };
        let catalog = catalog.unwrap_or_default();

        let mut out: Vec<ModelInfo> = ids
            .into_iter()
            .map(|id| {
                let entry = catalog.get(&id);
                let text = |key: &str| {
                    entry.and_then(|e| e.get(key)).and_then(|v| v.as_str()).unwrap_or("").to_string()
                };
                let flag = |key: &str| entry.and_then(|e| e.get(key)).and_then(|v| v.as_bool());
                let cost = |key: &str| {
                    entry
                        .and_then(|e| e.get("cost"))
                        .and_then(|c| c.get(key))
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0)
                };
                let vision = entry
                    .and_then(|e| e.get("modalities"))
                    .and_then(|m| m.get("input"))
                    .and_then(|i| i.as_array())
                    .is_some_and(|inputs| inputs.iter().any(|v| v.as_str() == Some("image")));
                let name = text("name");
                let (cost_input, cost_output) = (cost("input"), cost("output"));
                ModelInfo {
                    full_id: format!("{}/{}", provider.id, id),
                    provider: provider.id.clone(),
                    name: if name.is_empty() { id.clone() } else { name },
                    description: text("description"),
                    family: text("family"),
                    context: entry
                        .and_then(|e| e.get("limit"))
                        .and_then(|l| l.get("context"))
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0),
                    free: id.contains("free") || (entry.is_some() && cost_input == 0.0 && cost_output == 0.0),
                    reasoning: flag("reasoning").unwrap_or(false),
                    tool_call: flag("tool_call"),
                    vision,
                    cost_input,
                    cost_output,
                    released: text("release_date"),
                    in_catalog: entry.is_some(),
                    id,
                }
            })
            .collect();
        out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        Ok(out)
    }

    /// Identifiants des modèles accessibles avec la clé de ce fournisseur.
    /// Toutes les API visitées (`/models` chez OpenRouter, DeepSeek, Alibaba,
    /// Anthropic et OpenCode Go) répondent `{"data": [{"id": …}]}`. Avec
    /// l'abonnement Claude, ce sont les en-têtes OAuth (Bearer + bêta) qui
    /// déloquent l'accès.
    async fn account_models(&self, provider: &ProviderRuntime) -> Result<Vec<String>> {
        if !provider.claude_plan && provider.api_key.trim().is_empty() {
            return Err(Error::provider(&provider.label, "clé absente — la saisir dans Paramètres → LLM"));
        }
        let headers = self.request_headers(provider, Protocol::Chat).await?;
        let response = self
            .http
            .get(format!("{}/models", provider.base_url))
            .headers(headers)
            .timeout(Duration::from_secs(20))
            .send()
            .await
            .map_err(|e| Error::provider(&provider.label, e.to_string()))?;
        if !response.status().is_success() {
            return Err(Error::provider(&provider.label, format!("HTTP {}", response.status())));
        }
        let value: serde_json::Value = response
            .json()
            .await
            .map_err(|e| Error::provider(&provider.label, format!("liste illisible : {e}")))?;
        Ok(value
            .get("data")
            .and_then(|d| d.as_array())
            .map(|items| items.iter().filter_map(|m| m.get("id").and_then(|i| i.as_str()).map(String::from)).collect())
            .unwrap_or_default())
    }

    /// Le modèle accepte-t-il une image en entrée ? D'après le catalogue
    /// (`modalities.input`), donc OpenCode Go seulement. `None` : catalogue
    /// injoignable, fournisseur sans catalogue, ou modèle absent — l'appelant
    /// tente alors quand même.
    pub async fn accepts_images(&self, model: &str) -> Option<bool> {
        if self.runtime_for(model).id != PROVIDER_OPENCODE {
            return None;
        }
        let models = self.catalog_models(false).await.ok()?;
        let input = models.get(model)?.get("modalities")?.get("input")?.as_array()?.clone();
        Some(input.iter().any(|kind| kind == "image"))
    }

    /// Entrées du fournisseur `opencode-go` dans le catalogue public.
    async fn catalog_models(
        &self,
        refresh: bool,
    ) -> Result<HashMap<String, serde_json::Value>> {
        let cached = if refresh {
            None
        } else {
            self.catalog
                .lock()
                .ok()
                .and_then(|c| c.as_ref().filter(|(at, _)| at.elapsed() < Duration::from_secs(3600)).map(|(_, v)| v.clone()))
        };
        let catalog = match cached {
            Some(value) => value,
            None => {
                let value: serde_json::Value = self
                    .http
                    .get("https://models.opencode.ai/api.json")
                    .timeout(Duration::from_secs(30))
                    .send()
                    .await
                    .map_err(|e| Error::provider("catalogue des modèles", e.to_string()))?
                    .json()
                    .await
                    .map_err(|e| Error::provider("catalogue des modèles", e.to_string()))?;
                // Seul le fournisseur utile est gardé : le fichier complet
                // décrit des centaines de fournisseurs.
                let ours = value.get("opencode-go").cloned().unwrap_or(serde_json::Value::Null);
                if let Ok(mut slot) = self.catalog.lock() {
                    *slot = Some((std::time::Instant::now(), ours.clone()));
                }
                ours
            }
        };
        Ok(catalog
            .get("models")
            .and_then(|m| m.as_object())
            .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default())
    }

    /// Teste un modèle avec **les mêmes conditions que Jimmy** : une requête
    /// simple, puis la même requête avec un outil déclaré (`tool_choice: auto`),
    /// dans le format d'API du modèle. Plusieurs modèles répondent à la première
    /// et refusent la seconde en HTTP 400 : ils seraient inutilisables pour
    /// agir.
    pub async fn test_model(&self, provider: Option<&str>, model: &str) -> ModelTest {
        let ping = ToolSpec {
            name: "ping".into(),
            description: "Outil de test, à ne pas utiliser.".into(),
            parameters: serde_json::json!({ "type": "object", "properties": {} }),
        };
        let (plain, tools) = tokio::join!(
            self.timed_request(provider, model, &[]),
            self.timed_request(provider, model, std::slice::from_ref(&ping))
        );
        let (ok, latency_ms, reply, plain_error) = match plain {
            Ok((ms, reply)) => (true, ms, reply, String::new()),
            Err(error) => (false, 0, String::new(), error),
        };
        let (tools_ok, tools_latency_ms, tools_error) = match tools {
            Ok((ms, _)) => (true, ms, String::new()),
            Err(error) => (false, 0, error),
        };
        ModelTest {
            model: model.to_string(),
            ok,
            tools: tools_ok,
            latency_ms,
            tools_latency_ms,
            reply,
            error: if !ok {
                plain_error
            } else if !tools_ok {
                format!("n'accepte pas les outils : {tools_error}")
            } else {
                String::new()
            },
            tested_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    /// Une requête de test chronométrée (40 s au plus).
    /// `Ok((durée en ms, texte de la réponse))`.
    async fn timed_request(&self, provider: Option<&str>, model: &str, tools: &[ToolSpec]) -> std::result::Result<(u64, String), String> {
        let messages = [Message::user("Réponds uniquement par le mot : ok")];
        let started = std::time::Instant::now();
        let build = |p: Protocol| p.body(model, &messages, tools, None, 128, false);
        let service = self
            .provider_runtime(provider.unwrap_or(""))
            .or_else(|| self.main_runtime())
            .map(|p| p.label)
            .unwrap_or_else(|| "modèle".into());
        let outcome = async {
            let (protocol, response) = self.send_on(provider, model, &build, Some(Duration::from_secs(40))).await?;
            let raw = response.text().await.map_err(|e| Error::provider(&service, e.to_string()))?;
            let value: serde_json::Value = serde_json::from_str(&raw)
                .map_err(|e| Error::provider(&service, format!("réponse illisible : {e}")))?;
            protocol.parse(&value)
        }
        .await;
        match outcome {
            Ok(reply) => Ok((started.elapsed().as_millis() as u64, reply.content.trim().to_string())),
            // Le message du serveur (jamais la clé) : « HTTP 400 — invalid request ».
            Err(Error::Provider { message, .. }) => Err(server_message(&message)),
            Err(other) => Err(truncate(&other.to_string(), 160)),
        }
    }
}

/// Le refus de facturation Anthropic sur abonnement : 400 dont le corps parle
/// de « extra usage ». Fonction pure — testée sans réseau. `Some(message
/// clair)` seulement pour un fournisseur en mode abonnement.
fn plan_extra_usage(provider: &ProviderRuntime, status: u16, raw: &str) -> Option<String> {
    if provider.claude_plan && status == 400 && raw.contains("extra usage") {
        return Some(crate::config::CLAUDE_PLAN_EXTRA_USAGE_MSG.into());
    }
    None
}

/// « HTTP 400 Bad Request — {json} » → « HTTP 400 — message du serveur ».
fn server_message(error: &str) -> String {
    let Some((head, raw)) = error.split_once(" — ") else { return truncate(error, 160) };
    let code = head.split_whitespace().take(2).collect::<Vec<_>>().join(" ");
    let message = serde_json::from_str::<serde_json::Value>(raw)
        .ok()
        .and_then(|v| v.pointer("/error/message").and_then(|m| m.as_str()).map(String::from))
        .unwrap_or_else(|| raw.to_string());
    format!("{code} — {}", truncate(&message, 160))
}

fn truncate(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

#[cfg(test)]
mod routing_tests {
    use super::*;
    use crate::config::{PROVIDER_DEEPSEEK, PROVIDER_OPENCODE};

    fn settings_with(main: &str, model: &str, voice: &str, voice_provider: &str) -> LlmSettings {
        let mut s = LlmSettings::default();
        s.migrate_providers();
        s.provider = main.into();
        s.model = model.into();
        s.voice_model = voice.into();
        s.voice_provider = voice_provider.into();
        s
    }

    fn secrets() -> Secrets {
        Secrets { opencode_api_key: "sk-opencode".into(), openrouter_api_key: String::new() }
    }

    #[test]
    fn le_routeur_attache_chaque_modele_a_son_fournisseur() {
        let client = LlmClient::new("https://x/v1", "k", "s").unwrap();
        client.update_providers(&settings_with(PROVIDER_DEEPSEEK, "deepseek-chat", "glm-4.5-air", PROVIDER_OPENCODE), &secrets());
        // Le principal suit `provider`, le vocal suit `voice_provider`.
        assert_eq!(client.runtime_for("deepseek-chat").id, PROVIDER_DEEPSEEK);
        assert_eq!(client.runtime_for("glm-4.5-air").id, PROVIDER_OPENCODE);
        // La clé du vocal vient du repli d'environnement (OpenCode Go).
        assert_eq!(client.runtime_for("glm-4.5-air").api_key, "sk-opencode");
        // Un modèle inconnu du routeur retombe sur le principal.
        assert_eq!(client.runtime_for("n-importe-quoi").id, PROVIDER_DEEPSEEK);
    }

    #[test]
    fn la_cle_d_un_autre_fournisseur_ne_se_declare_pas_sur_opencode() {
        // Piège à ne pas Repayer : « la même clé partout » envoyerait la clé
        // DeepSeek à OpenCode Go avec `x-opencode-session`.
        let client = LlmClient::new("https://x/v1", "k", "s").unwrap();
        // Principal DeepSeek (sans clé), vocal un modèle OpenCode Go.
        client.update_providers(&settings_with(PROVIDER_DEEPSEEK, "deepseek-chat", "glm-4.5-air", PROVIDER_OPENCODE), &secrets());
        let headers = client.auth_headers(&client.runtime_for("deepseek-chat"));
        assert!(
            headers.get(reqwest::header::AUTHORIZATION).and_then(|v| v.to_str().ok()) == Some("Bearer "),
            "DeepSeek sans clé : le send refusera avant l'appel"
        );
        assert!(headers.get("x-opencode-session").is_none());
        let headers = client.auth_headers(&client.runtime_for("glm-4.5-air"));
        assert!(headers.get("x-opencode-session").is_some());
        assert_eq!(
            headers.get(reqwest::header::AUTHORIZATION).and_then(|v| v.to_str().ok()),
            Some("Bearer sk-opencode")
        );
    }

    #[test]
    fn le_refus_extra_usage_devient_un_message_actionnable() {
        let plan = ProviderRuntime {
            id: "anthropic".into(),
            label: "Anthropic (Claude)".into(),
            base_url: "https://api.anthropic.com/v1".into(),
            api_key: String::new(),
            protocol: Some(Protocol::Messages),
            x_api_key: true,
            session_header: false,
            claude_plan: true,
        };
        let cle = ProviderRuntime { claude_plan: false, ..plan.clone() };
        let corps = r#"{"type":"error","error":{"type":"invalid_request_error","message":"Third-party apps now draw from extra usage, not plan limits."}}"#;
        // Abonnement + 400 « extra usage » → le message long d'Anthropic est
        // remplacé par le conseil actionnable.
        let hint = plan_extra_usage(&plan, 400, corps).expect("abonnement : le 400 doit être traduit");
        assert!(hint.contains("extra usage") && hint.contains("clé API"), "{hint}");
        // Les autres cas passent sans interception.
        assert!(plan_extra_usage(&cle, 400, corps).is_none(), "clé API : pas d'interception");
        assert!(plan_extra_usage(&plan, 500, corps).is_none(), "500 : pas d'interception");
        assert!(plan_extra_usage(&plan, 400, "quota dépassé").is_none(), "sans « extra usage » : pas d'interception");
    }

    #[test]
    fn le_format_impose_par_le_fournisseur_saute_les_essais() {
        let client = LlmClient::new("https://x/v1", "k", "s").unwrap();
        // Claude : format Messages fixé dans le preset, pas de catalogue.
        let provider = ProviderRuntime {
            id: "anthropic".into(),
            label: "Anthropic".into(),
            base_url: "https://api.anthropic.com/v1".into(),
            api_key: "sk-ant".into(),
            protocol: Some(Protocol::Messages),
            x_api_key: true,
            session_header: false,
            claude_plan: false,
        };
        let rt = tokio::runtime::Runtime::new().unwrap();
        assert_eq!(rt.block_on(client.protocol(&provider, "claude-sonnet-4-5")), Protocol::Messages);
        // Le format Messages ajoute anthropic-version, même sans drapeau.
        let headers = client.protocol_headers(&provider, Protocol::Messages);
        assert_eq!(headers.get("anthropic-version").and_then(|v| v.to_str().ok()), Some("2023-06-01"));
        assert_eq!(headers.get("x-api-key").and_then(|v| v.to_str().ok()), Some("sk-ant"));
        assert!(headers.get(reqwest::header::AUTHORIZATION).is_none());
    }
}
