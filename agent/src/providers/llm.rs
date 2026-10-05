//! Client LLM.
//!
//! Jimmy parle à **OpenCode Go**, exposé en API compatible OpenAI
//! (`POST {base_url}/chat/completions`).
//!
//! Deux détails propre à ce fournisseur :
//!
//! * l'en-tête `x-opencode-session` est **obligatoire** ; sans lui l'API répond
//!   `400 MissingSessionID` car elle ne peut pas router la requête ;
//! * le catalogue des modèles vit à part : il est récupéré une fois au
//!   démarrage puis mis en cache, et rafraîchissable depuis les paramètres.

use std::time::Duration;

use serde::Deserialize;

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
        })
    }

    pub fn has_key(&self) -> bool {
        !self.api_key.trim().is_empty()
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

    /// Un appel de complétion, avec outils optionnels.
    pub async fn chat(
        &self,
        model: &str,
        messages: &[Message],
        tools: &[ToolSpec],
        temperature: Option<f32>,
        max_tokens: u32,
    ) -> Result<LlmReply> {
        if !self.has_key() {
            return Err(Error::provider("OpenCode Go", "clé OPENCODE_API_KEY absente"));
        }

        let mut body = serde_json::json!({
            "model": model,
            "messages": messages.iter().map(|m| m.to_wire()).collect::<Vec<_>>(),
            "max_tokens": max_tokens,
            "stream": false,
        });
        if let Some(temp) = temperature {
            body["temperature"] = serde_json::json!(temp);
        }
        if !tools.is_empty() {
            body["tools"] = serde_json::Value::Array(tools.iter().map(|t| t.to_wire()).collect());
            body["tool_choice"] = serde_json::json!("auto");
        }

        // Diagnostic : `JIMMY_LLM_DUMP=<dossier>` enregistre chaque requête
        // (corps JSON, sans la clé) pour la rejouer à la main et comparer des
        // latences. Sans cette variable, rien n'est écrit.
        if let Some(dir) = std::env::var_os("JIMMY_LLM_DUMP") {
            let n = std::fs::read_dir(&dir).map(|d| d.count()).unwrap_or(0);
            let _ = std::fs::write(
                std::path::Path::new(&dir).join(format!("requete-{n:02}.json")),
                body.to_string(),
            );
        }

        let response = self
            .http
            .post(format!("{}/chat/completions", self.base_url))
            .headers(self.auth_headers())
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::provider("OpenCode Go", e.to_string()))?;

        let status = response.status();
        let raw = response
            .text()
            .await
            .map_err(|e| Error::provider("OpenCode Go", e.to_string()))?;

        if !status.is_success() {
            // Le corps peut contenir un secret d'authentification en théorie :
            // on ne renvoie jamais la clé, seulement le message du serveur.
            return Err(Error::provider(
                "OpenCode Go",
                format!("HTTP {status} — {}", truncate(&raw, 400)),
            ));
        }

        let parsed: ChatResponse = serde_json::from_str(&raw)
            .map_err(|e| Error::provider("OpenCode Go", format!("réponse illisible : {e}")))?;

        let choice = parsed
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| Error::provider("OpenCode Go", "aucun choix dans la réponse"))?;

        let truncated = choice.finish_reason.as_deref() == Some("length");
        let mut tool_calls = Vec::new();
        for call in choice.message.tool_calls.unwrap_or_default() {
            let arguments = if call.function.arguments.trim().is_empty() {
                serde_json::json!({})
            } else {
                serde_json::from_str(&call.function.arguments).unwrap_or_else(|_| serde_json::json!({}))
            };
            tool_calls.push(ToolCall {
                id: call.id,
                name: call.function.name,
                arguments,
            });
        }

        Ok(LlmReply {
            content: choice.message.content.unwrap_or_default(),
            tool_calls,
            truncated,
            usage: Usage {
                prompt_tokens: parsed.usage.as_ref().map(|u| u.prompt_tokens).unwrap_or(0),
                completion_tokens: parsed.usage.as_ref().map(|u| u.completion_tokens).unwrap_or(0),
            },
        })
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
    /// simple, puis la même requête avec un outil déclaré (`tool_choice: auto`).
    /// Plusieurs modèles du fournisseur répondent à la première et refusent la
    /// seconde en HTTP 400 : ils seraient inutilisables pour agir.
    pub async fn test_model(&self, model: &str) -> ModelTest {
        let simple = serde_json::json!({
            "model": model,
            "messages": [{ "role": "user", "content": "Réponds uniquement par le mot : ok" }],
            "max_tokens": 128,
            "stream": false,
        });
        let mut with_tools = simple.clone();
        with_tools["tools"] = serde_json::json!([{
            "type": "function",
            "function": {
                "name": "ping",
                "description": "Outil de test, à ne pas utiliser.",
                "parameters": { "type": "object", "properties": {} },
            },
        }]);
        with_tools["tool_choice"] = serde_json::json!("auto");

        let (plain, tools) = tokio::join!(self.timed_request(&simple), self.timed_request(&with_tools));
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

    /// Une requête chronométrée. `Ok((durée en ms, texte de la réponse))`.
    async fn timed_request(&self, body: &serde_json::Value) -> std::result::Result<(u64, String), String> {
        let started = std::time::Instant::now();
        let response = self
            .http
            .post(format!("{}/chat/completions", self.base_url))
            .headers(self.auth_headers())
            .timeout(Duration::from_secs(40))
            .json(body)
            .send()
            .await
            .map_err(|e| if e.is_timeout() { "pas de réponse en 40 s".to_string() } else { e.to_string() })?;
        let status = response.status();
        let raw = response.text().await.map_err(|e| e.to_string())?;
        let elapsed = started.elapsed().as_millis() as u64;
        if !status.is_success() {
            // Le message du serveur (jamais la clé) : « HTTP 400 — invalid request ».
            let message = serde_json::from_str::<serde_json::Value>(&raw)
                .ok()
                .and_then(|v| v.get("error").and_then(|e| e.get("message")).and_then(|m| m.as_str()).map(String::from))
                .unwrap_or_else(|| truncate(&raw, 160));
            return Err(format!("HTTP {} — {}", status.as_u16(), truncate(&message, 160)));
        }
        let value: serde_json::Value = serde_json::from_str(&raw).map_err(|e| format!("réponse illisible : {e}"))?;
        let choice = value.get("choices").and_then(|c| c.get(0)).ok_or("aucun choix dans la réponse")?;
        let text = choice
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        Ok((elapsed, text))
    }
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

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
    usage: Option<UsageRaw>,
}

#[derive(Deserialize)]
struct Choice {
    message: MessageRaw,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct MessageRaw {
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<ToolCallRaw>>,
}

#[derive(Deserialize)]
struct ToolCallRaw {
    id: String,
    function: FunctionRaw,
}

#[derive(Deserialize)]
struct FunctionRaw {
    name: String,
    #[serde(default)]
    arguments: String,
}

#[derive(Deserialize)]
struct UsageRaw {
    #[serde(default)]
    prompt_tokens: u64,
    #[serde(default)]
    completion_tokens: u64,
}