//! Composants vocaux téléchargés au premier lancement.
//!
//! L'installeur n'embarque pas whisper.cpp ni ses modèles (≈ 65 Mo
//! indispensables, 180 Mo de plus pour le modèle précis) : Jimy les
//! télécharge lui-même, depuis les mêmes sources que `scripts/install.ps1`,
//! dans `<données>/components/whisper/`. Un clone de développement les a
//! déjà : tout est alors un no-op.
//!
//! Chaque fichier est écrit en `.part` puis renommé : une coupure ne laisse
//! jamais un modèle tronqué pris pour un modèle complet. L'archive du serveur
//! est extraite par `tar.exe`, présent dans Windows 10 et 11 (aucune
//! dépendance de plus).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tokio::io::AsyncWriteExt;

use crate::paths::Paths;

/// Archive du serveur whisper.cpp (contient `Release/whisper-server.exe`).
const WHISPER_ZIP: &str = "https://github.com/ggml-org/whisper.cpp/releases/download/b5130/whisper-bin-x64.zip";

/// Un fichier à télécharger : nom affiché, adresse, chemin relatif au dossier
/// whisper, indispensable ou non.
struct Item {
    label: &'static str,
    url: &'static str,
    target: &'static str,
    required: bool,
}

const ITEMS: &[Item] = &[
    Item { label: "serveur whisper.cpp", url: WHISPER_ZIP, target: "Release/whisper-server.exe", required: true },
    Item {
        label: "modèle de reconnaissance",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base-q5_1.bin",
        target: "models/ggml-base-q5_1.bin",
        required: true,
    },
    Item {
        label: "détection de la parole",
        url: "https://huggingface.co/ggml-org/whisper-vad/resolve/main/ggml-silero-v6.2.0.bin",
        target: "models/ggml-silero-v6.2.0.bin",
        required: true,
    },
    // Modèle précis des commandes : facultatif, téléchargé en dernier. Sans
    // lui, la commande retombe sur le modèle du mot d'éveil (`ensure_stt`).
    Item {
        label: "modèle précis (facultatif)",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small-q5_1.bin",
        target: "models/ggml-small-q5_1.bin",
        required: false,
    },
];

/// État visible dans l'interface (onglet Voix).
#[derive(Debug, Clone, Default, Serialize)]
pub struct Progress {
    /// Un téléchargement est en cours.
    pub active: bool,
    /// Fichier en cours (libellé).
    pub current: String,
    pub done_bytes: u64,
    pub total_bytes: u64,
    /// Composants indispensables présents : la voix peut démarrer.
    pub ready: bool,
    /// Dernière erreur, en une phrase.
    pub error: Option<String>,
}

#[derive(Default)]
pub struct Components {
    progress: Mutex<Progress>,
    running: AtomicBool,
}

impl Components {
    /// Les composants indispensables sont-ils présents ?
    pub fn ready(&self, whisper_dir: &Path) -> bool {
        ITEMS.iter().filter(|i| i.required).all(|i| whisper_dir.join(i.target).is_file())
    }

    pub fn progress(&self, whisper_dir: &Path) -> Progress {
        let mut p = self.progress.lock().map(|p| p.clone()).unwrap_or_default();
        p.ready = self.ready(whisper_dir);
        p
    }

    fn update(&self, f: impl FnOnce(&mut Progress)) {
        if let Ok(mut p) = self.progress.lock() {
            f(&mut p);
        }
    }

    /// Télécharge ce qui manque (indispensables d'abord). Un seul appel à la
    /// fois : un second revient tout de suite. Renvoie une erreur si un
    /// composant indispensable n'a pas pu être installé.
    pub async fn ensure(&self, paths: &Paths) -> Result<(), String> {
        let dir = paths.whisper_dir();
        if self.running.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        let result = self.run(&dir).await;
        self.running.store(false, Ordering::SeqCst);
        self.update(|p| {
            p.active = false;
            p.current.clear();
            p.error = result.as_ref().err().cloned();
        });
        result
    }

    async fn run(&self, dir: &Path) -> Result<(), String> {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(20))
            .build()
            .map_err(|e| e.to_string())?;
        for item in ITEMS {
            let target = dir.join(item.target);
            if target.is_file() {
                continue;
            }
            self.update(|p| {
                p.active = true;
                p.current = item.label.to_string();
                p.done_bytes = 0;
                p.total_bytes = 0;
                p.error = None;
            });
            log::info!("[composants] téléchargement : {}", item.label);
            let outcome = if item.url.ends_with(".zip") {
                self.fetch_zip(&client, item.url, dir).await
            } else {
                self.fetch(&client, item.url, &target).await
            };
            match outcome {
                Ok(()) if target.is_file() => log::info!("[composants] {} installé", item.label),
                Ok(()) => {
                    let message = format!("{} introuvable après installation", item.label);
                    if item.required {
                        return Err(message);
                    }
                    log::warn!("[composants] {message}");
                }
                Err(e) => {
                    let message = format!("{} : {e}", item.label);
                    if item.required {
                        log::warn!("[composants] {message}");
                        return Err(message);
                    }
                    log::warn!("[composants] {message} (facultatif)");
                }
            }
        }
        Ok(())
    }

    /// Téléchargement en flux vers `<cible>.part`, puis renommage.
    async fn fetch(&self, client: &reqwest::Client, url: &str, target: &Path) -> Result<(), String> {
        if let Some(parent) = target.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| e.to_string())?;
        }
        let part = part_path(target);
        let mut response = client.get(url).send().await.map_err(|e| e.to_string())?;
        if !response.status().is_success() {
            return Err(format!("HTTP {}", response.status()));
        }
        let total = response.content_length().unwrap_or(0);
        self.update(|p| p.total_bytes = total);
        let mut file = tokio::fs::File::create(&part).await.map_err(|e| e.to_string())?;
        let mut done = 0u64;
        while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
            file.write_all(&chunk).await.map_err(|e| e.to_string())?;
            done += chunk.len() as u64;
            self.update(|p| p.done_bytes = done);
        }
        file.flush().await.map_err(|e| e.to_string())?;
        drop(file);
        if total > 0 && done != total {
            let _ = tokio::fs::remove_file(&part).await;
            return Err(format!("téléchargement incomplet ({done}/{total} octets)"));
        }
        tokio::fs::rename(&part, target).await.map_err(|e| e.to_string())
    }

    /// Archive : téléchargée à côté, extraite dans `dir` par `tar.exe`.
    async fn fetch_zip(&self, client: &reqwest::Client, url: &str, dir: &Path) -> Result<(), String> {
        let zip = dir.join("whisper-bin-x64.zip");
        self.fetch(client, url, &zip).await?;
        let mut command = tokio::process::Command::new("tar");
        command.arg("-xf").arg(&zip).arg("-C").arg(dir);
        #[cfg(windows)]
        {
            // Pas de fenêtre de console qui clignote.
            command.creation_flags(0x08000000);
        }
        let status = command.status().await.map_err(|e| format!("tar : {e}"))?;
        let _ = tokio::fs::remove_file(&zip).await;
        if !status.success() {
            return Err(format!("extraction de l'archive en échec ({status})"));
        }
        Ok(())
    }
}

fn part_path(target: &Path) -> PathBuf {
    let mut name = target.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(".part");
    target.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indispensables_detectes() {
        let dir = std::env::temp_dir().join(format!("composants-{}", uuid::Uuid::new_v4()));
        let components = Components::default();
        assert!(!components.ready(&dir));
        for item in ITEMS.iter().filter(|i| i.required) {
            let path = dir.join(item.target);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("dossier");
            std::fs::write(&path, b"x").expect("fichier");
        }
        // Le modèle précis (facultatif) manque : la voix peut quand même démarrer.
        assert!(components.ready(&dir));
        assert!(components.progress(&dir).ready);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn fichier_partiel_a_cote_de_la_cible() {
        let p = part_path(Path::new(r"C:\x\models\ggml-base-q5_1.bin"));
        assert_eq!(p, PathBuf::from(r"C:\x\models\ggml-base-q5_1.bin.part"));
    }
}
