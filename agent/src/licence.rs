//! Licences des contenus premium (skins payants).
//!
//! Une licence est un texte `JIMY-<charge>.<signature>` : la charge (JSON en
//! base64 URL, `{"v":1,"skin":"neon",…}`) dit ce qu'elle débloque, la
//! signature Ed25519 est faite par le site jimy-agent.me avec une clé privée
//! qui ne quitte jamais le serveur. L'appli ne contient que la clé publique :
//! elle vérifie **hors ligne**, et le code source public ne permet pas de
//! fabriquer une licence.
//!
//! Skin `*` = tous les skins premium (licence du propriétaire, émise à la
//! main avec la même clé privée : pas de porte dérobée dans le code).
//!
//! Les licences activées vivent dans `data/licences.json` (liste des clés).
//! Une clé de licence n'est jamais écrite au journal : seul le skin l'est.

use std::path::PathBuf;
use std::sync::Mutex;

use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use ring::signature::{UnparsedPublicKey, ED25519};

/// Clé publique Ed25519 (32 octets bruts) des licences. La clé privée est
/// sur Vercel (`LICENSE_PRIVATE_KEY`) et dans le coffre de l'auteur.
const PUBLIC_KEY_B64: &str = "i83kv9ubcdp4tjL3mZClpvt5qpbwS9dssF0prxk48Do=";
const PREFIX: &str = "JIMY-";
/// Skin d'une licence qui débloque tous les skins premium.
pub const ALL_SKINS: &str = "*";
/// Seule version de charge connue : une autre est refusée plutôt que mal lue.
const VERSION: u64 = 1;

/// Ce qu'une licence valide débloque.
#[derive(Debug, Clone, PartialEq)]
pub struct Licence {
    /// Identifiant du skin (`neon`), ou [`ALL_SKINS`].
    pub skin: String,
}

impl Licence {
    pub fn unlocks(&self, skin: &str) -> bool {
        self.skin == ALL_SKINS || self.skin == skin
    }
}

/// Message unique pour toute clé refusée : inutile d'aider à deviner pourquoi.
fn invalid() -> String {
    "clé de licence invalide".to_string()
}

/// Clé publique Ed25519 de Jimy (licences et manifeste premium).
pub(crate) fn public_key() -> Vec<u8> {
    // Constante du code : un échec ici est une erreur de compilation déguisée,
    // couverte par le test `cle_publique_lisible`.
    STANDARD.decode(PUBLIC_KEY_B64).unwrap_or_default()
}

/// Vérifie une clé avec la clé publique de Jimy.
pub fn verify(key: &str) -> Result<Licence, String> {
    verify_with(key, &public_key())
}

/// Vérifie une clé avec une clé publique donnée (tests).
pub fn verify_with(key: &str, public_key: &[u8]) -> Result<Licence, String> {
    let body = key.trim().strip_prefix(PREFIX).ok_or_else(invalid)?;
    let (payload_b64, sig_b64) = body.split_once('.').ok_or_else(invalid)?;
    let payload = URL_SAFE_NO_PAD.decode(payload_b64).map_err(|_| invalid())?;
    let signature = URL_SAFE_NO_PAD.decode(sig_b64).map_err(|_| invalid())?;
    UnparsedPublicKey::new(&ED25519, public_key)
        .verify(&payload, &signature)
        .map_err(|_| invalid())?;
    // Signature bonne : la charge vient du site, on peut la lire.
    let json: serde_json::Value = serde_json::from_slice(&payload).map_err(|_| invalid())?;
    if json.get("v").and_then(|v| v.as_u64()) != Some(VERSION) {
        return Err(invalid());
    }
    let skin = json.get("skin").and_then(|s| s.as_str()).unwrap_or("").trim().to_string();
    if skin.is_empty() {
        return Err(invalid());
    }
    Ok(Licence { skin })
}

/// Licences activées sur cette machine.
pub struct Licences {
    /// (clé telle que saisie, ce qu'elle débloque).
    active: Mutex<Vec<(String, Licence)>>,
    /// Fichier où elles survivent au redémarrage (`None` : en mémoire, tests).
    store: Option<PathBuf>,
    public_key: Vec<u8>,
}

impl Licences {
    /// Charge `path` (absent ou illisible : aucune licence). Une clé qui ne
    /// se vérifie plus (fichier modifié à la main) est ignorée.
    pub fn load(path: PathBuf) -> Self {
        Self::load_with(Some(path), public_key())
    }

    pub fn load_with(path: Option<PathBuf>, public_key: Vec<u8>) -> Self {
        let keys: Vec<String> = path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default();
        let mut active = Vec::new();
        for key in keys {
            match verify_with(&key, &public_key) {
                Ok(licence) => active.push((key, licence)),
                Err(_) => log::warn!("[licence] clé enregistrée refusée (ignorée)"),
            }
        }
        if !active.is_empty() {
            let skins: Vec<&str> = active.iter().map(|(_, l)| l.skin.as_str()).collect();
            log::info!("[licence] skins premium débloqués : {}", skins.join(", "));
        }
        Licences { active: Mutex::new(active), store: path, public_key }
    }

    /// Vérifie puis enregistre une clé. Renvoie ce qu'elle débloque.
    pub fn activate(&self, key: &str) -> Result<Licence, String> {
        let key = key.trim().to_string();
        let licence = verify_with(&key, &self.public_key)?;
        {
            let mut active = self.active.lock().map_err(|_| "licences indisponibles".to_string())?;
            if !active.iter().any(|(k, _)| *k == key) {
                active.push((key, licence.clone()));
            }
        }
        self.save()?;
        log::info!("[licence] activée pour le skin « {} »", licence.skin);
        Ok(licence)
    }

    /// Le skin premium est-il débloqué par une licence activée ?
    pub fn unlocks(&self, skin: &str) -> bool {
        self.active.lock().is_ok_and(|active| active.iter().any(|(_, l)| l.unlocks(skin)))
    }

    /// Clé à présenter au serveur pour télécharger les fichiers du skin :
    /// la licence de ce skin de préférence, sinon une licence « tous ».
    pub fn key_for(&self, skin: &str) -> Option<String> {
        let active = self.active.lock().ok()?;
        active
            .iter()
            .find(|(_, l)| l.skin == skin)
            .or_else(|| active.iter().find(|(_, l)| l.unlocks(skin)))
            .map(|(k, _)| k.clone())
    }

    fn save(&self) -> Result<(), String> {
        let Some(path) = &self.store else {
            return Ok(());
        };
        let keys: Vec<String> =
            self.active.lock().map(|a| a.iter().map(|(k, _)| k.clone()).collect()).unwrap_or_default();
        let raw = serde_json::to_string_pretty(&keys).map_err(|e| e.to_string())?;
        std::fs::write(path, raw).map_err(|e| format!("licence non enregistrée : {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::rand::SystemRandom;
    use ring::signature::{Ed25519KeyPair, KeyPair};

    /// Paire de test (jamais la vraie clé privée) et une clé signée avec.
    fn keypair() -> Ed25519KeyPair {
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).expect("pkcs8");
        Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).expect("paire")
    }

    fn sign(pair: &Ed25519KeyPair, payload: &str) -> String {
        let sig = pair.sign(payload.as_bytes());
        format!("{PREFIX}{}.{}", URL_SAFE_NO_PAD.encode(payload), URL_SAFE_NO_PAD.encode(sig.as_ref()))
    }

    #[test]
    fn cle_publique_lisible() {
        assert_eq!(public_key().len(), 32);
    }

    #[test]
    fn licence_signee_acceptee() {
        let pair = keypair();
        let key = sign(&pair, r#"{"v":1,"skin":"neon","ref":"abc","ts":1}"#);
        let licence = verify_with(&format!("  {key}\n"), pair.public_key().as_ref()).expect("valide");
        assert_eq!(licence.skin, "neon");
        assert!(licence.unlocks("neon"));
        assert!(!licence.unlocks("metal"));
    }

    #[test]
    fn charge_modifiee_refusee() {
        let pair = keypair();
        let key = sign(&pair, r#"{"v":1,"skin":"neon"}"#);
        // Même signature, charge changée pour débloquer tout : refusé.
        let (_, sig) = key.split_once('.').expect("point");
        let forged = format!("{PREFIX}{}.{sig}", URL_SAFE_NO_PAD.encode(r#"{"v":1,"skin":"*"}"#));
        assert!(verify_with(&forged, pair.public_key().as_ref()).is_err());
    }

    #[test]
    fn autre_cle_privee_refusee() {
        let key = sign(&keypair(), r#"{"v":1,"skin":"neon"}"#);
        assert!(verify_with(&key, keypair().public_key().as_ref()).is_err());
        // Et la vraie clé publique de Jimy la refuse aussi.
        assert!(verify(&key).is_err());
    }

    #[test]
    fn formats_invalides_refuses() {
        let pair = keypair();
        let public = pair.public_key().as_ref().to_vec();
        for key in ["", "JIMY-", "JIMY-abc", "neon", "JIMY-!!.??"] {
            assert!(verify_with(key, &public).is_err(), "{key}");
        }
        // Signature valide mais version ou skin absents.
        assert!(verify_with(&sign(&pair, r#"{"v":2,"skin":"neon"}"#), &public).is_err());
        assert!(verify_with(&sign(&pair, r#"{"v":1,"skin":""}"#), &public).is_err());
    }

    #[test]
    fn licence_tous_skins() {
        let pair = keypair();
        let licence = verify_with(&sign(&pair, r#"{"v":1,"skin":"*"}"#), pair.public_key().as_ref()).expect("valide");
        assert!(licence.unlocks("neon") && licence.unlocks("fantome"));
    }

    #[test]
    fn activation_persistee_et_rechargee() {
        let pair = keypair();
        let public = pair.public_key().as_ref().to_vec();
        let path = std::env::temp_dir().join(format!("licences-{}.json", uuid::Uuid::new_v4()));
        let licences = Licences::load_with(Some(path.clone()), public.clone());
        assert!(!licences.unlocks("neon"));
        assert!(licences.activate("JIMY-faux.faux").is_err());
        let key = sign(&pair, r#"{"v":1,"skin":"neon"}"#);
        licences.activate(&key).expect("activée");
        licences.activate(&key).expect("deuxième fois sans doublon");
        assert!(licences.unlocks("neon"));
        // Rechargement : la licence survit, une clé ajoutée à la main est ignorée.
        let raw = std::fs::read_to_string(&path).expect("fichier");
        let mut keys: Vec<String> = serde_json::from_str(&raw).expect("json");
        assert_eq!(keys.len(), 1);
        keys.push("JIMY-bidon.bidon".into());
        std::fs::write(&path, serde_json::to_string(&keys).expect("json")).expect("écriture");
        let reloaded = Licences::load_with(Some(path.clone()), public);
        assert!(reloaded.unlocks("neon"));
        assert!(!reloaded.unlocks("metal"));
        // Clé à présenter au serveur : celle du skin, rien pour un autre.
        assert_eq!(reloaded.key_for("neon").as_deref(), Some(key.as_str()));
        assert_eq!(reloaded.key_for("metal"), None);
        let _ = std::fs::remove_file(path);
    }
}
