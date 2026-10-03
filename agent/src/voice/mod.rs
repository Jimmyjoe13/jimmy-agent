//! Voix : microphone, détection du wake word, reconnaissance, lecture.
//!
//! Tout se passe dans Rust, pas dans la fenêtre : la capture audio native évite
//! les aléas du WebView (permissions, formats, coupures) et garde un seul
//! chemin audio, donc moins de cas à debugger.
//!
//! ## Boucle d'écoute
//!
//! ```text
//! micro (cpal)
//!   → sous-échantillonnage à 16 kHz
//!   → VAD par énergie
//!   → fenêtre glissante de 2,4 s
//!   → whisper.cpp (base) → texte
//!   → le texte contient « Jimmy » ?  →  OUI : on continue d'écouter la phrase
//!                                  →  NON : on ouvre une fenêtre plus longue
//! ```
//!
//! L'énergie RMS n'est utilisée **que** pour décider de transcrire ou non :
//! entre deux phrases, le CPU est au repos. La reconnaissance, elle, est
//! confiée à Whisper, dont le modèle « base » s'est montré fiable sur le
//! français en environ 1,3 s pour une fenêtre courte — assez pour un wake word.
//!
//! Une fois le wake word détecté, la phrase entière est retranscrite avec un
//! modèle plus précis (`small` par défaut pour la commande), car là la qualité
//! française compte plus que la latence.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub mod listener;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::error::{Error, Result};
use crate::providers::stt::{float_to_i16, is_speech, pcm_to_wav, resample};

/// Ce que la couche d'écoute remonte à l'agent.
#[derive(Debug, Clone)]
pub enum VoiceEvent {
    /// Le wake word a été détecté.
    Wake { at_sample: usize },
    /// Fin de phrase : le texte reconnu est disponible.
    Speech { text: String },
    /// L'utilisateur a began à parler, sans wake word.
    Ignored { text: String },
    Error(String),
}

/// Ce que la couche agentique fournit à la couche d'écoute.
#[derive(Debug, Clone)]
pub struct Utterance {
    pub text: String,
}

/// Microphone capturé en mémoire. `cpal` impose une callback temps réel : le
/// tampon y est seulement recopié, le reste se fait dans la tâche async.
struct Capture {
    stream: cpal::Stream,
    /// Format réel du périphérique, nécessaire pour rééchantillononner.
    device_rate: u32,
    _device: cpal::Device,
}

pub struct VoiceRuntime {
    capture: Mutex<Option<Capture>>,
    running: Arc<AtomicBool>,
    sample_rate: u32,
    /// Réservoir d'échantillons partagé avec la callback.
    buffer: Arc<Mutex<VecDeque<f32>>>,
}

impl VoiceRuntime {
    pub fn new(sample_rate: u32) -> Self {
        VoiceRuntime {
            capture: Mutex::new(None),
            running: Arc::new(AtomicBool::new(false)),
            sample_rate,
            buffer: Arc::new(Mutex::new(VecDeque::with_capacity(sample_rate as usize * 30))),
        }
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// Ouvre le périphérique d'entrée par défaut et démarre la capture.
    pub fn start(&self) -> Result<()> {
        if self.is_running() {
            return Ok(());
        }
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| Error::Voice("aucun microphone détecté".into()))?;
        let supported = device
            .default_input_config()
            .map_err(|e| Error::Voice(format!("configuration micro indisponible : {e}")))?;
        let stream_config: cpal::StreamConfig = supported.config();
        let device_rate = stream_config.sample_rate.0;

        let buffer = self.buffer.clone();
        let err_fn = |_err| {};
        let stream = match supported.sample_format() {
            cpal::SampleFormat::I16 => device.build_input_stream(
                &stream_config,
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    push(&buffer, data.iter().map(|s| *s as f32 / 32768.0).collect::<Vec<_>>());
                },
                err_fn,
                None,
            ),
            cpal::SampleFormat::I8 => device.build_input_stream(
                &stream_config,
                move |data: &[i8], _: &cpal::InputCallbackInfo| {
                    push(&buffer, data.iter().map(|s| *s as f32 / 128.0).collect::<Vec<_>>());
                },
                err_fn,
                None,
            ),
            cpal::SampleFormat::U16 => device.build_input_stream(
                &stream_config,
                move |data: &[u16], _: &cpal::InputCallbackInfo| {
                    push(&buffer, data.iter().map(|s| (*s as f32 - 32768.0) / 32768.0).collect::<Vec<_>>());
                },
                err_fn,
                None,
            ),
            cpal::SampleFormat::F32 => device.build_input_stream(
                &stream_config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    push(&buffer, data.to_vec());
                },
                err_fn,
                None,
            ),
            other => {
                return Err(Error::Voice(format!(
                    "format de micro non géré : {other:?}"
                )))
            }
        }
        .map_err(|e| Error::Voice(format!("capture impossible : {e}")))?;

        stream
            .play()
            .map_err(|e| Error::Voice(format!("démarrage du flux impossible : {e}")))?;
        log::info!(
            "[voice] micro ouvert — périphérique à {device_rate} Hz, analyse à {} Hz",
            self.sample_rate
        );

        *self.capture.lock().unwrap() = Some(Capture {
            stream,
            device_rate,
            _device: device,
        });
        self.running.store(true, Ordering::Relaxed);
        Ok(())
    }

    pub fn stop(&self) {
        if let Some(capture) = self.capture.lock().unwrap().take() {
            let _ = capture.stream.pause();
        }
        self.running.store(false, Ordering::Relaxed);
        log::info!("[voice] micro fermé");
    }

    /// Liste les périphériques d'entrée disponibles, pour l'onboarding.
    pub fn list_devices() -> Vec<String> {
        let host = cpal::default_host();
        match host.input_devices() {
            Ok(devices) => devices.filter_map(|device| device.name().ok()).collect(),
            Err(_) => Vec::new(),
        }
    }

    /// Tire les derniers échantillons du réservoir et les rééchantillonne.
    pub fn take_window(&self, samples: usize) -> Vec<f32> {
        let buffer = self.buffer.lock().unwrap();
        if buffer.len() < samples {
            return Vec::new();
        }
        let start = buffer.len() - samples;
        let raw: Vec<f32> = buffer.iter().skip(start).copied().collect();
        drop(buffer);
        let device_rate = self.device_rate();
        if device_rate == self.sample_rate {
            raw
        } else {
            resample(&raw, device_rate, self.sample_rate)
        }
    }

    /// Consomme tout le buffered pour repartir d'une base propre.
    pub fn drain(&self) {
        self.buffer.lock().unwrap().clear();
    }

    pub fn voice_activity(&self, samples: usize, threshold: f32) -> bool {
        let window = self.take_window(samples);
        !window.is_empty() && is_speech(&window, threshold)
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn device_rate(&self) -> u32 {
        self.capture
            .lock()
            .unwrap()
            .as_ref()
            .map(|c| c.device_rate)
            .unwrap_or(self.sample_rate)
    }
}

fn push(buffer: &Arc<Mutex<VecDeque<f32>>>, samples: Vec<f32>) {
    let mut buffer = buffer.lock().unwrap();
    // On borne le réservoir : au-delà de 30 s, les échantillons sont périmés.
    let limit = buffer.capacity();
    for sample in samples {
        if buffer.len() == limit {
            buffer.pop_front();
        }
        buffer.push_back(sample);
    }
}

/// Convertit une fenêtre d'échantillons en fichier WAV prêt pour Whisper.
pub fn window_to_wav(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    pcm_to_wav(&float_to_i16(samples), sample_rate)
}

/// Rendu du mot d'activation par Whisper, entokens normalisés.
///
/// « Jimmy » est un mot court, sans contexte : les modèles rapides le
/// confondent régulièrement. Sur le modèle `base`, les rendus observés sont
/// « j'ai mis » et « j'y mise ». On les liste explicitement, ce qui reste sans
/// risque de faux positif : une phrase comme « j'ai besoin » ne correspond à
/// aucune de ces formes.
const KNOWN_PHRASES: &[&str] = &["j ai mis", "j y mise", "j ai mi", "chemise", "shami"];

/// Nombre de mots Recognition en tête de `transcript`, si le mot d'activation
/// y figure. `None` si la phrase ne commence pas par le mot d'activation.
pub fn find_wake_prefix(transcript: &str, wake_word: &str) -> Option<usize> {
    let target = crate::memory::embed::normalize(wake_word);
    if target.is_empty() {
        return None;
    }
    let normalized = crate::memory::embed::normalize(transcript);
    let tokens: Vec<&str> = normalized.split(' ').filter(|t| !t.is_empty()).collect();
    if tokens.is_empty() {
        return None;
    }
    if tokens[0] == target || levenshtein(tokens[0], &target) <= tolerance_for(&target) {
        return Some(1);
    }
    for phrase in KNOWN_PHRASES {
        let parts: Vec<&str> = phrase.split(' ').collect();
        if tokens.len() >= parts.len() && tokens[..parts.len()] == *parts {
            return Some(parts.len());
        }
    }
    None
}

/// Normalise une reconnaissance pour comparer au mot d'activation.
pub fn matches_wake_word(transcript: &str, wake_word: &str) -> bool {
    find_wake_prefix(transcript, wake_word).is_some()
}

fn tolerance_for(word: &str) -> usize {
    match word.chars().count() {
        0..=3 => 0,
        4..=6 => 1,
        _ => 2,
    }
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    let mut current = vec![0usize; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        current[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            current[j + 1] = (previous[j] + cost)
                .min(previous[j + 1] + 1)
                .min(current[j] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[b.len()]
}

/// Retire le mot d'activation du début d'une commande.
///
/// [`find_wake_prefix`] raisonne en tokens normalisés, où « J'y mise » compte
/// pour trois ; ici on parcourt les mots d'origine, dont la ponctuation doit
/// disparaître. On convertit donc : chaque mot d'origine consomme autant de
/// tokens normalisés qu'il en produit.
pub fn strip_wake_word(text: &str, wake_word: &str) -> String {
    let Some(prefix_len) = find_wake_prefix(text, wake_word) else {
        return text.trim().to_string();
    };

    let mut remaining = prefix_len;
    let mut tail: Vec<&str> = Vec::new();
    for word in text.split_whitespace() {
        if remaining == 0 {
            tail.push(word);
            continue;
        }
        let produced = crate::memory::embed::normalize(word)
            .split(' ')
            .filter(|t| !t.is_empty())
            .count()
            .max(1);
        remaining = remaining.saturating_sub(produced);
    }
    // Le mot d'activation emporte la ponctuation qui le suit.
    tail.join(" ")
        .trim()
        .trim_start_matches(|c: char| !c.is_alphanumeric())
        .to_string()
}

/// Joue un buffer audio via le périphérique de sortie par défaut.
///
/// `sample_rate` décrit l'audio entrant : 44 100 Hz pour le PCM de Fish Audio.
/// Si la carte son ne sait pas travailler à cette fréquence — c'est rare, mais
/// un périphérique à 48 kHz peut refuser — on rééchantillonne.
///
/// Le flux est construit explicitement plutôt que via une aide globale : le
/// tampon est vidé par la callback, et l'appelant attend la fin de la lecture.
/// C'est ce qui permet d'enchaîner parole → animation → parole sans que les
/// flux se chevauchent.
pub fn play_bytes(bytes: &[u8], sample_rate: u32) -> Result<()> {
    if bytes.is_empty() {
        return Ok(());
    }
    let mut samples = decode_audio(bytes, sample_rate)?;
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| Error::Voice("aucun périphérique de sortie détecté".into()))?;
    let supported = device
        .default_output_config()
        .map_err(|e| Error::Voice(format!("configuration de sortie indisponible : {e}")))?;
    let config: cpal::StreamConfig = supported.config();

    let device_rate = config.sample_rate.0;
    if device_rate != sample_rate && sample_rate > 0 {
        samples = resample(&samples, sample_rate, device_rate);
    }

    let total_samples = samples.len() as u64;
    let queue = Arc::new(Mutex::new(samples.into_iter().collect::<VecDeque<f32>>()));
    let done = Arc::new(AtomicBool::new(false));
    let queue_cb = queue.clone();
    let done_cb = done.clone();

    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => device.build_output_stream(
            &config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                drain(&queue_cb, &done_cb, data, |s| s);
            },
            |_err| {},
            None,
        ),
        cpal::SampleFormat::I16 => device.build_output_stream(
            &config,
            move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                drain(&queue_cb, &done_cb, data, |s| (s * 32767.0) as i16);
            },
            |_err| {},
            None,
        ),
        other => {
            return Err(Error::Voice(format!(
                "format de sortie non géré : {other:?}"
            )))
        }
    }
    .map_err(|e| Error::Voice(format!("lecture impossible : {e}")))?;

    // Indispensable : `build_output_stream` ne fait que préparer le flux. Sans
    // cet appel, la callback n'est jamais invoquée et la boucle d'attente
    // ci-dessous ne termine jamais.
    stream
        .play()
        .map_err(|e| Error::Voice(format!("démarrage de la lecture impossible : {e}")))?;

    // Filet de sécurité : si le périphérique se tait, on ne bloque pas
    // l'agent indéfiniment. La durée réelle est majorée d'une seconde de
    // latence de périphérique.
    let limit = std::time::Duration::from_millis(
        ((total_samples * 1000) / device_rate.max(1) as u64).saturating_add(1_000),
    );
    let started = std::time::Instant::now();
    while !done.load(Ordering::Relaxed) {
        if started.elapsed() > limit {
            log::warn!("[voice] lecture interrompue : le périphérique n'a rien produit");
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    Ok(())
}


/// Vide la file dans le tampon de sortie, et signale la fin quand elle n'a
/// plus rien à fournir.
fn drain<T: Copy + Default>(
    queue: &Arc<Mutex<VecDeque<f32>>>,
    done: &Arc<AtomicBool>,
    data: &mut [T],
    convert: impl Fn(f32) -> T,
) {
    let mut queue = queue.lock().unwrap();
    let mut produced = 0usize;
    for slot in data.iter_mut() {
        match queue.pop_front() {
            Some(value) => {
                *slot = convert(value);
                produced += 1;
            }
            None => {
                *slot = T::default();
                break;
            }
        }
    }
    drop(queue);
    if produced < data.len() {
        done.store(true, Ordering::Relaxed);
    }
}

/// Décode un buffer audio en échantillons flottants.
///
/// Deux formats sont acceptés, tous deux sans dépendance :
///
/// * **WAV** — décodé directement ;
/// * **PCM brut** — ce que Fish Audio renvoie quand on demande
///   `response_format: "pcm"` : échantillons 16 bits signés, petit-boutistes,
///   sans aucune en-tête.
///
/// Pourquoi le PCM plutôt que le MP3 ? Le modèle ne produit pas de WAV
/// (`wav` renvoie 400), et le MP3 exigerait un décodeur — donc une
/// dépendance et quelques licences. Demander du PCM évite les deux : le
/// fichier est déjà dans le format que la carte son consomme.
fn decode_audio(bytes: &[u8], sample_rate: u32) -> Result<Vec<f32>> {
    if bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WAVE" {
        return decode_wav(bytes);
    }
    decode_pcm(bytes, sample_rate)
}

/// PCM 16 bits signés, petit-boutiste, sans en-tête.
fn decode_pcm(bytes: &[u8], _sample_rate: u32) -> Result<Vec<f32>> {
    // Un nombre impair d'octets signifie un échantillon tronqué : on l'ignore.
    let usable = bytes.len() - (bytes.len() % 2);
    if usable == 0 {
        return Err(Error::Voice("audio vide".into()));
    }
    let mut out = Vec::with_capacity(usable / 2);
    for chunk in bytes[..usable].chunks_exact(2) {
        let value = i16::from_le_bytes([chunk[0], chunk[1]]);
        out.push(value as f32 / 32768.0);
    }
    Ok(out)
}

fn decode_wav(bytes: &[u8]) -> Result<Vec<f32>> {
    let mut reader = hound::WavReader::new(std::io::Cursor::new(bytes))
        .map_err(|e| Error::Voice(format!("WAV illisible : {e}")))?;
    let spec = reader.spec();
    let samples: Vec<i16> = reader
        .samples::<i16>()
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| Error::Voice(format!("WAV illisible : {e}")))?;
    let scale = if spec.channels > 1 { 1.0 / spec.channels as f32 } else { 1.0 };
    Ok(samples.iter().map(|s| *s as f32 / 32768.0 * scale).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wake_word_exact() {
        assert!(matches_wake_word("Jimmy, analyse ce dossier", "jimmy"));
        assert_eq!(strip_wake_word("Jimmy analyse ce dossier", "jimmy"), "analyse ce dossier");
    }

    #[test]
    fn wake_word_variante_asr() {
        assert!(matches_wake_word("J'y mise, analyse ce dossier", "jimmy"));
        assert!(matches_wake_word("j'ai mis ouvre le fichier", "jimmy"));
        assert_eq!(
            strip_wake_word("J'y mise, analyse ce dossier", "jimmy"),
            "analyse ce dossier"
        );
    }

    #[test]
    fn pas_de_faux_positif_sur_jai() {
        // « j'ai » seul ne doit jamais réveiller Jimmy : c'est le cas le plus
        // courant en français.
        assert!(!matches_wake_word("J'ai besoin d'un rapport", "jimmy"));
        assert!(!matches_wake_word("ouvre le dossier du projet", "jimmy"));
    }

    #[test]
    fn commande_sans_wake_word_inchangee() {
        assert_eq!(strip_wake_word("analyse ce dossier", "jimmy"), "analyse ce dossier");
    }

    #[test]
    fn distance_edition() {
        assert_eq!(levenshtein("jimmy", "jimmy"), 0);
        assert_eq!(levenshtein("jimmy", "jimi"), 2);
        assert_eq!(levenshtein("", "abc"), 3);
        assert_eq!(levenshtein("abc", "abc"), 0);
    }
}