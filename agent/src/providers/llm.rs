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
    /// Identifiant complet `provider/model`, celui qu'on stocke en config.
    pub full_id: String,
    pub name: String,
    pub context: u64,
    pub free: bool,
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
    pub usage: Usage,
}

pub struct LlmClient {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
    session_id: String,
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

    /// Catalogue des modèles OpenCode Go, mis en cache par l'appelant.
    pub async fn list_models(&self, provider: &str) -> Result<Vec<ModelInfo>> {
        #[derive(Deserialize)]
        struct Catalog {
            #[serde(default)]
            models: std::collections::HashMap<String, CatalogModel>,
        }
        #[derive(Deserialize)]
        struct CatalogModel {
            #[serde(default)]
            name: String,
            #[serde(default)]
            limit: Option<Limit>,
            #[serde(default)]
            cost: Option<Cost>,
        }
        #[derive(Deserialize)]
        struct Limit {
            #[serde(default)]
            context: u64,
        }
        #[derive(Deserialize)]
        struct Cost {
            #[serde(default)]
            input: f64,
            #[serde(default)]
            output: f64,
        }

        let response = self
            .http
            .get("https://models.opencode.ai/api.json")
            .send()
            .await
            .map_err(|e| Error::provider("OpenCode Go", e.to_string()))?;
        let catalog: Catalog = response
            .json()
            .await
            .map_err(|e| Error::provider("OpenCode Go", e.to_string()))?;

        let mut out: Vec<ModelInfo> = catalog
            .models
            .into_iter()
            .map(|(id, model)| ModelInfo {
                full_id: format!("{provider}/{id}"),
                id,
                name: model.name,
                context: model.limit.map(|l| l.context).unwrap_or(0),
                free: model.cost.map(|c| c.input == 0.0 && c.output == 0.0).unwrap_or(false),
            })
            .collect();
        out.sort_by(|a, b| (&a.free, &a.name).cmp(&(&b.free, &b.name)));
        Ok(out)
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