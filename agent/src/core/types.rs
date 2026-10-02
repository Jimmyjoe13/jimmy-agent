//! Types partagés par le cœur de l'agent.

use serde::{Deserialize, Serialize};

/// Rôle d'un message dans la conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        }
    }

    pub fn from_str(value: &str) -> Role {
        match value {
            "system" => Role::System,
            "tool" => Role::Tool,
            "assistant" => Role::Assistant,
            _ => Role::User,
        }
    }
}

/// Appel d'outil demandé par le modèle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// Arguments déjà désérialisés (objet JSON attendu par le schéma).
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Message {
            role: Role::System,
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Message {
            role: Role::User,
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Message {
            role: Role::Assistant,
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }

    pub fn tool_result(call_id: impl Into<String>, name: impl Into<String>, content: impl Into<String>) -> Self {
        Message {
            role: Role::Tool,
            content: content.into(),
            tool_calls: None,
            tool_call_id: Some(call_id.into()),
            name: Some(name.into()),
        }
    }

    /// Version OpenAI-compatible.
    pub fn to_wire(&self) -> serde_json::Value {
        let mut map = serde_json::Map::new();
        map.insert("role".into(), serde_json::Value::String(self.role.as_str().into()));
        map.insert("content".into(), serde_json::Value::String(self.content.clone()));
        if let Some(calls) = &self.tool_calls {
            if !calls.is_empty() {
                map.insert(
                    "tool_calls".into(),
                    serde_json::Value::Array(
                        calls
                            .iter()
                            .map(|c| {
                                serde_json::json!({
                                    "id": c.id,
                                    "type": "function",
                                    "function": {
                                        "name": c.name,
                                        "arguments": serde_json::Value::String(
                                            serde_json::to_string(&c.arguments).unwrap_or_default()
                                        ),
                                    },
                                })
                            })
                            .collect(),
                    ),
                );
            }
        }
        if let Some(id) = &self.tool_call_id {
            map.insert("tool_call_id".into(), serde_json::Value::String(id.clone()));
        }
        if let Some(name) = &self.name {
            map.insert("name".into(), serde_json::Value::String(name.clone()));
        }
        serde_json::Value::Object(map)
    }
}

/// Déclaration d'un outil au format JSON Schema.
#[derive(Debug, Clone, Serialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

impl ToolSpec {
    pub fn to_wire(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": self.name,
                "description": self.description,
                "parameters": self.parameters,
            }
        })
    }
}

/// États de l'avatar. L'ensemble est ouvert : ajouter une variante suffit à
/// exposer un nouvel état côté Godot (le mapping y est déjà générique).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AvatarState {
    Idle,
    Listening,
    Thinking,
    Speaking,
    Executing,
    Success,
    Error,
    Waiting,
}

impl AvatarState {
    pub fn as_str(self) -> &'static str {
        match self {
            AvatarState::Idle => "idle",
            AvatarState::Listening => "listening",
            AvatarState::Thinking => "thinking",
            AvatarState::Speaking => "speaking",
            AvatarState::Executing => "executing",
            AvatarState::Success => "success",
            AvatarState::Error => "error",
            AvatarState::Waiting => "waiting",
        }
    }
}

/// Événements émis par l'agent pendant une demande.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AgentEvent {
    /// Changement d'état de l'avatar.
    State { state: AvatarState, detail: String },
    /// Le modèle appelle un outil.
    ToolStart { call_id: String, name: String, arguments: serde_json::Value },
    /// Un outil a rendu.
    ToolEnd {
        call_id: String,
        name: String,
        ok: bool,
        summary: String,
        duration_ms: u64,
    },
    /// Mémoire consultée ou écrite.
    Memory { action: String, detail: String },
    /// Synaptiq consulté ou alimenté.
    Synaptiq { action: String, detail: String },
    /// Un skill a été chargé ou créé.
    Skill { action: String, name: String },
    /// Réponse finale prête.
    Final { text: String },
    /// Erreur non fatale, Jimmy continue.
    Notice { message: String },
    /// Erreur fatale pour cette demande.
    Failed { message: String },
}

/// Réponse complète d'une demande.
#[derive(Debug, Clone, Serialize)]
pub struct AgentAnswer {
    pub session_id: String,
    pub text: String,
    pub tools_used: Vec<String>,
    pub duration_ms: u64,
}