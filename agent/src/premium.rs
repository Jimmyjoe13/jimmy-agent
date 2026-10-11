//! Intégrité des contenus premium (palettes, modèles 3D).
//!
//! Les fichiers premium vivent dans `<données>/premium/` (livrés après achat,
//! hors du dépôt public). Sans contrôle, on pouvait y modifier une palette
//! ou remplacer un modèle pour se faire un avatar maison. Ils sont donc
//! accompagnés d'un **manifeste signé** :
//!   - `manifest.json` : `{"v":1,"files":{"skins/neon.json":"<sha256>",…}}` ;
//!   - `manifest.sig`  : signature Ed25519 (base64 URL) des octets exacts du
//!     manifeste, avec la clé privée des licences (`crate::licence`).
//!
//! Un skin premium n'est utilisable que si sa palette et les fichiers qu'elle
//! cite (`model`, `rig`) ont l'empreinte du manifeste. Le manifeste est
//! produit par `scripts/premium/sign-manifest.mjs`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ring::signature::{UnparsedPublicKey, ED25519};

pub struct Premium {
    dir: PathBuf,
    public_key: Vec<u8>,
    /// Empreintes déjà calculées : (date, taille) → sha256. Un modèle de
    /// 6 Mo n'est pas relu à chaque vérification.
    hashes: Mutex<HashMap<PathBuf, (SystemTime, u64, String)>>,
}

impl Premium {
    pub fn new(dir: PathBuf) -> Self {
        Self::with_key(dir, crate::licence::public_key())
    }

    pub fn with_key(dir: PathBuf, public_key: Vec<u8>) -> Self {
        Premium { dir, public_key, hashes: Mutex::new(HashMap::new()) }
    }

    /// Manifeste vérifié : chemin relatif → empreinte, ou `None` s'il manque
    /// ou si sa signature est fausse.
    fn manifest(&self) -> Option<HashMap<String, String>> {
        let raw = std::fs::read(self.dir.join("manifest.json")).ok()?;
        let sig_text = std::fs::read_to_string(self.dir.join("manifest.sig")).ok()?;
        let sig = URL_SAFE_NO_PAD.decode(sig_text.trim()).ok()?;
        if UnparsedPublicKey::new(&ED25519, &self.public_key).verify(&raw, &sig).is_err() {
            log::warn!("[premium] signature du manifeste invalide");
            return None;
        }
        let json: serde_json::Value = serde_json::from_slice(&raw).ok()?;
        let files = json.get("files")?.as_object()?;
        Some(files.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_lowercase()))).collect())
    }

    fn sha256(&self, path: &Path) -> Option<String> {
        let meta = std::fs::metadata(path).ok()?;
        let stamp = (meta.modified().ok()?, meta.len());
        if let Some((t, len, hash)) = self.hashes.lock().ok()?.get(path) {
            if (*t, *len) == stamp {
                return Some(hash.clone());
            }
        }
        let bytes = std::fs::read(path).ok()?;
        let digest = ring::digest::digest(&ring::digest::SHA256, &bytes);
        let hash: String = digest.as_ref().iter().map(|b| format!("{b:02x}")).collect();
        if let Ok(mut cache) = self.hashes.lock() {
            cache.insert(path.to_path_buf(), (stamp.0, stamp.1, hash.clone()));
        }
        Some(hash)
    }

    /// Le fichier `rel` (relatif au dossier premium) est-il celui du manifeste ?
    fn intact(&self, manifest: &HashMap<String, String>, rel: &str) -> bool {
        let Some(expected) = manifest.get(rel) else {
            return false;
        };
        self.sha256(&self.dir.join(rel)).is_some_and(|h| &h == expected)
    }

    /// Palette du skin et fichiers qu'elle cite conformes au manifeste signé.
    pub fn skin_intact(&self, skin: &str) -> bool {
        let Some(manifest) = self.manifest() else {
            return false;
        };
        let palette = format!("skins/{skin}.json");
        if !self.intact(&manifest, &palette) {
            log::warn!("[premium] palette « {skin} » absente ou modifiée");
            return false;
        }
        let json: serde_json::Value = match std::fs::read(self.dir.join(&palette)).ok().and_then(|b| serde_json::from_slice(&b).ok()) {
            Some(json) => json,
            None => return false,
        };
        for key in ["model", "rig"] {
            if let Some(name) = json.get(key).and_then(|v| v.as_str()) {
                if !self.intact(&manifest, &format!("models/{name}")) {
                    log::warn!("[premium] {key} de « {skin} » absent ou modifié");
                    return false;
                }
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::rand::SystemRandom;
    use ring::signature::{Ed25519KeyPair, KeyPair};

    fn hash(bytes: &[u8]) -> String {
        ring::digest::digest(&ring::digest::SHA256, bytes).as_ref().iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Dossier premium de test : palette + modèle + manifeste signé.
    fn setup() -> (PathBuf, Ed25519KeyPair) {
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).expect("pkcs8");
        let pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).expect("paire");
        let dir = std::env::temp_dir().join(format!("premium-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("skins")).expect("skins");
        std::fs::create_dir_all(dir.join("models")).expect("models");
        let palette = br#"{"fur":[1,0,0],"model":"m.glb"}"#;
        let model = b"modele-3d";
        std::fs::write(dir.join("skins/x.json"), palette).expect("palette");
        std::fs::write(dir.join("models/m.glb"), model).expect("modele");
        let manifest = format!(
            r#"{{"v":1,"files":{{"skins/x.json":"{}","models/m.glb":"{}"}}}}"#,
            hash(palette),
            hash(model)
        );
        std::fs::write(dir.join("manifest.json"), &manifest).expect("manifeste");
        let sig = pair.sign(manifest.as_bytes());
        std::fs::write(dir.join("manifest.sig"), URL_SAFE_NO_PAD.encode(sig.as_ref())).expect("signature");
        (dir, pair)
    }

    #[test]
    fn contenu_d_origine_accepte() {
        let (dir, pair) = setup();
        let premium = Premium::with_key(dir.clone(), pair.public_key().as_ref().to_vec());
        assert!(premium.skin_intact("x"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn palette_ou_modele_modifie_refuse() {
        let (dir, pair) = setup();
        let premium = Premium::with_key(dir.clone(), pair.public_key().as_ref().to_vec());
        std::fs::write(dir.join("skins/x.json"), br#"{"fur":[0,1,0],"model":"m.glb"}"#).expect("palette");
        assert!(!premium.skin_intact("x"), "palette modifiée");
        let _ = std::fs::remove_dir_all(&dir);

        let (dir, pair) = setup();
        let premium = Premium::with_key(dir.clone(), pair.public_key().as_ref().to_vec());
        std::fs::write(dir.join("models/m.glb"), b"autre-modele").expect("modele");
        assert!(!premium.skin_intact("x"), "modèle remplacé");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn skin_invente_ou_manifeste_falsifie_refuse() {
        let (dir, pair) = setup();
        let public = pair.public_key().as_ref().to_vec();
        let premium = Premium::with_key(dir.clone(), public.clone());
        // Skin ajouté à la main : absent du manifeste.
        std::fs::write(dir.join("skins/maison.json"), br#"{"fur":[0,0,1]}"#).expect("maison");
        assert!(!premium.skin_intact("maison"));
        // Manifeste réécrit pour l'y ajouter : la signature ne suit plus.
        let forged = format!(r#"{{"v":1,"files":{{"skins/maison.json":"{}"}}}}"#, hash(br#"{"fur":[0,0,1]}"#));
        std::fs::write(dir.join("manifest.json"), forged).expect("faux manifeste");
        assert!(!premium.skin_intact("maison"));
        assert!(!premium.skin_intact("x"));
        // Et une autre clé que celle de Jimy refuse tout.
        let (dir2, _) = setup();
        assert!(!Premium::new(dir2.clone()).skin_intact("x"));
        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_dir_all(dir2);
    }
}
