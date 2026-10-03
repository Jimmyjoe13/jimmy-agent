//! Sons d'état : trois courtes répliques dites avec la voix de Jimmy.
//!
//! Plutôt que des fichiers audio versionnés, les sons sont **synthétisés** par
//! le même pipeline que les réponses (Fish Audio via OpenRouter, PCM), avec la
//! voix choisie dans les paramètres. Ils sont mis en cache dans
//! `data/audio/cues/` dès la première utilisation : un seul appel réseau par
//! voix et par réplique, puis une lecture instantanée.
//!
//! Le nom du fichier contient l'identifiant de voix et une empreinte de la
//! phrase : changer de voix ou de texte invalide le cache sans rien purger.
//!
//! Format du cache : 4 octets (fréquence, u32 little-endian) puis le PCM brut.

use std::path::PathBuf;

use crate::error::{Error, Result};
use crate::App;

/// Un son d'état.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cue {
    /// Le wake word a été entendu seul : Jimmy invite à parler.
    Listening,
    /// Une réponse écrite est prête (chat texte).
    Answer,
    /// Le modèle de langage tarde : Jimmy le dit au lieu de se taire.
    Thinking,
    /// La demande a échoué.
    Error,
}

impl Cue {
    /// Sons pré-générés. `Answer` n'en fait plus partie : le chat lit
    /// désormais la réponse elle-même.
    pub const ALL: [Cue; 3] = [Cue::Listening, Cue::Error, Cue::Thinking];

    fn id(self) -> &'static str {
        match self {
            Cue::Listening => "listening",
            Cue::Answer => "answer",
            Cue::Thinking => "thinking",
            Cue::Error => "error",
        }
    }

    /// Texte dit. Volontairement très court : un son d'état ne doit pas
    /// couvrir ce que l'utilisateur s'apprête à dire.
    pub fn phrase(self) -> &'static str {
        match self {
            Cue::Listening => "Oui ?",
            Cue::Answer => "C'est prêt.",
            Cue::Thinking => "Un instant.",
            Cue::Error => "Oups, ça n'a pas marché.",
        }
    }
}

/// Empreinte FNV-1a 32 bits de la phrase (stable d'une version à l'autre,
/// contrairement au `DefaultHasher` de la bibliothèque standard).
fn fingerprint(text: &str) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in text.bytes() {
        hash ^= byte as u32;
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

fn cache_file(app: &App, voice: &str, cue: Cue) -> PathBuf {
    app.paths.audio_dir().join("cues").join(format!(
        "{voice}-{}-{:08x}.pcm",
        cue.id(),
        fingerprint(cue.phrase())
    ))
}

/// Charge un son depuis le cache, ou le synthétise puis le met en cache.
/// Renvoie `(fréquence, pcm)`.
async fn load(app: &App, cue: Cue) -> Result<(u32, Vec<u8>)> {
    let settings = app.settings();
    let path = cache_file(app, &settings.tts.voice, cue);

    if let Ok(raw) = std::fs::read(&path) {
        if raw.len() > 4 {
            let rate = u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]);
            return Ok((rate, raw[4..].to_vec()));
        }
    }

    let speech = app
        .tts
        .speak(
            &app.secrets.openrouter_api_key,
            &settings.tts.model,
            &settings.tts.voice,
            cue.phrase(),
            settings.tts.chars_per_minute,
        )
        .await?;
    if speech.bytes.is_empty() {
        return Err(Error::Tts(format!("son « {} » vide", cue.id())));
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut raw = Vec::with_capacity(speech.bytes.len() + 4);
    raw.extend_from_slice(&speech.sample_rate.to_le_bytes());
    raw.extend_from_slice(&speech.bytes);
    std::fs::write(&path, &raw)?;
    log::info!("[cues] « {} » mis en cache ({} octets)", cue.phrase(), speech.bytes.len());

    Ok((speech.sample_rate, speech.bytes))
}

/// Joue un son d'état et attend la fin de la lecture. Ne renvoie jamais
/// d'erreur : un son manquant ne doit pas interrompre l'agent.
pub async fn play(app: &App, cue: Cue) {
    let settings = app.settings();
    if !settings.tts.enabled || !settings.tts.cues {
        return;
    }
    let (rate, bytes) = match load(app, cue).await {
        Ok(sound) => sound,
        Err(error) => {
            log::warn!("[cues] {cue:?} indisponible : {error}");
            return;
        }
    };
    // `cpal` bloque pendant la lecture : on l'isole du runtime asynchrone.
    let outcome = tokio::task::spawn_blocking(move || super::play_bytes(&bytes, rate)).await;
    if let Ok(Err(error)) = outcome {
        log::warn!("[cues] lecture impossible : {error}");
    }
}

/// Pré-génère les sons manquants, pour que le premier « Oui ? » soit immédiat.
pub async fn warm(app: &App) {
    let settings = app.settings();
    if !settings.tts.enabled || !settings.tts.cues {
        return;
    }
    for cue in Cue::ALL {
        if let Err(error) = load(app, cue).await {
            log::warn!("[cues] pré-génération de {cue:?} impossible : {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empreinte_stable_et_discriminante() {
        // Valeur figée : si elle change, tous les caches existants deviennent
        // orphelins. Ne pas modifier l'algorithme sans raison.
        assert_eq!(fingerprint(""), 0x811c_9dc5);
        assert_ne!(fingerprint(Cue::Answer.phrase()), fingerprint(Cue::Error.phrase()));
    }
}
