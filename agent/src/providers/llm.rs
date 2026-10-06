//! Client LLM.
//!
//! Jimmy parle à **OpenCode Go**. Selon le modèle, le fournisseur attend l'un
//! de trois formats d'API (`/chat/completions`, `/responses`, `/messages`) :
//! le format est lu dans le catalogue public, et la traduction vit dans
//! [`super::protocol`].
//!
//! Deux détails propre à ce fournisseur :
//!
//! * l'en-tête `x-opencode-session` est **obligatoire** ; sans lui l'API répond
//!   `400 MissingSessionID` car elle ne peut pas router la requête ;
//! * le catalogue des modèles vit à part : il est récupéré une fois au
//!   démarrage puis mis en cache, et rafraîchissable depuis les paramètres.

use std::time::Duration;

use super::protocol::{Protocol, StreamAccum};
use crate::core::types::{Message, ToolCall, ToolSpec};
use crate::error::{Error, Result};

#[derive(Debug, Clone, serde::Serialize)]
pub struct ModelInfo {
    pub id: String,
    /// Identifiant complet `fournisseur/modèle` (compatibilité).
    pub full_id: String,
    pub name: String,
    pub description: String,
    pub family: String,
    /// Fenêtre de contexte, en jetons (0 = inconnue).
    pub context: u64,
    pub free: bool,
    pub reasoning: bool,
    /// `None` : modèle absent du catalogue public, capacité inconnue.
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

#[derive(Debug, Clone)]
pub struct LlmReply {
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
    /// Réponse coupée par la limite de longueur (`finish_reason = "length"`).
    pub truncated: bool,
    pub usage: Usage,
}

pub struct LlmClient {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
    session_id: String,
    /// Catalogue public (plusieurs Mo) gardé 1 h en mémoire.
    catalog: std::sync::Mutex<Option<(std::time::Instant, serde_json::Value)>>,
    /// Format d'API retenu pour chaque modèle (catalogue, ou essai réussi).
    protocols: std::sync::Mutex<std::collections::HashMap<String, Protocol>>,
}

impl LlmClient {
    pub fn new(base_url: &str, api_key: &str, session_id: &str) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(180))
            .user_agent("jimmy/0.1 (desktop agent)")
            .build()?;
        Ok(LlmClient {
            http,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            session_id: session_id.to_string(),
            catalog: std::sync::Mutex::new(None),
            protocols: std::sync::Mutex::new(std::collections::HashMap::new()),
        })
    }

    pub fn has_key(&self) -> bool {
        !self.api_key.trim().is_empty()
    }

    /// En-têtes d'un appel au format `protocol`. Le format Messages lit la
    /// clé dans `x-api-key` (sinon `401 Missing API key`, sonde du 6 octobre).
    fn protocol_headers(&self, protocol: Protocol) -> reqwest::header::HeaderMap {
        let mut headers = self.auth_headers();
        if protocol == Protocol::Messages {
            if let Ok(value) = reqwest::header::HeaderValue::from_str(&self.api_key) {
                headers.insert("x-api-key", value);
            }
            headers.insert("anthropic-version", reqwest::header::HeaderValue::from_static("2023-06-01"));
        }
        headers
    }

    fn auth_headers(&self) -> reqwest::header::HeaderMap {
        let mut headers = reqwest::header::HeaderMap::new();
        if let Ok(value) = reqwest::header::HeaderValue::from_str(&format!("Bearer {}", self.api_key)) {
            headers.insert(reqwest::header::AUTHORIZATION, value);
        }
        if let Ok(value) = reqwest::header::HeaderValue::from_str(&self.session_id) {
            headers.insert("x-opencode-session", value);
        }
        headers
    }

    /// Format d'API du modèle, d'après le catalogue public (`provider.npm`).
    /// Mis en cache ; catalogue injoignable = Chat, sans mémoriser.
    async fn protocol(&self, model: &str) -> Protocol {
        if let Some(known) = self.protocols.lock().ok().and_then(|p| p.get(model).copied()) {
            return known;
        }
        let Ok(catalog) = self.catalog_models(false).await else { return Protocol::Chat };
        let npm = catalog.get(model).and_then(|e| e.pointer("/provider/npm")).and_then(|v| v.as_str());
        let protocol = Protocol::from_npm(npm);
        log::info!("[llm] « {model} » : format {}", protocol.label());
        self.remember(model, protocol);
        protocol
    }

    fn remember(&self, model: &str, protocol: Protocol) {
        if let Ok(mut known) = self.protocols.lock() {
            known.insert(model.to_string(), protocol);
        }
    }

    /// Envoie la requête au format du modèle. Si le fournisseur répond
    /// `ModelProtocolUnsupported` (modèle absent du catalogue, catalogue
    /// injoignable), les autres formats sont essayés et le bon est retenu.
    /// Renvoie le format utilisé et la réponse HTTP réussie.
    async fn send(
        &self,
        model: &str,
        build: &(dyn Fn(Protocol) -> serde_json::Value + Sync),
        timeout: Option<Duration>,
    ) -> Result<(Protocol, reqwest::Response)> {
        if !self.has_key() {
            return Err(Error::provider("OpenCode Go", "clé OPENCODE_API_KEY absente"));
        }
        let first = self.protocol(model).await;
        let order = std::iter::once(first).chain(Protocol::ALL.into_iter().filter(move |p| *p != first));
        let mut last_error = None;
        for protocol in order {
            let body = build(protocol);
            // Diagnostic : `JIMMY_LLM_DUMP=<dossier>` enregistre chaque requête
            // (corps JSON, sans la clé) pour la rejouer à la main et comparer
            // des latences. Sans cette variable, rien n'est écrit.
            if let Some(dir) = std::env::var_os("JIMMY_LLM_DUMP") {
                let n = std::fs::read_dir(&dir).map(|d| d.count()).unwrap_or(0);
                let _ = std::fs::write(std::path::Path::new(&dir).join(format!("requete-{n:02}.json")), body.to_string());
            }
            let mut request = self
                .http
                .post(format!("{}{}", self.base_url, protocol.path()))
                .headers(self.protocol_headers(protocol))
                .json(&body);
            if let Some(timeout) = timeout {
                request = request.timeout(timeout);
            }
            let response = request.send().await.map_err(|e| {
                let message = match timeout {
                    Some(t) if e.is_timeout() => format!("pas de réponse en {} s", t.as_secs()),
                    _ => e.to_string(),
                };
                Error::provider("OpenCode Go", message)
            })?;
            let status = response.status();
            if status.is_success() {
                if protocol != first {
                    log::info!("[llm] « {model} » : format {} retenu après essai", protocol.label());
                    self.remember(model, protocol);
                }
                return Ok((protocol, response));
            }
            let raw = response.text().await.map_err(|e| Error::provider("OpenCode Go", e.to_string()))?;
            // Le corps peut contenir un secret d'authentification en théorie :
            // on ne renvoie jamais la clé, seulement le message du serveur.
            let error = Error::provider("OpenCode Go", format!("HTTP {status} — {}", truncate(&raw, 400)));
            if !raw.contains("ModelProtocolUnsupported") {
                return Err(error);
            }
            last_error = Some(error);
        }
        Err(last_error.unwrap_or_else(|| Error::provider("OpenCode Go", "aucun format d'API accepté")))
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
        let (protocol, response) = self.send(model, &build, None).await?;
        let raw = response.text().await.map_err(|e| Error::provider("OpenCode Go", e.to_string()))?;
        let value: serde_json::Value = serde_json::from_str(&raw)
            .map_err(|e| Error::provider("OpenCode Go", format!("réponse illisible : {e}")))?;
        protocol.parse(&value)
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
            .map_err(|e| Error::provider("OpenCode Go", e.to_string()))?
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
        acc.into_reply()
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

    /// Modèles utilisables, enrichis par le catalogue public.
    ///
    /// La **liste vient du compte** (`GET {base_url}/models`) : c'est ce que le
    /// fournisseur accepte réellement. Le catalogue public
    /// (`models.opencode.ai/api.json`, organisé par fournisseur) n'ajoute que le
    /// nom, le contexte, le prix et les capacités. L'ancienne version lisait
    /// une clé `models` à la racine du catalogue, qui n'existe pas : la liste
    /// était toujours vide.
    pub async fn list_models(&self, refresh: bool) -> Result<Vec<ModelInfo>> {
        let account = self.account_models().await;
        let catalog = self.catalog_models(refresh).await;
        let ids: Vec<String> = match (&account, &catalog) {
            (Ok(ids), _) if !ids.is_empty() => ids.clone(),
            (_, Ok(models)) => models.keys().cloned().collect(),
            (Err(error), _) => return Err(Error::provider("OpenCode Go", error.to_string())),
            _ => Vec::new(),
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
                    full_id: format!("opencode-go/{id}"),
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

    /// Identifiants des modèles accessibles avec cette clé.
    async fn account_models(&self) -> Result<Vec<String>> {
        let response = self
            .http
            .get(format!("{}/models", self.base_url))
            .headers(self.auth_headers())
            .timeout(Duration::from_secs(20))
            .send()
            .await
            .map_err(|e| Error::provider("OpenCode Go", e.to_string()))?;
        if !response.status().is_success() {
            return Err(Error::provider("OpenCode Go", format!("HTTP {}", response.status())));
        }
        let value: serde_json::Value = response
            .json()
            .await
            .map_err(|e| Error::provider("OpenCode Go", format!("liste illisible : {e}")))?;
        Ok(value
            .get("data")
            .and_then(|d| d.as_array())
            .map(|items| items.iter().filter_map(|m| m.get("id").and_then(|i| i.as_str()).map(String::from)).collect())
            .unwrap_or_default())
    }

    /// Entrées du fournisseur `opencode-go` dans le catalogue public.
    async fn catalog_models(
        &self,
        refresh: bool,
    ) -> Result<std::collections::HashMap<String, serde_json::Value>> {
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
    /// dans le format d'API du modèle. Plusieurs modèles du fournisseur
    /// répondent à la première et refusent la seconde en HTTP 400 : ils
    /// seraient inutilisables pour agir.
    pub async fn test_model(&self, model: &str) -> ModelTest {
        let ping = ToolSpec {
            name: "ping".into(),
            description: "Outil de test, à ne pas utiliser.".into(),
            parameters: serde_json::json!({ "type": "object", "properties": {} }),
        };
        let (plain, tools) = tokio::join!(
            self.timed_request(model, &[]),
            self.timed_request(model, std::slice::from_ref(&ping))
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
    async fn timed_request(&self, model: &str, tools: &[ToolSpec]) -> std::result::Result<(u64, String), String> {
        let messages = [Message::user("Réponds uniquement par le mot : ok")];
        let started = std::time::Instant::now();
        let build = |p: Protocol| p.body(model, &messages, tools, None, 128, false);
        let outcome = async {
            let (protocol, response) = self.send(model, &build, Some(Duration::from_secs(40))).await?;
            let raw = response.text().await.map_err(|e| Error::provider("OpenCode Go", e.to_string()))?;
            let value: serde_json::Value = serde_json::from_str(&raw)
                .map_err(|e| Error::provider("OpenCode Go", format!("réponse illisible : {e}")))?;
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
