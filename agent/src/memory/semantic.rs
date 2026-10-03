//! Embeddings sémantiques via LM Studio (API compatible OpenAI).
//!
//! Le vectoriseur de hachage ([`super::embed`]) rapproche les mots, pas le
//! sens : « voiture » et « véhicule » restent étrangers. Ce module demande de
//! vrais embeddings à LM Studio, déjà lancé pour SynaptiQ, avec le même modèle
//! multilingue (bon en français, 384 dimensions).
//!
//! **LM Studio est facultatif.** S'il ne répond pas, chaque appel renvoie
//! `None` et la mémoire retombe sur le hachage, toujours calculé. Après un
//! échec, on attend [`RETRY_AFTER`] avant de réessayer : sans cela, chaque
//! rappel de mémoire paierait le délai d'attente réseau.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde_json::json;

/// Pause après un échec avant de retenter LM Studio.
const RETRY_AFTER: Duration = Duration::from_secs(60);

pub struct SemanticEmbedder {
    http: reqwest::Client,
    endpoint: String,
    model: String,
    /// Horodatage (s) du dernier échec, 0 = aucun.
    failed_at: AtomicU64,
}

#[derive(Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingItem>,
}

#[derive(Deserialize)]
struct EmbeddingItem {
    embedding: Vec<f32>,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl SemanticEmbedder {
    /// `base_url` du type `http://localhost:1234/v1`. Renvoie `None` si l'URL
    /// ou le modèle sont vides (embeddings sémantiques désactivés).
    pub fn new(base_url: &str, model: &str) -> Option<Self> {
        // « localhost » → 127.0.0.1 : sous Windows, une connexion vers un port
        // fermé de `localhost` tente d'abord IPv6 puis IPv4 et met ~2,3 s à
        // échouer (mesuré : chaque rappel de mémoire coûtait 2,3 s quand LM
        // Studio était éteint). En adresse numérique, le refus est immédiat.
        let base_url = base_url.trim().trim_end_matches('/').replace("://localhost", "://127.0.0.1");
        if base_url.is_empty() || model.trim().is_empty() {
            return None;
        }
        // Délai court : un embedding local prend quelques dizaines de ms ;
        // au-delà, mieux vaut répondre avec le hachage que faire attendre.
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(4))
            .connect_timeout(Duration::from_millis(500))
            .build()
            .ok()?;
        Some(SemanticEmbedder {
            http,
            endpoint: format!("{base_url}/embeddings"),
            model: model.trim().to_string(),
            failed_at: AtomicU64::new(0),
        })
    }

    /// Identifiant du modèle : stocké avec chaque vecteur, pour ne jamais
    /// comparer des vecteurs de deux modèles différents.
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Vecteur normalisé, ou `None` si LM Studio est indisponible.
    pub async fn embed(&self, text: &str) -> Option<Vec<f32>> {
        let failed_at = self.failed_at.load(Ordering::Relaxed);
        if failed_at != 0 && now_secs().saturating_sub(failed_at) < RETRY_AFTER.as_secs() {
            return None;
        }
        match self.request(text).await {
            Ok(vector) => {
                if failed_at != 0 {
                    log::info!("[memory] LM Studio de nouveau disponible");
                }
                self.failed_at.store(0, Ordering::Relaxed);
                Some(vector)
            }
            Err(error) => {
                if failed_at == 0 {
                    log::warn!("[memory] embeddings LM Studio indisponibles, repli sur le hachage : {error}");
                }
                self.failed_at.store(now_secs(), Ordering::Relaxed);
                None
            }
        }
    }

    async fn request(&self, text: &str) -> Result<Vec<f32>, String> {
        let response = self
            .http
            .post(&self.endpoint)
            .json(&json!({ "model": self.model, "input": text }))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !response.status().is_success() {
            return Err(format!("HTTP {}", response.status()));
        }
        let parsed: EmbeddingResponse = response.json().await.map_err(|e| e.to_string())?;
        let mut vector = parsed
            .data
            .into_iter()
            .next()
            .map(|item| item.embedding)
            .filter(|v| !v.is_empty())
            .ok_or_else(|| "réponse sans vecteur".to_string())?;
        // Normalisé une fois pour toutes : la similarité devient un produit
        // scalaire, comme pour le hachage.
        let norm: f32 = vector.iter().map(|v| v * v).sum::<f32>().sqrt();
        if norm > f32::EPSILON {
            for v in vector.iter_mut() {
                *v /= norm;
            }
        }
        Ok(vector)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn localhost_devient_une_adresse_numerique() {
        let e = SemanticEmbedder::new("http://localhost:1234/v1", "modele").unwrap();
        assert_eq!(e.endpoint, "http://127.0.0.1:1234/v1/embeddings");
        // Une autre machine n'est pas touchée.
        let e = SemanticEmbedder::new("http://lm-studio.local:1234/v1/", "modele").unwrap();
        assert_eq!(e.endpoint, "http://lm-studio.local:1234/v1/embeddings");
        assert!(SemanticEmbedder::new("", "modele").is_none());
    }
}
