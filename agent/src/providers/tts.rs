//! Synthèse vocale.
//!
//! Fournisseur : **Fish Audio** via OpenRouter
//! (`POST https://openrouter.ai/api/v1/audio/speech`, corps
//! `{model, input, voice, response_format}`).
//!
//! Le provider est un enum et non un trait : à ce jour Fish Audio est le seul
//! backend, et un enum évite une dépendance supplémentaire. Ajouter une voix
//! ne demande qu'une entrée dans [`TtsVoice::PRESETS`] ; ajouter un fournisseur
//! ne demande qu'une variante.
//!
//! Deux règles apprises à l'usage et documentées dans le projet, reprises ici :
//! les retours à la ligne coupent la lecture, donc les lignes sont jointes par
//! des espaces ; et un seul appel par piste avec la voix imposée, sinon Fish
//! choisit une voix au hasard et le timbre change d'une réponse à l'autre.

use std::time::Duration;

use serde::Deserialize;

use crate::error::{Error, Result};

/// Voix françaises proposées. La première est la voix validée pour la lecture
/// de Jimmy (calme, posée) ; la seconde est plus douce et claire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TtsVoice {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
}

impl TtsVoice {
    pub const FEMININE: TtsVoice = TtsVoice {
        id: "5567200c7d8341738f0892bbacd3be3c",
        label: "Féminine",
        description: "Calme et posée — voix validée pour Jimmy",
    };
    pub const CLEMENCE: TtsVoice = TtsVoice {
        id: "a288bdc744da4ad194921adad6863175",
        label: "Clémence",
        description: "Douce et claire",
    };

    pub const PRESETS: &'static [TtsVoice] = &[TtsVoice::FEMININE, TtsVoice::CLEMENCE];

    pub fn from_id(id: &str) -> Option<TtsVoice> {
        TtsVoice::PRESETS.iter().copied().find(|v| v.id == id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TtsProvider {
    FishAudio,
}

#[derive(Debug, Clone)]
pub struct Speech {
    /// Audio prêt à être lu : PCM 16 bits si le fournisseur l'accepte, WAV
    /// sinon.
    pub bytes: Vec<u8>,
    /// Fréquence d'échantillonnage de `bytes`.
    pub sample_rate: u32,
    /// Durée estimée, à partir du débit moyen observé.
    pub estimated_ms: u64,
}

/// Réponse d'une synthèse, avant agrégation des morceaux.
struct AudioPayload {
    bytes: Vec<u8>,
    sample_rate: u32,
}

/// Extrait la fréquence d'un en-tête `audio/pcm;rate=44100;channels=1`.
/// Renvoie 0 si l'en-tête est absent : le lecteur ne rééchantillonne pas.
fn parse_rate(content_type: &str) -> u32 {
    content_type
        .split(';')
        .find_map(|part| {
            let part = part.trim();
            part.strip_prefix("rate=")
                .and_then(|v| v.parse::<u32>().ok())
        })
        .unwrap_or(0)
}

pub struct Tts {
    http: reqwest::Client,
    provider: TtsProvider,
    endpoint: &'static str,
}

impl Tts {
    pub fn new(provider: TtsProvider) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(180))
            .user_agent("jimmy/0.1 (desktop agent)")
            .build()?;
        Ok(Tts {
            http,
            provider,
            endpoint: "https://openrouter.ai/api/v1/audio/speech",
        })
    }

    pub fn provider(&self) -> TtsProvider {
        self.provider
    }

    /// Synthétise `text`. Le texte est d'abord réécrit pour l'oral : les
    /// retours à la ligne sont remplacés par des espaces, ce qui évite les
    /// coupures dans la lecture.
    pub async fn speak(
        &self,
        api_key: &str,
        model: &str,
        voice: &str,
        text: &str,
        chars_per_minute: f32,
    ) -> Result<Speech> {
        if api_key.trim().is_empty() {
            return Err(Error::Tts("clé OPENROUTER_API_KEY absente".into()));
        }
        let prepared = prepare_for_speech(text);
        if prepared.is_empty() {
            return Ok(Speech {
                bytes: Vec::new(),
                sample_rate: 0,
                estimated_ms: 0,
            });
        }

        // Au-delà de cette taille, le fournisseur refuse : on découpe.
        const MAX_CHARS: usize = 4000;
        if prepared.chars().count() <= MAX_CHARS {
            let audio = self.request_once(api_key, model, voice, &prepared).await?;
            let ms = estimate_ms(&prepared, chars_per_minute);
            return Ok(Speech {
                bytes: audio.bytes,
                sample_rate: audio.sample_rate,
                estimated_ms: ms,
            });
        }

        let mut all = Vec::new();
        let mut sample_rate = 0;
        for chunk in split_sentences(&prepared, MAX_CHARS) {
            let audio = self.request_once(api_key, model, voice, &chunk).await?;
            sample_rate = sample_rate.max(audio.sample_rate);
            all.extend(audio.bytes);
        }
        Ok(Speech {
            bytes: all,
            sample_rate,
            estimated_ms: estimate_ms(&prepared, chars_per_minute),
        })
    }

    /// Un appel de synthèse. Retourne les octets et la fréquence annoncée.
    async fn request_once(
        &self,
        api_key: &str,
        model: &str,
        voice: &str,
        text: &str,
    ) -> Result<AudioPayload> {
        let body = serde_json::json!({
            "model": model,
            "input": text,
            "voice": voice,
            // PCM plutôt que MP3 : le modèle ne produit pas de WAV (400), et le
            // PCM arrive déjà dans le format que la carte son consomme — donc
            // aucun décodeur à embarquer.
            "response_format": "pcm",
        });
        let response = self
            .http
            .post(self.endpoint)
            .bearer_auth(api_key)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::Tts(e.to_string()))?;

        let status = response.status();
        if !status.is_success() {
            let raw = response.text().await.unwrap_or_default();
            return Err(Error::Tts(format!("HTTP {status} — {}", sanitize(&raw, 300))));
        }
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        if !content_type.contains("audio") {
            let raw = response.text().await.unwrap_or_default();
            return Err(Error::Tts(format!(
                "réponse non audio ({content_type}) — {}",
                sanitize(&raw, 200)
            )));
        }
        let sample_rate = parse_rate(&content_type);
        let bytes = response
            .bytes()
            .await
            .map_err(|e| Error::Tts(e.to_string()))?;
        Ok(AudioPayload {
            bytes: bytes.to_vec(),
            sample_rate,
        })
    }
}

/// Rend un texte écrit pour l'écran lisible à voix haute.
pub fn prepare_for_speech(text: &str) -> String {
    let text = strip_markdown(text);
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\n' | '\r' | '\t' => out.push(' '),
            c => out.push(c),
        }
    }
    // Double espace : le fournisseur le lit comme une pause.
    let mut cleaned = String::with_capacity(out.len());
    let mut previous_space = false;
    for ch in out.chars() {
        if ch == ' ' {
            if !previous_space {
                cleaned.push(' ');
            }
            previous_space = true;
        } else {
            cleaned.push(ch);
            previous_space = false;
        }
    }
    cleaned.trim().to_string()
}

/// Retire ce qui ne se lit pas à voix haute : blocs de code, balises
/// Markdown (`**`, `#`, `` ` ``, puces), liens et URL. Sans cela, la synthèse
/// prononçait « astérisque astérisque » ou des adresses entières.
pub fn strip_markdown(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_code = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            if !in_code {
                out.push_str("(code affiché à l'écran). ");
            }
            in_code = !in_code;
            continue;
        }
        if in_code {
            continue;
        }
        // Titres et puces : on garde le texte, pas le marqueur.
        let line = trimmed
            .trim_start_matches('#')
            .trim_start_matches(|c| c == '-' || c == '*' || c == '>')
            .trim_start();
        out.push_str(line);
        out.push('\n');
    }
    // Liens [texte](url) → texte ; URL nues → « lien ».
    let mut cleaned = String::with_capacity(out.len());
    let chars: Vec<char> = out.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '[' {
            if let Some(close) = chars[i..].iter().position(|&c| c == ']') {
                let close = i + close;
                if chars.get(close + 1) == Some(&'(') {
                    if let Some(end) = chars[close..].iter().position(|&c| c == ')') {
                        cleaned.extend(&chars[i + 1..close]);
                        i = close + end + 1;
                        continue;
                    }
                }
            }
        }
        cleaned.push(chars[i]);
        i += 1;
    }
    let words: Vec<String> = cleaned
        .split(' ')
        .map(|w| {
            if w.starts_with("http://") || w.starts_with("https://") {
                "lien".to_string()
            } else {
                w.replace("**", "").replace("__", "").replace('`', "")
            }
        })
        .collect();
    words.join(" ")
}

/// Estimation de durée : le débit retenu lors des essais est d'environ
/// 1 000 caractères par minute d'audio.
pub fn estimate_ms(text: &str, chars_per_minute: f32) -> u64 {
    let rate = if chars_per_minute <= 0.0 {
        1000.0
    } else {
        chars_per_minute
    };
    let chars = text.chars().count() as f32;
    ((chars / rate) * 60_000.0) as u64
}

/// Découpe aux fins de phrase, pour ne jamais couper une phrase en deux.
fn split_sentences(text: &str, max_chars: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    for sentence in split_keep_separator(text) {
        if !current.is_empty() && current.chars().count() + sentence.chars().count() > max_chars {
            chunks.push(std::mem::take(&mut current));
        }
        current.push_str(&sentence);
    }
    if !current.trim().is_empty() {
        chunks.push(current);
    }
    chunks
}

fn split_keep_separator(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        current.push(ch);
        if matches!(ch, '.' | '!' | '?' | '…' | ';' | ':') {
            out.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// Retire toute valeur ressemblant à une clé avant de logger une réponse.
///
/// Le format des clés est `sk-` suivi de caractères sans espace ; on masque
/// à partir de ce préfixe jusqu'au premier séparateur.
fn sanitize(text: &str, max: usize) -> String {
    let mut out = String::with_capacity(text.len().min(max));
    let mut rest = text;
    loop {
        let Some(index) = rest.find("sk-") else {
            out.push_str(rest);
            break;
        };
        out.push_str(&rest[..index]);
        out.push_str("***");
        // On saute le début de la clé : préfixe + tout ce qui suit jusqu'au
        // premier caractère de séparation.
        let after = &rest[index + 3..];
        let end = after
            .find(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == ',' || c == '}')
            .unwrap_or(after.len());
        rest = &after[end..];
        if out.len() > max {
            break;
        }
    }
    out.chars().take(max).collect()
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct SpeechResponse {
    #[serde(default)]
    id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_non_lu_a_voix_haute() {
        let texte = "## Résumé
**Trois** points :
- un `fichier`
```rust
fn main() {}
```
Voir [la doc](https://exemple.fr) ou https://x.fr/a";
        let oral = prepare_for_speech(texte);
        assert!(!oral.contains('*') && !oral.contains('#') && !oral.contains('`'), "{oral}");
        assert!(!oral.contains("fn main"), "{oral}");
        assert!(oral.contains("code affiché"), "{oral}");
        assert!(oral.contains("la doc") && !oral.contains("https"), "{oral}");
        assert!(oral.starts_with("Résumé Trois points"), "{oral}");
    }

    #[test]
    fn retours_a_la_ligne_deviennent_des_espaces() {
        assert_eq!(prepare_for_speech("Bonjour.\n\nÇa va ?"), "Bonjour. Ça va ?");
    }

    #[test]
    fn estimation_de_duree_coherente() {
        // 1000 caractères ≈ 60 s à 1000 caractères/minute.
        let text = "a".repeat(1000);
        assert_eq!(estimate_ms(&text, 1000.0), 60_000);
    }

    #[test]
    fn texte_long_decoupe_sur_les_phrases() {
        let sentence = "Cette phrase est assez longue pour être comptée. ";
        let text = sentence.repeat(400);
        let chunks = split_sentences(&text, 1000);
        assert!(chunks.len() > 1);
        for chunk in &chunks {
            assert!(chunk.chars().count() <= 1100);
        }
    }

    #[test]
    fn secrets_absentes_des_erreurs() {
        let message = sanitize("échec avec sk-or-v1-abcdefghijklmnop dans le texte", 200);
        assert!(!message.contains("abcdefghijklmnop"));
    }
}