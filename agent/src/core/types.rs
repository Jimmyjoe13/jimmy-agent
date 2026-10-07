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

/// Image jointe à une demande de l'utilisateur (capture de la fenêtre
/// active, vision du 7 octobre). Jamais sérialisée : ni dans l'historique, ni
/// dans les journaux de requêtes — seule une note « (capture jointe : …) »
/// reste dans le texte du message.
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    /// `image/jpeg` ou `image/png`.
    pub media_type: String,
    /// Contenu encodé en base64.
    pub base64: String,
    /// Ce que montre l'image (titre de la fenêtre) : pour la note de
    /// l'historique et pour l'interface.
    pub label: String,
}

impl Image {
    /// URL `data:` (formats Chat et Responses).
    pub fn data_url(&self) -> String {
        format!("data:{};base64,{}", self.media_type, self.base64)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
    /// Images jointes (messages de l'utilisateur seulement). Hors
    /// sérialisation : une capture d'écran ne doit jamais aller sur disque.
    #[serde(skip)]
    pub images: Vec<Image>,
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
            images: Vec::new(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Message {
            role: Role::User,
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            images: Vec::new(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Message {
            role: Role::Assistant,
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            images: Vec::new(),
        }
    }

    pub fn tool_result(call_id: impl Into<String>, name: impl Into<String>, content: impl Into<String>) -> Self {
        Message {
            role: Role::Tool,
            content: content.into(),
            tool_calls: None,
            tool_call_id: Some(call_id.into()),
            name: Some(name.into()),
            images: Vec::new(),
        }
    }

    /// Demande de l'utilisateur avec des images jointes.
    pub fn user_with_images(content: impl Into<String>, images: Vec<Image>) -> Self {
        let mut message = Message::user(content);
        message.images = images;
        message
    }

    /// Version OpenAI-compatible.
    pub fn to_wire(&self) -> serde_json::Value {
        let mut map = serde_json::Map::new();
        map.insert("role".into(), serde_json::Value::String(self.role.as_str().into()));
        // Avec image : contenu en parties (texte puis images), forme validée
        // par la sonde du 7 octobre (mimo-v2.6-flash, glm-5.3-flash).
        let content = if self.images.is_empty() {
            serde_json::Value::String(self.content.clone())
        } else {
            let mut parts = vec![serde_json::json!({ "type": "text", "text": self.content })];
            parts.extend(
                self.images
                    .iter()
                    .map(|image| serde_json::json!({ "type": "image_url", "image_url": { "url": image.data_url() } })),
            );
            serde_json::Value::Array(parts)
        };
        map.insert("content".into(), content);
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
        // Le `name` du résultat d'outil n'est PAS envoyé : l'upstream le refuse
        // (« messages[N]: "name" is not supported by this endpoint », HTTP 400
        // en usage réel le 5 octobre, conversation coupée en pleine procédure).
        // Il est de toute façon redondant : `tool_call_id` relie le résultat à
        // son appel. Le champ reste dans la structure pour l'affichage.
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
// `rename_all_fields` : sans lui, `call_id` et `duration_ms` partaient en
// snake_case alors que l'interface lit `callId` et `durationMs` — les durées
// des outils n'apparaissaient jamais.
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
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
    /// Vault Obsidian consulté ou alimenté.
    Vault { action: String, detail: String },
    /// Un skill a été chargé ou créé.
    Skill { action: String, name: String },
    /// Réponse finale prête.
    Final { text: String },
    /// Fragment de la réponse, reçu en flux pendant la génération : la bulle
    /// du Chat s'écrit en direct au lieu d'attendre la fin de l'appel (mesuré
    /// à 22 s de silence). Transitoire : `Final` fournit toujours le texte
    /// complet et remplace la bulle. Ne doit pas être lu à voix haute ni
    /// historisé — seuls `Final` et les outils le sont.
    Delta { text: String },
    /// Un outil veut modifier un fichier sensible (`.env`, clés, secrets) :
    /// il est suspendu jusqu'à la réponse de l'utilisateur
    /// (`approval_respond`). `detail` = l'outil et la commande exacte.
    Approval { id: String, target: String, detail: String },
    /// La demande est close : accord, refus ou délai dépassé.
    ApprovalResolved { id: String, approved: bool },
    /// Erreur non fatale, Jimmy continue.
    Notice { message: String },
    /// Commande dite à voix haute, telle que transmise à l'agent : affichée
    /// dans le chat comme un message de l'utilisateur.
    /// `session_id` : la session vocale, que le Chat adopte pour afficher la
    /// conversation et y rattacher un projet (sans lui, le projet choisi dans
    /// le Chat pendant une conversation vocale se perdait).
    Spoken { text: String, session_id: String },
    /// Où en est une longue tâche (« Je lis config.py. »), dit à voix haute de
    /// temps en temps pendant une conversation vocale : une tâche de code prend
    /// plusieurs minutes, le silence ressemblait à une panne.
    Progress { text: String },
    /// Où en est l'écoute, pour que l'utilisateur sache toujours ce que Jimmy
    /// attend de lui. `phase` : `idle` (en veille), `capturing` (je t'entends),
    /// `transcribing`, `your_turn` (à toi, `remaining` ms pour commencer),
    /// `thinking`, `speaking`.
    Listen { phase: String, remaining: u64 },
    /// Ce que l'écoute a transcrit (fenêtre du wake word). `matched` : le mot
    /// d'activation y a été reconnu. Sert au retour visuel de la vue Voix.
    Heard { text: String, matched: bool },
    /// Erreur fatale pour cette demande.
    Failed { message: String },
    /// La tâche passe en arrière-plan (voir `tasks`) : le premier plan est
    /// libre — le Chat débloque la saisie, la voix se remet à écouter. Tous
    /// ses événements suivants arrivent enveloppés dans `Background`.
    Detached { task_id: String, session_id: String, title: String },
    /// Événement d'une tâche de fond, à ne pas mélanger au tour en cours du
    /// premier plan (sa réponse finale va dans sa session, avec une annonce).
    Background { task_id: String, session_id: String, event: Box<AgentEvent> },
}

/// Réponse complète d'une demande.
#[derive(Debug, Clone, Serialize)]
pub struct AgentAnswer {
    pub session_id: String,
    pub text: String,
    pub tools_used: Vec<String>,
    pub duration_ms: u64,
}

#[cfg(test)]
mod wire_tests {
    use super::*;

    /// Le nom du résultat d'outil ne part plus sur le fil : l'upstream le
    /// refuse en HTTP 400 ("name" is not supported by this endpoint, usage
    /// réel du 5 octobre, conversation coupée en pleine procédure).
    /// tool_call_id reste, lui : c'est lui qui relie le résultat à son appel.
    #[test]
    fn le_resultat_d_outil_n_envoie_pas_son_nom() {
        let message = Message::tool_result("call_7", "vault_search", "notes trouvées");
        let texte = message.to_wire().to_string();
        assert!(texte.contains("tool_call_id"), "{texte}");
        assert!(!texte.contains("\"name\""), "{texte}");
    }
}
