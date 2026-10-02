//! Client Synaptiq — moteur local de réflexion complémentaire.
//!
//! Synaptiq est **déjà installé** sur la machine (`C:\Users\jimmy\synaptiq`,
//! API locale sur `http://127.0.0.1:8000`). Jimmy ne le remplace pas et ne le
//! recrée pas : il l'interroge.
//!
//! Contrat observé sur l'instance locale (via `apps/mcp/server.py`) :
//!
//! | Route              | Méthode | Charge utile                                       |
//! |--------------------|---------|---------------------------------------------------|
//! | `/v1/retrieve`     | POST    | `{agent_id, query, limit, memory_type?}` → `memories` |
//! | `/v1/context/build`| POST    | `{agent_id, session_id, task, query, constraints}` → `context_packet` |
//! | `/v1/memories`     | POST    | `{agent_id, type, subtype?, content, importance}` |
//! | `/v1/health`       | GET     | état des services                                  |
//!
//! Authentification : `Authorization: Bearer <SYNAPTIQ_API_KEY>`.
//!
//! **Règle de déclenchement** (PLAN §21) : Synaptiq n'est pas appelé à chaque
//! demande. Jimmy ne le consulte que si la demande est volumineuse ou clairement
//! porteuse de contexte (« ma infrastructure », « comme la dernière fois »,
//! « le projet X »). Voir [`should_consult`].

use std::time::Duration;

use serde::Deserialize;

use crate::error::{Error, Result};

#[derive(Debug, Clone)]
pub struct SynaptiqClient {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
    agent_id: String,
}

/// Verdict de la décision « faut-il consulter Synaptiq ? ».
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Usefulness {
    /// Non : demande courte et autonome, la mémoire locale suffit.
    No,
    /// Oui : la demande référence un contexte antérieur.
    Yes,
}

impl SynaptiqClient {
    pub fn new(base_url: &str, api_key: &str, agent_id: &str, timeout_ms: u64) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_millis(timeout_ms))
            .build()?;
        Ok(SynaptiqClient {
            http,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            agent_id: if agent_id.is_empty() { "jimmy".into() } else { agent_id.to_string() },
        })
    }

    pub fn has_key(&self) -> bool {
        !self.api_key.trim().is_empty()
    }

    fn headers(&self) -> reqwest::header::HeaderMap {
        let mut headers = reqwest::header::HeaderMap::new();
        if let Ok(value) = reqwest::header::HeaderValue::from_str(&format!("Bearer {}", self.api_key)) {
            headers.insert(reqwest::header::AUTHORIZATION, value);
        }
        headers
    }

    pub async fn health(&self) -> Result<bool> {
        let response = self
            .http
            .get(format!("{}/v1/health", self.base_url))
            .headers(self.headers())
            .send()
            .await
            .map_err(|e| Error::provider("Synaptiq", e.to_string()))?;
        Ok(response.status().is_success())
    }

    /// Recherche sémantique.
    pub async fn retrieve(&self, query: &str, limit: usize) -> Result<String> {
        let payload = serde_json::json!({
            "agent_id": self.agent_id,
            "query": query,
            "limit": limit,
        });
        let memories: Vec<SynaptiqMemory> = self.post("/v1/retrieve", &payload).await?;
        if memories.is_empty() {
            return Ok("Aucun souvenir pertinent.".into());
        }
        Ok(memories
            .iter()
            .map(|m| format!("- [{}] {}", m.memory_type, m.content))
            .collect::<Vec<_>>()
            .join("\n"))
    }

    /// Paquet de contexte compact, prêt à injecter dans le prompt système.
    pub async fn build_context(&self, task: &str, query: &str, max_tokens: u32) -> Result<String> {
        let payload = serde_json::json!({
            "agent_id": self.agent_id,
            "session_id": "jimmy",
            "task": task,
            "query": query,
            "constraints": {
                "max_tokens": max_tokens,
                "memory_types": ["semantic", "episodic", "procedural", "working", "reflective"],
            },
        });
        let value: serde_json::Value = self.post_raw("/v1/context/build", &payload).await?;
        let estimate = value.get("token_estimate").and_then(|v| v.as_u64()).unwrap_or(0);
        let packet = value.get("context_packet").cloned().unwrap_or(serde_json::Value::Null);
        let body = render_packet(&packet);
        if body.is_empty() {
            return Ok(String::new());
        }
        Ok(format!("Contexte Synaptiq (~{estimate} tokens) :\n{body}"))
    }

    /// Enregistre un souvenir.
    pub async fn remember(&self, content: &str, memory_type: &str, subtype: Option<&str>) -> Result<String> {
        let mut payload = serde_json::json!({
            "agent_id": self.agent_id,
            "type": memory_type,
            "content": content,
            "confidence": 1.0,
            "importance": 0.5,
        });
        if let Some(subtype) = subtype {
            payload["subtype"] = serde_json::Value::String(subtype.to_string());
        }
        let _: serde_json::Value = self.post_raw("/v1/memories", &payload).await?;
        Ok(format!("mémorisé dans Synaptiq : {content}"))
    }

    async fn post<T: for<'de> Deserialize<'de>>(&self, path: &str, payload: &serde_json::Value) -> Result<T> {
        let value: serde_json::Value = self.post_raw(path, payload).await?;
        serde_json::from_value(value).map_err(|e| Error::provider("Synaptiq", e.to_string()))
    }

    async fn post_raw(&self, path: &str, payload: &serde_json::Value) -> Result<serde_json::Value> {
        let response = self
            .http
            .post(format!("{}{}", self.base_url, path))
            .headers(self.headers())
            .json(payload)
            .send()
            .await
            .map_err(|e| Error::provider("Synaptiq", e.to_string()))?;
        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|e| Error::provider("Synaptiq", e.to_string()))?;
        if !status.is_success() {
            return Err(Error::provider(
                "Synaptiq",
                format!("HTTP {status} — {}", truncate(&text, 300)),
            ));
        }
        serde_json::from_str(&text).map_err(|e| Error::provider("Synaptiq", e.to_string()))
    }
}

#[derive(Debug, Deserialize)]
struct SynaptiqMemory {
    #[serde(default)]
    memory_type: String,
    #[serde(default)]
    content: String,
}

/// Rend le paquet de contexte Synaptiq sous forme de lignes lisibles.
fn render_packet(packet: &serde_json::Value) -> String {
    let Some(object) = packet.as_object() else {
        return String::new();
    };
    let mut lines = Vec::new();
    for (section, value) in object {
        let rendered = match value {
            serde_json::Value::Array(items) => items
                .iter()
                .filter_map(|item| {
                    let text = item
                        .get("content")
                        .and_then(|c| c.as_str())
                        .unwrap_or(item.as_str().unwrap_or(""));
                    if text.is_empty() {
                        None
                    } else {
                        Some(format!("  - {text}"))
                    }
                })
                .collect::<Vec<_>>()
                .join("\n"),
            serde_json::Value::String(text) => format!("  - {text}"),
            _ => continue,
        };
        if !rendered.trim().is_empty() {
            lines.push(format!("{section} :\n{rendered}"));
        }
    }
    lines.join("\n")
}

/// Formulation explicite de la règle de déclenchement, pour l'interface et les
/// logs. Volontairement simple et lisible : c'est une heuristique, pas un
/// classifieur.
pub fn should_consult(request: &str, min_chars: u32, has_context_markers: bool) -> Usefulness {
    if has_context_markers {
        return Usefulness::Yes;
    }
    if request.chars().count() as u32 >= min_chars {
        return Usefulness::Yes;
    }
    Usefulness::No
}

/// Marqueurs de référence au contexte antérieur, en français.
const CONTEXT_MARKERS: &[&str] = &[
    "la dernière fois", "la semaine dernière", "hier", "avant-hier", "mon projet", "mes projets",
    "mon infrastructure", "comme d'habitude", "tu te souviens", "souviens-toi", "on avait",
    "precedent", "précédent", "déjà fait", "deja fait", "mon setup", "ma config",
    "configuration actuelle", "ce que j'ai demandé", "ce que nous avons",
];

pub fn has_context_markers(request: &str) -> bool {
    let lowered = request.to_lowercase();
    CONTEXT_MARKERS.iter().any(|marker| lowered.contains(marker))
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
mod tests {
    use super::*;

    #[test]
    fn demande_courte_sans_marqueur_ne_declenche_pas() {
        assert_eq!(should_consult("quelle heure est-il", 180, false), Usefulness::No);
    }

    #[test]
    fn demande_longue_declenche() {
        let long = "x".repeat(200);
        assert_eq!(should_consult(&long, 180, false), Usefulness::Yes);
    }

    #[test]
    fn reference_au_contexte_declenche() {
        assert_eq!(
            should_consult("reprends mon projet comme la dernière fois", 180, true),
            Usefulness::Yes
        );
        assert!(has_context_markers("tu te souviens de mon infra ?"));
        assert!(!has_context_markers("ouvre le dossier X"));
    }
}