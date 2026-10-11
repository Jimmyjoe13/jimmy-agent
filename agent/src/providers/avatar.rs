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

/// Skin appliqué quand le skin demandé n'est pas autorisé.
pub const DEFAULT_SKIN: &str = "renard";

/// Skins premium (identifiant, libellé). Leurs palettes vivent dans
/// `godot/premium/skins/<id>.json` (licence propriétaire, hors MIT) : garder
/// les deux alignés. Débloqués par une licence (`crate::licence`).
/// Un skin premium peut aussi porter un modèle 3D (`"model"` dans son JSON) :
/// le `.glb` vit hors dépôt, dans `<données>/premium/models/`.
pub const PREMIUM_SKINS: &[(&str, &str)] = &[("neon", "Renard Néon"), ("jeva", "JEVA")];

/// Boutique où s'achètent les skins premium.
pub const SHOP_URL: &str = "https://jimy-agent.me";

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

    /// Lapin assistant des tâches de fond : `working` (une tâche tourne en
    /// arrière-plan), `success` / `error` (sa fin), `hidden`.
    pub async fn helper(&self, state: &str) -> bool {
        self.post("/helper", serde_json::json!({ "state": state })).await
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
#[cfg(test)]
mod tests {
    use super::*;

    /// Chaque skin premium proposé a sa palette côté Godot : sinon un achat
    /// débloquerait un skin que l'avatar refuse (« skin inconnu »).
    #[test]
    fn skins_premium_ont_leur_palette_godot() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../godot/premium/skins");
        for (id, _) in PREMIUM_SKINS {
            let file = dir.join(format!("{id}.json"));
            let raw = std::fs::read_to_string(&file).unwrap_or_else(|_| panic!("palette absente : {}", file.display()));
            let json: serde_json::Value = serde_json::from_str(&raw).expect("JSON valide");
            for key in ["fur", "cream", "dark", "shirt", "accent"] {
                assert!(json.get(key).is_some_and(|v| v.as_array().is_some_and(|a| a.len() >= 3)), "{id} : couleur {key}");
            }
        }
        // Un skin premium a son compagnon de tâches de fond (le lapin blanc
        // est réservé aux skins de base, décision du 11 octobre).
        for (id, _) in PREMIUM_SKINS {
            let raw = std::fs::read_to_string(dir.join(format!("{id}.json"))).expect("palette");
            let json: serde_json::Value = serde_json::from_str(&raw).expect("JSON valide");
            let companion = json.get("companion").and_then(|c| c.as_str()).unwrap_or("lapin");
            assert!(["lapin_neon", "sonde"].contains(&companion), "{id} : compagnon « {companion} »");
        }
        // Un identifiant ne peut pas être à la fois de base et premium.
        assert!(PREMIUM_SKINS.iter().all(|(p, _)| !SKINS.iter().any(|(b, _)| b == p)));
        assert!(SKINS.iter().any(|(id, _)| *id == DEFAULT_SKIN));
    }
}
