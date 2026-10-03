//! Reconnaissance vocale locale : **whisper.cpp**.
//!
//! Choix justifié par le PLAN (« traitement local, faible latence, bonne
//! qualité française, faible consommation ») : l'audio ne quitte jamais la
//! machine, et whisper.cpp est la solution locale de référence.
//!
//! Jimmy lance `whisper-server.exe` comme processus annexe et lui envoie des
//!amples WAV via `POST /inference`. Un serveur plutôt qu'un appel de CLI à
//! chaque fois : le modèle reste chargé en mémoire (chargement une fois pour
//! toutes, ~80 ms) au lieu d'être relu à chaque mot.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::Deserialize;
use tokio::process::{Child, Command};

use crate::error::{Error, Result};

/// Modèles proposés, du plus rapide au plus précis.
pub const MODELS: &[(&str, &str, &str)] = &[
    ("ggml-tiny-q5_1.bin", "Rapide", "Le plus réactif, le moins précis"),
    ("ggml-base-q5_1.bin", "Équilibré", "Bon compromis pour le français"),
    ("ggml-small-q5_1.bin", "Précis", "Le meilleur français, ~3 s de plus"),
];

pub struct Stt {
    http: reqwest::Client,
    endpoint: String,
    language: String,
    /// Texte de contexte passé à Whisper (`prompt`). Y mettre le nom de
    /// l'assistant l'oriente vers la bonne orthographe : sans lui, « Jimmy »
    /// sortait en « Guimmi » ou « J'y mise ».
    prompt: String,
    child: Option<Child>,
}

#[derive(Debug, Deserialize)]
struct Inference {
    #[serde(default)]
    text: String,
}

impl Stt {
    pub fn new(port: u16, language: &str) -> Result<Self> {
        // Aucune connexion gardée ouverte : whisper-server ferme les siennes
        // après un court délai, et réutiliser une connexion fermée faisait
        // échouer la première requête après chaque pause (« error sending
        // request », mesuré dans le journal) — d'où une réparation complète
        // des serveurs et plusieurs secondes perdues.
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .pool_max_idle_per_host(0)
            .user_agent("jimmy/0.1 (desktop agent)")
            .build()?;
        Ok(Stt {
            http,
            endpoint: format!("http://127.0.0.1:{port}/inference"),
            language: language.to_string(),
            prompt: String::new(),
            child: None,
        })
    }

    /// Contexte de transcription (voir le champ `prompt`).
    pub fn with_prompt(mut self, prompt: &str) -> Self {
        self.prompt = prompt.trim().to_string();
        self
    }

    pub fn set_language(&mut self, language: &str) {
        self.language = language.to_string();
    }

    /// Démarre `whisper-server.exe` s'il ne répond pas déjà.
    /// `audio_ctx` : contexte audio de Whisper, en trames de 20 ms (1500 =
    /// 30 s, valeur d'origine ; 0 = ne pas le réduire). Whisper traite toujours
    /// une fenêtre complète, même pour une phrase de 2 s : réduire le contexte
    /// divise le temps d'inférence. Mesuré sur 8 phrases (modèle `small`) :
    /// 4,0 s → 1,1 s (512) ou 1,7 s (768), erreur de mots identique (~20 %).
    pub async fn ensure_server(&mut self, exe: &Path, model: &Path, threads: u32, audio_ctx: u32) -> Result<()> {
        if self.health().await {
            log::info!("[stt] whisper-server déjà actif sur le port{}", self.port());
            return Ok(());
        }
        if !exe.is_file() {
            return Err(Error::Stt(format!(
                "whisper-server introuvable : {}",
                exe.display()
            )));
        }
        if !model.is_file() {
            return Err(Error::Stt(format!("modèle introuvable : {}", model.display())));
        }
        let mut command = Command::new(exe);
        command
            .arg("-m")
            .arg(model)
            .arg("--host")
            .arg("127.0.0.1")
            .arg("--port")
            .arg(self.port().to_string())
            .arg("-l")
            .arg(&self.language)
            .arg("--vad")
            .arg("--vad-model")
            .arg(vad_model_for(model))
            .arg("-t")
            .arg(if threads == 0 { num_cpus() } else { threads }.to_string());
        if audio_ctx > 0 {
            command.arg("-ac").arg(audio_ctx.to_string());
        }
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        // Windows : masquer la fenêtre de la console du processus annexe.
        #[cfg(windows)]
        {
            command.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        self.child = Some(
            command
                .spawn()
                .map_err(|e| Error::Stt(format!("démarrage impossible : {e}")))?,
        );

        for _ in 0..80 {
            if self.health().await {
                log::info!("[stt] whisper-server prêt ({} threads)", num_cpus().max(1));
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        Err(Error::Stt(
            "whisper-server a démarré mais ne répond pas".into(),
        ))
    }

    fn port(&self) -> u16 {
        self.endpoint
            .split(':')
            .nth(2)
            .and_then(|p| p.split('/').next())
            .and_then(|p| p.parse().ok())
            .unwrap_or(8178)
    }

    pub async fn health(&self) -> bool {
        let port = self.port();
        let url = format!("http://127.0.0.1:{port}/health");
        self.http
            .get(url)
            .timeout(Duration::from_millis(800))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    /// Transcrit un buffer WAV PCM 16 bits mono 16 kHz.
    pub async fn transcribe_wav(&self, wav: Vec<u8>) -> Result<String> {
        let part = reqwest::multipart::Part::bytes(wav)
            .file_name("capture.wav")
            .mime_str("audio/wav")?;
        let form = reqwest::multipart::Form::new()
            .part("file", part)
            .text("temperature", "0.0")
            .text("language", self.language.clone())
            .text("response_format", "json");
        let form = if self.prompt.is_empty() {
            form
        } else {
            form.text("prompt", self.prompt.clone())
        };
        let response = self
            .http
            .post(&self.endpoint)
            .multipart(form)
            .send()
            .await
            .map_err(|e| Error::Stt(e.to_string()))?;
        if !response.status().is_success() {
            return Err(Error::Stt(format!("HTTP {}", response.status())));
        }
        let parsed: Inference = response
            .json()
            .await
            .map_err(|e| Error::Stt(format!("réponse illisible : {e}")))?;
        Ok(clean_transcript(&parsed.text))
    }

    pub async fn shutdown(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill().await;
        }
    }
}

/// Phrases que Whisper « invente » sur du bruit ou du silence : elles
/// viennent de ses données d'entraînement (sous-titres de vidéos), pas de ce
/// que l'utilisateur a dit.
const HALLUCINATIONS: &[&str] = &[
    "amara.org",
    "sous-titr",
    "sous titr",
    "merci d'avoir regardé",
    "merci d’avoir regardé",
    "abonnez-vous",
    "abonne-toi",
    "n'oubliez pas de vous abonner",
];

/// Nettoie une transcription : retire les annotations de Whisper (`[BLANK_AUDIO]`,
/// `(musique)`, `*bruit*`, notes de musique) et les phrases inventées sur du
/// bruit. Renvoie une chaîne vide s'il ne reste rien d'un propos réel.
pub fn clean_transcript(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut closing: Option<char> = None;
    for ch in text.chars() {
        match (closing, ch) {
            (None, '[') => closing = Some(']'),
            (None, '(') => closing = Some(')'),
            (None, '*') => closing = Some('*'),
            (Some(close), c) if c == close => closing = None,
            (Some(_), _) => {}
            (None, '♪' | '♫') => {}
            (None, c) => out.push(c),
        }
    }
    // Annotation jamais refermée : on ne jette pas tout le reste de la phrase.
    let base = if closing.is_some() {
        text.chars().filter(|c| !matches!(c, '♪' | '♫')).collect::<String>()
    } else {
        out
    };
    let cleaned = base.split_whitespace().collect::<Vec<_>>().join(" ");
    let lower = cleaned.to_lowercase();
    if HALLUCINATIONS.iter().any(|h| lower.contains(h)) {
        return String::new();
    }
    if !cleaned.chars().any(|c| c.is_alphanumeric()) {
        return String::new();
    }
    cleaned
}

fn num_cpus() -> u32 {
    std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(4)
}

/// Cherche le modèle silero utilisé par `--vad`, à côté du modèle Whisper.
fn vad_model_for(whisper_model: &Path) -> PathBuf {
    if let Some(dir) = whisper_model.parent() {
        let candidate = dir.join("ggml-silero-v6.2.0.bin");
        if candidate.is_file() {
            return candidate;
        }
    }
    PathBuf::from("ggml-silero-v6.2.0.bin")
}

/// Emballe des échantillons PCM 16 bits mono dans un conteneur WAV.
pub fn pcm_to_wav(samples: &[i16], sample_rate: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // taille du bloc fmt
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // octets/seconde
    out.extend_from_slice(&2u16.to_le_bytes()); // alignement
    out.extend_from_slice(&16u16.to_le_bytes()); // bits par échantillon
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        out.extend_from_slice(&sample.to_le_bytes());
    }
    out
}

/// Rééchantillonnage naïf parInterpolation linéaire.
/// Suffisant pour passer du 44,1 / 48 kHz au 16 kHz attendu par Whisper.
pub fn resample(input: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == to_rate || input.is_empty() {
        return input.to_vec();
    }
    let ratio = to_rate as f64 / from_rate as f64;
    let out_len = (input.len() as f64 * ratio).round() as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let position = i as f64 / ratio;
        let index = position.floor() as usize;
        let frac = (position - index as f64) as f32;
        let a = input[index.min(input.len() - 1)];
        let b = input[(index + 1).min(input.len() - 1)];
        out.push(a + (b - a) * frac);
    }
    out
}

pub fn float_to_i16(samples: &[f32]) -> Vec<i16> {
    samples
        .iter()
        .map(|s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
        .collect()
}

/// Détecte la présence de parole par énergie RMS, après pré-emphasis.
/// Seuil unique, réglable : suffisant pour un assistant personnel, et
/// remplaçable par Silero VAD (déjà embarqué dans whisper.cpp) si le bruit
/// ambiant gêne.
pub fn is_speech(samples: &[f32], threshold: f32) -> bool {
    if samples.len() < 64 {
        return false;
    }
    let energy: f32 = samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32;
    energy.sqrt() > threshold
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcription_nettoyee() {
        assert_eq!(clean_transcript("[BLANK_AUDIO]"), "");
        assert_eq!(clean_transcript("(musique) Bonjour"), "Bonjour");
        assert_eq!(clean_transcript("♪ ♪"), "");
        assert_eq!(clean_transcript("*bruit de porte*"), "");
        assert_eq!(clean_transcript("  Quelle   heure est-il ?  "), "Quelle heure est-il ?");
        assert_eq!(clean_transcript("..."), "");
        // Phrases de sous-titres inventées sur du bruit.
        assert_eq!(clean_transcript("Sous-titres réalisés par la communauté d'Amara.org"), "");
        assert_eq!(clean_transcript("Merci d'avoir regardé cette vidéo !"), "");
        // Annotation non refermée : le reste est conservé.
        assert_eq!(clean_transcript("Bonjour (euh"), "Bonjour (euh");
        // Un vrai propos n'est jamais touché.
        assert_eq!(clean_transcript("Dis-moi bonjour."), "Dis-moi bonjour.");
    }

    #[test]
    fn wav_a_entete_valide() {
        let wav = pcm_to_wav(&[0i16; 160], 16_000);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(wav.len(), 44 + 320);
    }

    #[test]
    fn reechantillonnage_change_la_longueur() {
        let input: Vec<f32> = (0..48_000).map(|i| (i as f32 * 0.01).sin()).collect();
        let out = resample(&input, 48_000, 16_000);
        assert_eq!(out.len(), 16_000);
    }

    #[test]
    fn silence_nest_pas_de_la_parole() {
        assert!(!is_speech(&[0.0; 512], 0.01));
        assert!(is_speech(&[0.5; 512], 0.01));
    }
}