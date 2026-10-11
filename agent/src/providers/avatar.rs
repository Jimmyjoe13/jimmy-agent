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

/// Skins premium (identifiant, libellé). Leurs palettes vivent **hors du
/// dépôt public** (dépôt privé `jimy-premium`, licence propriétaire), dans
/// `<données>/premium/skins/<id>.json` : garder les deux alignés. Débloqués par une licence (`crate::licence`).
/// Un skin premium peut aussi porter un modèle 3D (`"model"` dans son JSON) :
/// le `.glb` vit hors dépôt, dans `<données>/premium/models/`.
/// Troisième champ : son **mot d'éveil**, le nom du skin (décision du
/// 11 octobre : un skin premium répond à son nom, plus à « Jimy »).
pub const PREMIUM_SKINS: &[(&str, &str, &str)] = &[("neon", "Renard Néon", "néon"), ("jeva", "JEVA", "jeva"), ("ark", "ARK", "ark")];

/// Boutique où s'achètent les skins premium.
pub const SHOP_URL: &str = "https://jimy-agent.me";

/// En-tête qui porte le jeton de l'avatar (voir `godot/scripts/http_server.gd`).
pub const TOKEN_HEADER: &str = "X-Jimy-Token";

#[derive(Debug, Clone)]
pub struct AvatarClient {
    http: reqwest::Client,
    base_url: String,
    /// Jeton exigé par Godot, nouveau à chaque lancement de Jimy (variable
    /// d'environnement JIMMY_GODOT_TOKEN du processus Godot).
    token: String,
}

impl AvatarClient {
    pub fn new(host: &str, port: u16, token: &str) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_millis(1500))
            .build()?;
        Ok(AvatarClient {
            http,
            base_url: format!("http://{host}:{port}"),
            token: token.to_string(),
        })
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    pub async fn is_up(&self) -> bool {
        self.http
            .get(format!("{}/health", self.base_url))
            .header(TOKEN_HEADER, &self.token)
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    /// Un serveur répond-il sur le port, même en refusant (401) ? Un Godot
    /// orphelin d'un lancement précédent a un autre jeton : il répond mais
    /// ne se laisse plus piloter.
    pub async fn port_answers(&self) -> bool {
        self.http.get(format!("{}/health", self.base_url)).send().await.is_ok()
    }

    async fn post(&self, path: &str, payload: serde_json::Value) -> bool {
        match self
            .http
            .post(format!("{}{}", self.base_url, path))
            .header(TOKEN_HEADER, &self.token)
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

    /// La liste des skins premium est cohérente : identifiants distincts des
    /// skins de base, mot d'éveil renseigné, skin par défaut existant.
    #[test]
    fn liste_des_skins_premium_coherente() {
        for (id, label, wake) in PREMIUM_SKINS {
            assert!(!id.is_empty() && !label.is_empty(), "{id}");
            assert!(!wake.trim().is_empty(), "{id} : mot d'éveil vide");
        }
        assert!(PREMIUM_SKINS.iter().all(|(p, _, _)| !SKINS.iter().any(|(b, _)| b == p)));
        assert!(SKINS.iter().any(|(id, _)| *id == DEFAULT_SKIN));
    }

    /// Les palettes premium vivent hors du dépôt public (dépôt privé
    /// `jimy-premium`, copié dans `data/premium/`). Quand elles sont là (la
    /// machine de l'auteur), chaque skin proposé a la sienne et son compagnon
    /// (le lapin blanc est réservé aux skins de base) ; sinon (clone public,
    /// compilation GitHub), rien à vérifier.
    #[test]
    fn skins_premium_ont_leur_palette_si_presentes() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../data/premium/skins");
        if !dir.is_dir() {
            return;
        }
        for (id, _, _) in PREMIUM_SKINS {
            let file = dir.join(format!("{id}.json"));
            let raw = std::fs::read_to_string(&file).unwrap_or_else(|_| panic!("palette absente : {}", file.display()));
            let json: serde_json::Value = serde_json::from_str(&raw).expect("JSON valide");
            for key in ["fur", "cream", "dark", "shirt", "accent"] {
                assert!(json.get(key).is_some_and(|v| v.as_array().is_some_and(|a| a.len() >= 3)), "{id} : couleur {key}");
            }
            let companion = json.get("companion").and_then(|c| c.as_str()).unwrap_or("lapin");
            assert!(["lapin_neon", "sonde", "sonde_ark"].contains(&companion), "{id} : compagnon « {companion} »");
        }
    }
}
