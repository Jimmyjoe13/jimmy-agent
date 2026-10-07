//! Formats d'appel des modèles (« protocoles »).
//!
//! OpenCode Go sert chaque modèle dans **un seul** des trois formats d'API ; le
//! catalogue public l'indique par le paquet SDK du modèle (`provider.npm`) :
//!
//! * absent ou `@ai-sdk/openai-compatible` → [`Protocol::Chat`]
//!   (`/chat/completions`) : GLM, Kimi, MiMo, DeepSeek… ;
//! * `@ai-sdk/openai` → [`Protocol::Responses`] (`/responses`) : Muse Spark,
//!   GPT, Grok ;
//! * `@ai-sdk/anthropic` → [`Protocol::Messages`] (`/messages`) : Qwen 3.7+,
//!   MiniMax.
//!
//! Un modèle appelé dans un autre format répond `400 ModelProtocolUnsupported`
//! (cas réel du 6 octobre : Muse Spark 1.3 muet dans Jimmy, qui ne parlait que
//! Chat). Ce module traduit la conversation de Jimmy ([`Message`],
//! [`ToolSpec`]) vers chaque format, et chaque réponse — entière ou en flux
//! SSE — vers une [`LlmReply`].
//!
//! Formats vérifiés par sondes réelles (6 octobre 2026, Muse Spark 1.3 et
//! Qwen 3.8 Flash), aller-retour d'outil compris. Le raisonnement (chiffré
//! côté Responses, blocs `thinking` côté Messages) n'a pas besoin d'être
//! renvoyé au tour suivant : il est compté, jamais affiché ni réémis.

use serde::Deserialize;
use serde_json::{json, Value};

use super::llm::{LlmReply, Usage};
use crate::core::types::{Message, Role, ToolCall, ToolSpec};
use crate::error::{Error, Result};

const SERVICE: &str = "OpenCode Go";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Chat,
    Responses,
    Messages,
}

impl Protocol {
    /// Tous les formats, dans l'ordre d'essai quand le catalogue ne dit rien.
    pub const ALL: [Protocol; 3] = [Protocol::Chat, Protocol::Responses, Protocol::Messages];

    /// Format attendu d'après le paquet SDK du catalogue (`provider.npm`).
    pub fn from_npm(npm: Option<&str>) -> Protocol {
        match npm {
            Some("@ai-sdk/openai") => Protocol::Responses,
            Some("@ai-sdk/anthropic") => Protocol::Messages,
            _ => Protocol::Chat,
        }
    }

    /// Chemin de l'API, à ajouter à `base_url`.
    pub fn path(self) -> &'static str {
        match self {
            Protocol::Chat => "/chat/completions",
            Protocol::Responses => "/responses",
            Protocol::Messages => "/messages",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Protocol::Chat => "chat",
            Protocol::Responses => "responses",
            Protocol::Messages => "messages",
        }
    }

    /// Corps de la requête dans ce format.
    pub fn body(
        self,
        model: &str,
        messages: &[Message],
        tools: &[ToolSpec],
        temperature: Option<f32>,
        max_tokens: u32,
        stream: bool,
    ) -> Value {
        let mut body = match self {
            Protocol::Chat => chat_body(model, messages, tools, max_tokens),
            Protocol::Responses => responses_body(model, messages, tools, max_tokens),
            Protocol::Messages => messages_body(model, messages, tools, max_tokens),
        };
        body["stream"] = json!(stream);
        if let Some(temp) = temperature {
            body["temperature"] = json!(temp);
        }
        body
    }

    /// Réponse entière (sans flux) → [`LlmReply`].
    pub fn parse(self, value: &Value) -> Result<LlmReply> {
        match self {
            Protocol::Chat => parse_chat(value),
            Protocol::Responses => parse_responses(value),
            Protocol::Messages => parse_messages(value),
        }
    }
}

// ── Outils communs ───────────────────────────────────────────────────────────

/// Arguments d'outil reçus en texte JSON ; vide ou illisible = objet vide.
fn parse_args(raw: &str) -> Value {
    if raw.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str(raw).unwrap_or_else(|_| json!({}))
    }
}

/// Les messages système, réunis : Responses et Messages les veulent à part
/// (`instructions`, `system`), pas dans la conversation.
fn system_text(messages: &[Message]) -> String {
    messages
        .iter()
        .filter(|m| m.role == Role::System && !m.content.trim().is_empty())
        .map(|m| m.content.as_str())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn tool_calls(message: &Message) -> impl Iterator<Item = &ToolCall> {
    message.tool_calls.iter().flatten()
}

/// Message d'erreur du serveur dans un corps JSON (`error.message`).
fn server_error(value: &Value) -> Option<String> {
    let error = value.get("error").filter(|e| !e.is_null())?;
    Some(
        error
            .get("message")
            .and_then(|m| m.as_str())
            .map(String::from)
            .unwrap_or_else(|| error.to_string()),
    )
}

// ── Chat (`/chat/completions`, compatible OpenAI) ────────────────────────────

fn chat_body(model: &str, messages: &[Message], tools: &[ToolSpec], max_tokens: u32) -> Value {
    let mut body = json!({
        "model": model,
        "messages": messages.iter().map(|m| m.to_wire()).collect::<Vec<_>>(),
        "max_tokens": max_tokens,
    });
    if !tools.is_empty() {
        body["tools"] = Value::Array(tools.iter().map(|t| t.to_wire()).collect());
        body["tool_choice"] = json!("auto");
    }
    body
}

fn parse_chat(value: &Value) -> Result<LlmReply> {
    let parsed: ChatResponse = serde_json::from_value(value.clone())
        .map_err(|e| Error::provider(SERVICE, format!("réponse illisible : {e}")))?;
    let choice = parsed
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| Error::provider(SERVICE, "aucun choix dans la réponse"))?;
    let tool_calls = choice
        .message
        .tool_calls
        .unwrap_or_default()
        .into_iter()
        .map(|call| ToolCall {
            id: call.id,
            name: call.function.name,
            arguments: parse_args(&call.function.arguments),
        })
        .collect();
    Ok(LlmReply {
        content: choice.message.content.unwrap_or_default(),
        tool_calls,
        truncated: choice.finish_reason.as_deref() == Some("length"),
        usage: Usage {
            prompt_tokens: parsed.usage.as_ref().map(|u| u.prompt_tokens).unwrap_or(0),
            completion_tokens: parsed.usage.as_ref().map(|u| u.completion_tokens).unwrap_or(0),
        },
    })
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

/// Assemble les fragments SSE Chat en une [`LlmReply`]. Le contenu visible
/// (`delta.content`) et le raisonnement caché (`delta.reasoning_content`,
/// compté mais jamais affiché) arrivent en fragments distincts ; les appels
/// d'outils arrivent éparpillés par `index` — identifiant et nom au premier
/// fragment, `arguments` en morceaux ensuite ; `usage` n'arrive que dans un
/// fragment final. Formats constatés sur le fournisseur (sondes réelles du
/// 5 octobre 2026).
#[derive(Default)]
pub struct ChatAccum {
    content: String,
    reasoning: String,
    tool_calls: Vec<ToolCallAccum>,
    finish_reason: Option<String>,
    usage: Usage,
}

#[derive(Default, Clone)]
struct ToolCallAccum {
    id: String,
    name: String,
    arguments: String,
}

impl ChatAccum {
    fn ingest(&mut self, value: &Value, on_delta: &mut dyn FnMut(&str)) {
        // `usage` arrive dans un fragment séparé (souvent le dernier).
        if let Some(u) = value.get("usage").filter(|u| !u.is_null()) {
            self.usage.prompt_tokens = u
                .get("prompt_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(self.usage.prompt_tokens);
            self.usage.completion_tokens = u
                .get("completion_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(self.usage.completion_tokens);
        }
        let Some(choices) = value.get("choices").and_then(|c| c.as_array()) else { return };
        for choice in choices {
            if let Some(reason) = choice.get("finish_reason").and_then(|f| f.as_str()) {
                self.finish_reason = Some(reason.to_string());
            }
            let Some(delta) = choice.get("delta") else { continue };
            if let Some(text) = delta.get("reasoning_content").and_then(|c| c.as_str()) {
                self.reasoning.push_str(text);
            }
            if let Some(text) = delta.get("content").and_then(|c| c.as_str()) {
                if !text.is_empty() {
                    self.content.push_str(text);
                    on_delta(text);
                }
            }
            let Some(calls) = delta.get("tool_calls").and_then(|c| c.as_array()) else { continue };
            for call in calls {
                let index = call.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
                if self.tool_calls.len() <= index {
                    self.tool_calls.resize(index + 1, ToolCallAccum::default());
                }
                let slot = &mut self.tool_calls[index];
                if let Some(id) = call.get("id").and_then(|v| v.as_str()) {
                    slot.id.push_str(id);
                }
                if let Some(function) = call.get("function") {
                    if let Some(name) = function.get("name").and_then(|v| v.as_str()) {
                        slot.name.push_str(name);
                    }
                    if let Some(args) = function.get("arguments").and_then(|v| v.as_str()) {
                        slot.arguments.push_str(args);
                    }
                }
            }
        }
    }

    fn into_reply(self) -> Result<LlmReply> {
        // Le raisonnement caché consomme le plafond de jetons sans être visible
        // dans le contenu ; sa taille au journal aide à comprendre les réponses
        // coupées (max_tokens mangé par la réflexion, piège 70).
        log::debug!(
            "[llm] flux : {} caractères visibles, {} de raisonnement",
            self.content.chars().count(),
            self.reasoning.chars().count()
        );
        let tool_calls = self
            .tool_calls
            .into_iter()
            .map(|call| ToolCall { id: call.id, name: call.name, arguments: parse_args(&call.arguments) })
            .collect::<Vec<_>>();
        // Ni contenu, ni outil, ni raison de fin : le flux s'est coupé avant
        // la première réponse. Tout autre cas produit une réponse utilisable.
        if self.finish_reason.is_none() && self.content.is_empty() && tool_calls.is_empty() {
            return Err(interrupted());
        }
        Ok(LlmReply {
            truncated: self.finish_reason.as_deref() == Some("length"),
            content: self.content,
            tool_calls,
            usage: self.usage,
        })
    }
}

fn interrupted() -> Error {
    Error::provider(SERVICE, "flux interrompu avant la fin (aucun fragment reçu)")
}

// ── Responses (`/responses`, OpenAI) ─────────────────────────────────────────

/// La conversation devient une liste d'**éléments** : messages, appels
/// d'outil (`function_call`) et leurs résultats (`function_call_output`),
/// reliés par `call_id`.
fn responses_body(model: &str, messages: &[Message], tools: &[ToolSpec], max_tokens: u32) -> Value {
    let mut input = Vec::new();
    for message in messages {
        match message.role {
            Role::System => {}
            Role::User if message.images.is_empty() => input.push(json!({ "role": "user", "content": message.content })),
            // Avec image : parties `input_text` / `input_image` (sonde du
            // 7 octobre, muse-spark-1.3-contributor).
            Role::User => {
                let mut parts = vec![json!({ "type": "input_text", "text": message.content })];
                parts.extend(message.images.iter().map(|image| json!({ "type": "input_image", "image_url": image.data_url() })));
                input.push(json!({ "role": "user", "content": parts }));
            }
            Role::Assistant => {
                if !message.content.trim().is_empty() {
                    input.push(json!({ "role": "assistant", "content": message.content }));
                }
                for call in tool_calls(message) {
                    input.push(json!({
                        "type": "function_call",
                        "call_id": call.id,
                        "name": call.name,
                        "arguments": serde_json::to_string(&call.arguments).unwrap_or_default(),
                    }));
                }
            }
            Role::Tool => input.push(json!({
                "type": "function_call_output",
                "call_id": message.tool_call_id.clone().unwrap_or_default(),
                "output": message.content,
            })),
        }
    }
    let mut body = json!({ "model": model, "input": input, "max_output_tokens": max_tokens });
    let system = system_text(messages);
    if !system.is_empty() {
        body["instructions"] = json!(system);
    }
    if !tools.is_empty() {
        body["tools"] = Value::Array(
            tools
                .iter()
                .map(|t| json!({ "type": "function", "name": t.name, "description": t.description, "parameters": t.parameters }))
                .collect(),
        );
        body["tool_choice"] = json!("auto");
    }
    body
}

/// Objet `response` (réponse entière, ou `response.completed` en flux).
fn parse_responses(value: &Value) -> Result<LlmReply> {
    if let Some(message) = server_error(value) {
        return Err(Error::provider(SERVICE, message));
    }
    let status = value.get("status").and_then(|s| s.as_str()).unwrap_or("");
    if status == "failed" {
        return Err(Error::provider(SERVICE, "génération en échec (status failed)"));
    }
    let mut content = String::new();
    let mut calls = Vec::new();
    for item in value.get("output").and_then(|o| o.as_array()).into_iter().flatten() {
        match item.get("type").and_then(|t| t.as_str()) {
            Some("message") => {
                for part in item.get("content").and_then(|c| c.as_array()).into_iter().flatten() {
                    if part.get("type").and_then(|t| t.as_str()) == Some("output_text") {
                        content.push_str(part.get("text").and_then(|t| t.as_str()).unwrap_or(""));
                    }
                }
            }
            Some("function_call") => calls.push(ToolCall {
                id: item.get("call_id").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                name: item.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                arguments: parse_args(item.get("arguments").and_then(|v| v.as_str()).unwrap_or("")),
            }),
            // `reasoning` : chiffré, ni affiché ni renvoyé.
            _ => {}
        }
    }
    let usage = value.get("usage");
    let tokens = |key: &str| usage.and_then(|u| u.get(key)).and_then(|v| v.as_u64()).unwrap_or(0);
    Ok(LlmReply {
        content,
        tool_calls: calls,
        // `incomplete` : plafond `max_output_tokens` atteint.
        truncated: status == "incomplete",
        usage: Usage { prompt_tokens: tokens("input_tokens"), completion_tokens: tokens("output_tokens") },
    })
}

/// Flux Responses : le texte arrive en `response.output_text.delta` ; la
/// réponse complète (outils, usage, statut) dans l'événement final
/// `response.completed` (ou `response.incomplete`), qui fait foi.
#[derive(Default)]
pub struct ResponsesAccum {
    content: String,
    done: Option<Value>,
    error: Option<String>,
}

impl ResponsesAccum {
    fn ingest(&mut self, value: &Value, on_delta: &mut dyn FnMut(&str)) {
        match value.get("type").and_then(|t| t.as_str()).unwrap_or("") {
            "response.output_text.delta" => {
                if let Some(text) = value.get("delta").and_then(|d| d.as_str()).filter(|t| !t.is_empty()) {
                    self.content.push_str(text);
                    on_delta(text);
                }
            }
            "response.completed" | "response.incomplete" => self.done = value.get("response").cloned(),
            "response.failed" => {
                self.error = Some(
                    value
                        .get("response")
                        .and_then(server_error)
                        .unwrap_or_else(|| "génération en échec".into()),
                )
            }
            "error" => {
                self.error = Some(
                    value
                        .get("message")
                        .and_then(|m| m.as_str())
                        .map(String::from)
                        .or_else(|| server_error(value))
                        .unwrap_or_else(|| value.to_string()),
                )
            }
            _ => {}
        }
    }

    fn into_reply(self) -> Result<LlmReply> {
        if let Some(message) = self.error {
            return Err(Error::provider(SERVICE, message));
        }
        match self.done {
            Some(response) => parse_responses(&response),
            // Flux coupé avant l'événement final : le texte reçu vaut mieux
            // que rien, mais il est marqué coupé.
            None if !self.content.is_empty() => Ok(LlmReply {
                content: self.content,
                tool_calls: Vec::new(),
                truncated: true,
                usage: Usage::default(),
            }),
            None => Err(interrupted()),
        }
    }
}

// ── Messages (`/messages`, Anthropic) ────────────────────────────────────────

/// Ajoute un bloc au dernier message s'il a le même rôle : Messages exige
/// l'alternance utilisateur / assistant, et plusieurs résultats d'outil
/// consécutifs vont dans **un** message utilisateur.
fn push_block(out: &mut Vec<Value>, role: &str, block: Value) {
    if let Some(last) = out.last_mut().filter(|m| m["role"] == role) {
        if let Some(blocks) = last["content"].as_array_mut() {
            blocks.push(block);
            return;
        }
    }
    out.push(json!({ "role": role, "content": [block] }));
}

fn messages_body(model: &str, messages: &[Message], tools: &[ToolSpec], max_tokens: u32) -> Value {
    let mut out: Vec<Value> = Vec::new();
    for message in messages {
        match message.role {
            Role::System => {}
            Role::User => {
                // Images d'abord, puis le texte (sonde du 7 octobre,
                // qwen3.8-flash) : base64 brut et type de média à part.
                for image in &message.images {
                    push_block(
                        &mut out,
                        "user",
                        json!({ "type": "image", "source": { "type": "base64", "media_type": image.media_type, "data": image.base64 } }),
                    );
                }
                // Un bloc de texte vide est refusé par l'API.
                if !message.content.trim().is_empty() {
                    push_block(&mut out, "user", json!({ "type": "text", "text": message.content }));
                }
            }
            Role::Assistant => {
                if !message.content.trim().is_empty() {
                    push_block(&mut out, "assistant", json!({ "type": "text", "text": message.content }));
                }
                for call in tool_calls(message) {
                    // `input` doit être un objet.
                    let input = if call.arguments.is_object() { call.arguments.clone() } else { json!({}) };
                    push_block(
                        &mut out,
                        "assistant",
                        json!({ "type": "tool_use", "id": call.id, "name": call.name, "input": input }),
                    );
                }
            }
            Role::Tool => {
                let content = if message.content.is_empty() { "(vide)" } else { message.content.as_str() };
                push_block(
                    &mut out,
                    "user",
                    json!({
                        "type": "tool_result",
                        "tool_use_id": message.tool_call_id.clone().unwrap_or_default(),
                        "content": content,
                    }),
                );
            }
        }
    }
    let mut body = json!({ "model": model, "messages": out, "max_tokens": max_tokens });
    let system = system_text(messages);
    if !system.is_empty() {
        body["system"] = json!(system);
    }
    if !tools.is_empty() {
        body["tools"] = Value::Array(
            tools
                .iter()
                .map(|t| json!({ "name": t.name, "description": t.description, "input_schema": t.parameters }))
                .collect(),
        );
        body["tool_choice"] = json!({ "type": "auto" });
    }
    body
}

fn parse_messages(value: &Value) -> Result<LlmReply> {
    if let Some(message) = server_error(value) {
        return Err(Error::provider(SERVICE, message));
    }
    let mut content = String::new();
    let mut calls = Vec::new();
    for block in value.get("content").and_then(|c| c.as_array()).into_iter().flatten() {
        match block.get("type").and_then(|t| t.as_str()) {
            Some("text") => content.push_str(block.get("text").and_then(|t| t.as_str()).unwrap_or("")),
            Some("tool_use") => calls.push(ToolCall {
                id: block.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                name: block.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                arguments: block.get("input").cloned().filter(|i| i.is_object()).unwrap_or_else(|| json!({})),
            }),
            // `thinking` : raisonnement, ni affiché ni renvoyé.
            _ => {}
        }
    }
    let usage = value.get("usage");
    let tokens = |key: &str| usage.and_then(|u| u.get(key)).and_then(|v| v.as_u64()).unwrap_or(0);
    Ok(LlmReply {
        content,
        tool_calls: calls,
        truncated: value.get("stop_reason").and_then(|s| s.as_str()) == Some("max_tokens"),
        usage: Usage { prompt_tokens: tokens("input_tokens"), completion_tokens: tokens("output_tokens") },
    })
}

/// Flux Messages : des blocs numérotés (`content_block_start`, puis
/// `content_block_delta` : `text_delta`, `input_json_delta` pour les
/// arguments d'outil en morceaux, `thinking_delta` ignoré), puis
/// `message_delta` (raison d'arrêt, usage) et `message_stop`.
#[derive(Default)]
pub struct MessagesAccum {
    content: String,
    blocks: Vec<BlockAccum>,
    stop_reason: Option<String>,
    usage: Usage,
    error: Option<String>,
}

#[derive(Default, Clone)]
struct BlockAccum {
    kind: String,
    id: String,
    name: String,
    json: String,
}

impl MessagesAccum {
    fn ingest(&mut self, value: &Value, on_delta: &mut dyn FnMut(&str)) {
        let index = value.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
        match value.get("type").and_then(|t| t.as_str()).unwrap_or("") {
            "message_start" => {
                if let Some(n) = value.pointer("/message/usage/input_tokens").and_then(|v| v.as_u64()) {
                    self.usage.prompt_tokens = n;
                }
            }
            "content_block_start" => {
                if self.blocks.len() <= index {
                    self.blocks.resize(index + 1, BlockAccum::default());
                }
                let block = value.get("content_block");
                let text = |key: &str| block.and_then(|b| b.get(key)).and_then(|v| v.as_str()).unwrap_or("").to_string();
                self.blocks[index] = BlockAccum { kind: text("type"), id: text("id"), name: text("name"), json: String::new() };
            }
            "content_block_delta" => {
                let delta = value.get("delta");
                let field = |key: &str| delta.and_then(|d| d.get(key)).and_then(|v| v.as_str()).unwrap_or("");
                match field("type") {
                    "text_delta" => {
                        let text = field("text");
                        if !text.is_empty() {
                            self.content.push_str(text);
                            on_delta(text);
                        }
                    }
                    "input_json_delta" => {
                        if let Some(block) = self.blocks.get_mut(index) {
                            block.json.push_str(field("partial_json"));
                        }
                    }
                    _ => {}
                }
            }
            "message_delta" => {
                if let Some(reason) = value.pointer("/delta/stop_reason").and_then(|v| v.as_str()) {
                    self.stop_reason = Some(reason.to_string());
                }
                if let Some(n) = value.pointer("/usage/output_tokens").and_then(|v| v.as_u64()) {
                    self.usage.completion_tokens = n;
                }
                if let Some(n) = value.pointer("/usage/input_tokens").and_then(|v| v.as_u64()) {
                    self.usage.prompt_tokens = n;
                }
            }
            "error" => self.error = Some(server_error(value).unwrap_or_else(|| value.to_string())),
            _ => {}
        }
    }

    fn into_reply(self) -> Result<LlmReply> {
        if let Some(message) = self.error {
            return Err(Error::provider(SERVICE, message));
        }
        let tool_calls = self
            .blocks
            .into_iter()
            .filter(|b| b.kind == "tool_use")
            .map(|b| ToolCall { id: b.id, name: b.name, arguments: parse_args(&b.json) })
            .collect::<Vec<_>>();
        if self.stop_reason.is_none() && self.content.is_empty() && tool_calls.is_empty() {
            return Err(interrupted());
        }
        Ok(LlmReply {
            truncated: self.stop_reason.as_deref() == Some("max_tokens"),
            content: self.content,
            tool_calls,
            usage: self.usage,
        })
    }
}

// ── Flux, tous formats ───────────────────────────────────────────────────────

/// Assembleur de flux SSE du format utilisé.
pub enum StreamAccum {
    Chat(ChatAccum),
    Responses(ResponsesAccum),
    Messages(MessagesAccum),
}

impl StreamAccum {
    pub fn new(protocol: Protocol) -> Self {
        match protocol {
            Protocol::Chat => StreamAccum::Chat(ChatAccum::default()),
            Protocol::Responses => StreamAccum::Responses(ResponsesAccum::default()),
            Protocol::Messages => StreamAccum::Messages(MessagesAccum::default()),
        }
    }

    /// Un événement SSE (`data: {…}` décodé). `on_delta` ne reçoit que le
    /// texte visible, jamais le raisonnement.
    pub fn ingest(&mut self, value: &Value, on_delta: &mut dyn FnMut(&str)) {
        match self {
            StreamAccum::Chat(acc) => acc.ingest(value, on_delta),
            StreamAccum::Responses(acc) => acc.ingest(value, on_delta),
            StreamAccum::Messages(acc) => acc.ingest(value, on_delta),
        }
    }

    pub fn into_reply(self) -> Result<LlmReply> {
        match self {
            StreamAccum::Chat(acc) => acc.into_reply(),
            StreamAccum::Responses(acc) => acc.into_reply(),
            StreamAccum::Messages(acc) => acc.into_reply(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(protocol: Protocol, events: &[&str]) -> (Result<LlmReply>, String) {
        let mut seen = String::new();
        let mut acc = StreamAccum::new(protocol);
        for raw in events {
            acc.ingest(&serde_json::from_str::<Value>(raw).unwrap(), &mut |text| seen.push_str(text));
        }
        (acc.into_reply(), seen)
    }

    /// Une conversation avec outil : système, demande, appel, résultat.
    fn conversation() -> Vec<Message> {
        let mut assistant = Message::assistant("Je regarde.");
        assistant.tool_calls = Some(vec![
            ToolCall { id: "c1".into(), name: "heure".into(), arguments: json!({ "ville": "Paris" }) },
            ToolCall { id: "c2".into(), name: "heure".into(), arguments: json!({ "ville": "Lyon" }) },
        ]);
        vec![
            Message::system("Tu es Jimmy."),
            Message::user("Quelle heure est-il ?"),
            assistant,
            Message::tool_result("c1", "heure", "14:32"),
            Message::tool_result("c2", "heure", ""),
        ]
    }

    fn heure() -> Vec<ToolSpec> {
        vec![ToolSpec {
            name: "heure".into(),
            description: "Donne l'heure.".into(),
            parameters: json!({ "type": "object", "properties": { "ville": { "type": "string" } } }),
        }]
    }

    #[test]
    fn le_format_vient_du_catalogue() {
        assert_eq!(Protocol::from_npm(Some("@ai-sdk/openai")), Protocol::Responses);
        assert_eq!(Protocol::from_npm(Some("@ai-sdk/anthropic")), Protocol::Messages);
        assert_eq!(Protocol::from_npm(Some("@ai-sdk/openai-compatible")), Protocol::Chat);
        assert_eq!(Protocol::from_npm(None), Protocol::Chat);
    }

    // ── Chat ──

    /// Fragments capturés dans les sondes réelles du 5 octobre 2026 : le
    /// premier chunk, du raisonnement caché, un fragment de contenu, puis
    /// l'usage final. Séquence `data: …` telle qu'arrivée du fournisseur.
    #[test]
    fn chat_les_fragments_sont_assembles_comme_en_reel() {
        let (reply, seen) = feed(
            Protocol::Chat,
            &[
                r#"{"choices":[{"index":0,"finish_reason":null,"logprobs":null,"delta":{"role":"assistant","content":""}}],"usage":null}"#,
                r#"{"choices":[{"index":0,"finish_reason":null,"delta":{"reasoning_content":"The user"}}],"usage":null}"#,
                r#"{"choices":[{"index":0,"finish_reason":null,"delta":{"content":"Bon"}}],"usage":null}"#,
                r#"{"choices":[{"index":0,"finish_reason":null,"delta":{"content":"jour"}}],"usage":null}"#,
                r#"{"choices":[{"index":0,"finish_reason":"stop","delta":{}}],"usage":null}"#,
                r#"{"usage":{"prompt_tokens":17,"completion_tokens":32},"choices":[]}"#,
            ],
        );
        let reply = reply.unwrap();
        assert_eq!(reply.content, "Bonjour");
        assert_eq!(seen, "Bonjour", "on_delta ne reçoit que le contenu visible");
        assert!(!reply.truncated);
        assert_eq!(reply.usage.completion_tokens, 32);
    }

    /// Les fragments d'outil sont éparpillés : identifiant et nom au premier,
    /// `arguments` en morceaux ensuite, assemblés par `index`.
    #[test]
    fn chat_les_appels_d_outils_sont_reconstitues_par_index() {
        let (reply, _) = feed(
            Protocol::Chat,
            &[
                r#"{"choices":[{"index":0,"delta":{"role":"assistant","content":""}}]}"#,
                r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"834e2f99","type":"function","function":{"name":"ping","arguments":""}}]}}]}"#,
                r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{"}}]}}]}"#,
                r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"a\":1}"}}]}}]}"#,
                r#"{"choices":[{"index":0,"finish_reason":"tool_calls","delta":{}}]}"#,
                r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":1,"id":"second","type":"function","function":{"name":"deux","arguments":"{}"}}]}}]}"#,
            ],
        );
        let reply = reply.expect("reply");
        assert_eq!(reply.tool_calls.len(), 2, "{reply:?}");
        assert_eq!(reply.tool_calls[0].id, "834e2f99");
        assert_eq!(reply.tool_calls[0].name, "ping");
        assert_eq!(reply.tool_calls[0].arguments["a"], 1);
        assert_eq!(reply.tool_calls[1].id, "second");
        assert_eq!(reply.tool_calls[1].name, "deux");
    }

    // ── Responses ──

    /// Une image jointe part dans la forme acceptée par chaque format (sonde
    /// réelle du 7 octobre : les quatre modèles ont lu « rouge, bleu »).
    #[test]
    fn une_image_jointe_suit_le_format_de_chaque_api() {
        let image = crate::core::types::Image { media_type: "image/jpeg".into(), base64: "QUJD".into(), label: "Éditeur".into() };
        let messages = vec![
            Message::system("Tu es Jimmy."),
            Message::user_with_images("Que vois-tu ?", vec![image]),
        ];
        let chat = Protocol::Chat.body("m", &messages, &[], None, 512, false);
        let parts = &chat["messages"][1]["content"];
        assert_eq!(parts[0], json!({ "type": "text", "text": "Que vois-tu ?" }));
        assert_eq!(parts[1], json!({ "type": "image_url", "image_url": { "url": "data:image/jpeg;base64,QUJD" } }));

        let responses = Protocol::Responses.body("m", &messages, &[], None, 512, false);
        let parts = &responses["input"][0]["content"];
        assert_eq!(parts[0], json!({ "type": "input_text", "text": "Que vois-tu ?" }));
        assert_eq!(parts[1], json!({ "type": "input_image", "image_url": "data:image/jpeg;base64,QUJD" }));

        let anthropic = Protocol::Messages.body("m", &messages, &[], None, 512, false);
        let blocks = &anthropic["messages"][0]["content"];
        assert_eq!(blocks[0], json!({ "type": "image", "source": { "type": "base64", "media_type": "image/jpeg", "data": "QUJD" } }));
        assert_eq!(blocks[1], json!({ "type": "text", "text": "Que vois-tu ?" }));

        // Sans image : le texte reste une simple chaîne (rien ne change).
        let plain = Protocol::Chat.body("m", &[Message::user("Bonjour")], &[], None, 512, false);
        assert_eq!(plain["messages"][0]["content"], "Bonjour");
    }

    /// Corps tel qu'accepté par Muse Spark 1.3 (sonde du 6 octobre) : système
    /// en `instructions`, appels et résultats en éléments reliés par `call_id`.
    #[test]
    fn responses_la_conversation_devient_des_elements() {
        let body = Protocol::Responses.body("muse", &conversation(), &heure(), Some(0.3), 512, true);
        assert_eq!(body["instructions"], "Tu es Jimmy.");
        assert_eq!(body["max_output_tokens"], 512);
        assert_eq!(body["stream"], true);
        let input = body["input"].as_array().unwrap();
        assert_eq!(input[0], json!({ "role": "user", "content": "Quelle heure est-il ?" }));
        assert_eq!(input[1], json!({ "role": "assistant", "content": "Je regarde." }));
        assert_eq!(input[2]["type"], "function_call");
        assert_eq!(input[2]["call_id"], "c1");
        assert_eq!(input[2]["arguments"], r#"{"ville":"Paris"}"#);
        assert_eq!(input[4], json!({ "type": "function_call_output", "call_id": "c1", "output": "14:32" }));
        assert!(input.iter().all(|i| i.get("role") != Some(&json!("system"))));
        assert_eq!(body["tools"][0], json!({ "type": "function", "name": "heure", "description": "Donne l'heure.", "parameters": heure()[0].parameters }));
        assert!(body.get("messages").is_none());
    }

    /// Réponse réelle de Muse Spark 1.3 (raccourcie) : raisonnement chiffré,
    /// message, appel d'outil.
    #[test]
    fn responses_la_reponse_entiere_est_lue() {
        let value: Value = serde_json::from_str(
            r#"{"status":"completed","output":[
                {"id":"rs_1","type":"reasoning","encrypted_content":"Q-Pa"},
                {"type":"message","content":[{"type":"output_text","text":"Je regarde.","annotations":[]}]},
                {"type":"function_call","call_id":"call_a7","name":"heure","arguments":"{\"ville\":\"Paris\"}"}],
              "usage":{"input_tokens":547,"output_tokens":181}}"#,
        )
        .unwrap();
        let reply = Protocol::Responses.parse(&value).unwrap();
        assert_eq!(reply.content, "Je regarde.");
        assert_eq!(reply.tool_calls[0].id, "call_a7");
        assert_eq!(reply.tool_calls[0].arguments["ville"], "Paris");
        assert_eq!((reply.usage.prompt_tokens, reply.usage.completion_tokens), (547, 181));
        assert!(!reply.truncated);
        let cut: Value = serde_json::from_str(r#"{"status":"incomplete","output":[]}"#).unwrap();
        assert!(Protocol::Responses.parse(&cut).unwrap().truncated);
    }

    /// Flux réel : le texte en deltas, la réponse complète à la fin.
    #[test]
    fn responses_le_flux_est_assemble() {
        let (reply, seen) = feed(
            Protocol::Responses,
            &[
                r#"{"type":"response.created","response":{"status":"in_progress","output":[]}}"#,
                r#"{"type":"response.output_text.delta","output_index":1,"delta":"Il est 14"}"#,
                r#"{"type":"response.output_text.delta","output_index":1,"delta":"h32 à Paris."}"#,
                r#"{"type":"response.completed","response":{"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"Il est 14h32 à Paris."}]},{"type":"function_call","call_id":"c9","name":"heure","arguments":"{}"}],"usage":{"input_tokens":10,"output_tokens":5}}}"#,
            ],
        );
        let reply = reply.unwrap();
        assert_eq!(seen, "Il est 14h32 à Paris.");
        assert_eq!(reply.content, "Il est 14h32 à Paris.");
        assert_eq!(reply.tool_calls[0].id, "c9");
        assert_eq!(reply.usage.completion_tokens, 5);
        let (failed, _) = feed(Protocol::Responses, &[r#"{"type":"error","message":"quota dépassé"}"#]);
        assert!(failed.unwrap_err().to_string().contains("quota dépassé"));
        let (empty, _) = feed(Protocol::Responses, &[]);
        assert!(empty.is_err());
    }

    // ── Messages ──

    /// Corps tel qu'accepté par Qwen 3.8 Flash (sonde du 6 octobre) :
    /// système à part, alternance stricte, résultats d'outil regroupés dans
    /// un seul message utilisateur, jamais de texte vide.
    #[test]
    fn messages_la_conversation_alterne_les_roles() {
        let body = Protocol::Messages.body("qwen", &conversation(), &heure(), None, 512, false);
        assert_eq!(body["system"], "Tu es Jimmy.");
        assert_eq!(body["max_tokens"], 512);
        assert!(body.get("temperature").is_none());
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 3, "{messages:?}");
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[1]["role"], "assistant");
        assert_eq!(messages[1]["content"][0], json!({ "type": "text", "text": "Je regarde." }));
        assert_eq!(messages[1]["content"][1], json!({ "type": "tool_use", "id": "c1", "name": "heure", "input": { "ville": "Paris" } }));
        assert_eq!(messages[1]["content"][2]["id"], "c2");
        assert_eq!(messages[2]["role"], "user");
        assert_eq!(messages[2]["content"][0], json!({ "type": "tool_result", "tool_use_id": "c1", "content": "14:32" }));
        assert_eq!(messages[2]["content"][1]["content"], "(vide)");
        assert_eq!(body["tools"][0]["input_schema"], heure()[0].parameters);
        assert_eq!(body["tool_choice"], json!({ "type": "auto" }));
    }

    #[test]
    fn messages_la_reponse_entiere_est_lue() {
        let value: Value = serde_json::from_str(
            r#"{"type":"message","stop_reason":"tool_use","content":[
                {"type":"thinking","thinking":"The user asks"},
                {"type":"tool_use","id":"toolu_9a","name":"heure","input":{"ville":"Paris"}}],
              "usage":{"input_tokens":331,"output_tokens":78}}"#,
        )
        .unwrap();
        let reply = Protocol::Messages.parse(&value).unwrap();
        assert_eq!(reply.content, "", "le raisonnement n'est jamais du contenu");
        assert_eq!(reply.tool_calls[0].id, "toolu_9a");
        assert_eq!(reply.tool_calls[0].arguments["ville"], "Paris");
        assert_eq!(reply.usage.prompt_tokens, 331);
        let error: Value = serde_json::from_str(r#"{"type":"error","error":{"type":"AuthError","message":"Missing API key."}}"#).unwrap();
        assert!(Protocol::Messages.parse(&error).unwrap_err().to_string().contains("Missing API key"));
    }

    /// Flux réel de Qwen 3.8 Flash : raisonnement, puis outil dont les
    /// arguments arrivent en morceaux de JSON.
    #[test]
    fn messages_le_flux_est_assemble() {
        let (reply, seen) = feed(
            Protocol::Messages,
            &[
                r#"{"type":"ping"}"#,
                r#"{"type":"message_start","message":{"usage":{"input_tokens":57,"output_tokens":0}}}"#,
                r#"{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}"#,
                r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"The user asks"}}"#,
                r#"{"type":"content_block_stop","index":0}"#,
                r#"{"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}"#,
                r#"{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"Je regarde."}}"#,
                r#"{"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"toolu_12","name":"heure","input":{}}}"#,
                r#"{"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"{\"ville\":"}}"#,
                r#"{"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"\"Paris\"}"}}"#,
                r#"{"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"input_tokens":331,"output_tokens":48}}"#,
                r#"{"type":"message_stop"}"#,
            ],
        );
        let reply = reply.unwrap();
        assert_eq!(seen, "Je regarde.", "le raisonnement n'est pas affiché");
        assert_eq!(reply.tool_calls.len(), 1);
        assert_eq!(reply.tool_calls[0].id, "toolu_12");
        assert_eq!(reply.tool_calls[0].arguments["ville"], "Paris");
        assert_eq!((reply.usage.prompt_tokens, reply.usage.completion_tokens), (331, 48));
        assert!(!reply.truncated);
    }
}
