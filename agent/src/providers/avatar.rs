//! Pilotage de l'avatar Godot par HTTP local.
//!
//! Sens de lecture : **Tauri envoie**, Godot reçoit. Godot expose un petit
//! serveur HTTP (voir `godot/scripts/http_server.gd`) et n'a jamais besoin de
//! savoir qui l'appelle. Si l'avatar n'est pas lancé, l'envoi n'échoue pas :
//! l'agent continue, l'utilisateur le saura par l'interface.

use std::time::Duration;

use crate::core::types::AvatarState;
use crate::error::Result;

/// Skins disponibles (identifiant, libellé). Les palettes vivent dans
/// `godot/scripts/jimmy.gd` (`SKINS`) : garder les deux listes alignées.
pub const SKINS: &[(&str, &str)] = &[
    ("renard", "Renard roux"),
    ("arctique", "Renard arctique"),
    ("fennec", "Fennec"),
    ("ours", "Ours"),
    ("robot", "Robot"),
];

#[derive(Debug, Clone)]
pub struct AvatarClient {
    http: reqwest::Client,
    base_url: String,
}

impl AvatarClient {
    pub fn new(host: &str, port: u16) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_millis(1500))
            .build()?;
        Ok(AvatarClient {
            http,
            base_url: format!("http://{host}:{port}"),
        })
    }

    pub async fn is_up(&self) -> bool {
        self.http
            .get(format!("{}/health", self.base_url))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    async fn post(&self, path: &str, payload: serde_json::Value) -> bool {
        match self
            .http
            .post(format!("{}{}", self.base_url, path))
            .json(&payload)
            .send()
            .await
        {
            Ok(response) => response.status().is_success(),
            Err(error) => {
                log::debug!("[avatar] {} : {}", path, error);
                false
            }
        }
    }

    pub async fn set_state(&self, state: AvatarState, detail: &str) -> bool {
        self.post(
            "/state",
            serde_json::json!({ "state": state.as_str(), "detail": detail }),
        )
        .await
    }

    /// Affiche un texte dans la bulle et déclenche l'animation de parole.
    pub async fn say(&self, text: &str, duration_ms: u64) -> bool {
        self.post(
            "/say",
            serde_json::json!({ "text": text, "duration_ms": duration_ms }),
        )
        .await
    }

    pub async fn set_quality(&self, level: &str) -> bool {
        self.post("/quality", serde_json::json!({ "level": level })).await
    }

    /// Active ou coupe l'esquive du curseur.
    pub async fn set_dodge(&self, enabled: bool) -> bool {
        self.post("/dodge", serde_json::json!({ "enabled": enabled })).await
    }

    /// Change le skin de l'avatar ; Godot reconstruit le personnage.
    pub async fn set_skin(&self, skin: &str) -> bool {
        self.post("/skin", serde_json::json!({ "skin": skin })).await
    }

    pub async fn hide_bubble(&self) -> bool {
        self.post("/hide", serde_json::json!({})).await
    }
}