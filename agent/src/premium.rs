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
//!
//! **Livraison** : les fichiers sont téléchargés depuis le site
//! (`site-jimy/api/premium.js`) en présentant la licence. Le serveur la
//! vérifie et redirige vers un lien temporaire du dépôt privé. Chaque fichier
//! reçu est contrôlé contre le manifeste signé **avant** d'être écrit.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ring::signature::{UnparsedPublicKey, ED25519};

/// Adresse de livraison des fichiers premium (`site-jimy/api/premium.js`).
pub const DELIVERY_URL: &str = "https://jimy-agent.me/api/premium";
/// En-tête qui porte la licence vers le serveur de livraison.
const LICENCE_HEADER: &str = "X-Jimy-Licence";

pub struct Premium {
    dir: PathBuf,
    public_key: Vec<u8>,
    /// Empreintes déjà calculées : (date, taille) → sha256. Un modèle de
    /// 6 Mo n'est pas relu à chaque vérification.
    hashes: Mutex<HashMap<PathBuf, (SystemTime, u64, String)>>,
    /// Un seul téléchargement à la fois (démarrage et activation peuvent se
    /// croiser) : le second trouve ensuite les fichiers en place.
    downloading: tokio::sync::Mutex<()>,
}

fn sha256_hex(bytes: &[u8]) -> String {
    ring::digest::digest(&ring::digest::SHA256, bytes).as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

impl Premium {
    pub fn new(dir: PathBuf) -> Self {
        Self::with_key(dir, crate::licence::public_key())
    }

    pub fn with_key(dir: PathBuf, public_key: Vec<u8>) -> Self {
        Premium { dir, public_key, hashes: Mutex::new(HashMap::new()), downloading: tokio::sync::Mutex::new(()) }
    }

    /// Manifeste vérifié : chemin relatif → empreinte, ou `None` s'il manque
    /// ou si sa signature est fausse.
    fn manifest(&self) -> Option<HashMap<String, String>> {
        let raw = std::fs::read(self.dir.join("manifest.json")).ok()?;
        let sig_text = std::fs::read_to_string(self.dir.join("manifest.sig")).ok()?;
        self.parse_manifest(&raw, &sig_text)
    }

    /// Vérifie la signature puis lit la liste des empreintes.
    fn parse_manifest(&self, raw: &[u8], sig_text: &str) -> Option<HashMap<String, String>> {
        let sig = URL_SAFE_NO_PAD.decode(sig_text.trim()).ok()?;
        if UnparsedPublicKey::new(&ED25519, &self.public_key).verify(raw, &sig).is_err() {
            log::warn!("[premium] signature du manifeste invalide");
            return None;
        }
        let json: serde_json::Value = serde_json::from_slice(raw).ok()?;
        let files = json.get("files")?.as_object()?;
        Some(files.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_lowercase()))).collect())
    }

    /// Télécharge les fichiers du skin (manifeste, palette, modèle, squelette)
    /// avec la licence `licence`. Ne fait rien s'ils sont déjà conformes.
    pub async fn download(&self, licence: &str, skin: &str) -> Result<(), String> {
        self.download_from(DELIVERY_URL, licence, skin).await
    }

    async fn download_from(&self, base: &str, licence: &str, skin: &str) -> Result<(), String> {
        let _one_at_a_time = self.downloading.lock().await;
        if self.skin_intact(skin) {
            return Ok(());
        }
        // Redirections suivies à la main : la licence ne part que vers le
        // site, jamais vers l'hébergeur du lien temporaire.
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(20))
            .timeout(Duration::from_secs(600))
            .build()
            .map_err(|e| e.to_string())?;
        let get = |rel: String| {
            let client = client.clone();
            async move { fetch(&client, base, licence, &rel).await }
        };

        // 1. Manifeste signé : il dit quelles empreintes attendre.
        let manifest_raw = get("manifest.json".into()).await?;
        let manifest_sig = String::from_utf8(get("manifest.sig".into()).await?).map_err(|_| "signature illisible")?;
        let manifest = self
            .parse_manifest(&manifest_raw, &manifest_sig)
            .ok_or("manifeste reçu non signé par Jimy")?;

        // 2. Palette, puis les fichiers qu'elle cite. Rien n'est écrit tant
        // que tout n'est pas reçu et conforme.
        let palette_rel = format!("skins/{skin}.json");
        let mut files: Vec<(String, Vec<u8>)> = Vec::new();
        let palette = self.receive(&manifest, &palette_rel, &get).await?;
        let json: serde_json::Value = serde_json::from_slice(&palette).map_err(|_| "palette illisible")?;
        files.push((palette_rel, palette));
        for key in ["model", "rig"] {
            if let Some(name) = json.get(key).and_then(|v| v.as_str()) {
                let rel = format!("models/{name}");
                // Déjà là et conforme (gros modèle) : pas de nouveau téléchargement.
                if self.intact(&manifest, &rel) {
                    continue;
                }
                let bytes = self.receive(&manifest, &rel, &get).await?;
                files.push((rel, bytes));
            }
        }

        // 3. Écriture : fichiers du skin d'abord, manifeste en dernier.
        files.push(("manifest.json".into(), manifest_raw));
        files.push(("manifest.sig".into(), manifest_sig.into_bytes()));
        for (rel, bytes) in files {
            write_atomic(&self.dir.join(&rel), &bytes).await?;
        }
        if !self.skin_intact(skin) {
            return Err("fichiers reçus non conformes".into());
        }
        log::info!("[premium] skin « {skin} » téléchargé");
        Ok(())
    }

    /// Télécharge `rel` et vérifie son empreinte contre le manifeste.
    async fn receive<F, Fut>(&self, manifest: &HashMap<String, String>, rel: &str, get: &F) -> Result<Vec<u8>, String>
    where
        F: Fn(String) -> Fut,
        Fut: std::future::Future<Output = Result<Vec<u8>, String>>,
    {
        let expected = manifest.get(rel).ok_or_else(|| format!("{rel} absent du manifeste"))?;
        let bytes = get(rel.to_string()).await?;
        if &sha256_hex(&bytes) != expected {
            return Err(format!("{rel} reçu abîmé"));
        }
        Ok(bytes)
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
        let hash = sha256_hex(&bytes);
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

/// GET d'un fichier premium : demande au site avec la licence, puis suit la
/// redirection vers le lien temporaire **sans** la licence.
async fn fetch(client: &reqwest::Client, base: &str, licence: &str, rel: &str) -> Result<Vec<u8>, String> {
    let url = format!("{base}?file={rel}");
    let response = client.get(&url).header(LICENCE_HEADER, licence).send().await.map_err(|e| e.to_string())?;
    let response = if response.status().is_redirection() {
        let location = response
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .ok_or("redirection sans adresse")?;
        // Adresse absolue (GitHub) ou relative au site : les deux acceptées.
        let location = response.url().join(location).map_err(|_| "redirection invalide")?;
        client.get(location).send().await.map_err(|e| e.to_string())?
    } else {
        response
    };
    match response.status().as_u16() {
        200 => {}
        403 => return Err("licence refusée par le serveur".into()),
        code => return Err(format!("{rel} : HTTP {code}")),
    }
    response.bytes().await.map(|b| b.to_vec()).map_err(|e| e.to_string())
}

/// Écrit dans `<cible>.part` puis renomme : jamais de fichier à moitié écrit.
async fn write_atomic(target: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|e| e.to_string())?;
    }
    let mut part = target.as_os_str().to_owned();
    part.push(".part");
    tokio::fs::write(&part, bytes).await.map_err(|e| e.to_string())?;
    tokio::fs::rename(&part, target).await.map_err(|e| e.to_string())
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

    /// Faux serveur de livraison : `/api?file=X` avec la licence « BONNE » →
    /// 302 vers `/raw/X`, qui sert le fichier de `source`. Compte les
    /// requêtes `/raw` qui portent la licence (doivent rester à zéro).
    async fn fake_server(source: PathBuf) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("port");
        let addr = listener.local_addr().expect("adresse");
        let leaks = std::sync::Arc::new(AtomicUsize::new(0));
        let leaks_srv = leaks.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else { return };
                let mut buf = vec![0u8; 8192];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = request.split_whitespace().nth(1).unwrap_or("").to_string();
                let licensed = request.to_lowercase().contains("x-jimy-licence: bonne");
                let (status, location, body) = if let Some(file) = path.strip_prefix("/api?file=") {
                    if licensed {
                        ("302 Found", format!("Location: /raw/{file}\r\n"), Vec::new())
                    } else {
                        ("403 Forbidden", String::new(), Vec::new())
                    }
                } else if let Some(file) = path.strip_prefix("/raw/") {
                    if request.to_lowercase().contains("x-jimy-licence") {
                        leaks_srv.fetch_add(1, Ordering::SeqCst);
                    }
                    match std::fs::read(source.join(file)) {
                        Ok(bytes) => ("200 OK", String::new(), bytes),
                        Err(_) => ("404 Not Found", String::new(), Vec::new()),
                    }
                } else {
                    ("404 Not Found", String::new(), Vec::new())
                };
                let head = format!("HTTP/1.1 {status}\r\n{location}Content-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                let _ = socket.write_all(head.as_bytes()).await;
                let _ = socket.write_all(&body).await;
                let _ = socket.shutdown().await;
            }
        });
        (format!("http://{addr}/api"), leaks)
    }

    #[tokio::test]
    async fn livraison_avec_licence_puis_controle() {
        let (source, pair) = setup();
        let public = pair.public_key().as_ref().to_vec();
        let (base, leaks) = fake_server(source.clone()).await;
        let target = std::env::temp_dir().join(format!("premium-dl-{}", uuid::Uuid::new_v4()));
        let premium = Premium::with_key(target.clone(), public.clone());
        assert!(!premium.skin_intact("x"));

        // Licence refusée : rien n'est écrit.
        assert!(premium.download_from(&base, "MAUVAISE", "x").await.is_err());
        assert!(!target.join("manifest.json").exists());

        // Bonne licence : palette, modèle et manifeste en place et conformes.
        premium.download_from(&base, "BONNE", "x").await.expect("téléchargé");
        assert!(premium.skin_intact("x"));
        assert_eq!(leaks.load(std::sync::atomic::Ordering::SeqCst), 0, "licence envoyée hors du site");

        // Modèle abîmé côté serveur : refusé avant écriture.
        let target2 = std::env::temp_dir().join(format!("premium-dl-{}", uuid::Uuid::new_v4()));
        std::fs::write(source.join("models/m.glb"), b"pas-le-bon").expect("abîmé");
        let premium2 = Premium::with_key(target2.clone(), public);
        let error = premium2.download_from(&base, "BONNE", "x").await.expect_err("refusé");
        assert!(error.contains("abîmé"), "{error}");
        assert!(!target2.join("skins/x.json").exists());
        for dir in [source, target, target2] {
            let _ = std::fs::remove_dir_all(dir);
        }
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
