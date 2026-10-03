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
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

pub mod cues;
pub mod listener;
pub mod vad;

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
    /// Réservoir d'échantillons partagé avec la callback, **toujours en mono
    /// à `sample_rate`** : la conversion se fait à la capture.
    buffer: Arc<Mutex<VecDeque<f32>>>,
    /// Nombre total d'échantillons écrits depuis la création. Sert de
    /// curseur : `read_since` rend exactement l'audio arrivé depuis la
    /// dernière lecture, sans trou ni doublon.
    written: Arc<AtomicU64>,
    /// Incrémenté à chaque démarrage. Une boucle d'écoute s'arrête dès que la
    /// génération change : un arrêt suivi d'un redémarrage rapide ne laisse
    /// jamais deux boucles sur le même micro.
    generation: AtomicU64,
}

/// Taille du réservoir, en secondes d'audio.
const BUFFER_SECONDS: usize = 30;

impl VoiceRuntime {
    pub fn new(sample_rate: u32) -> Self {
        VoiceRuntime {
            capture: Mutex::new(None),
            running: Arc::new(AtomicBool::new(false)),
            sample_rate,
            buffer: Arc::new(Mutex::new(VecDeque::with_capacity(sample_rate as usize * BUFFER_SECONDS))),
            written: Arc::new(AtomicU64::new(0)),
            generation: AtomicU64::new(0),
        }
    }

    /// Génération courante (voir le champ `generation`).
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
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
        let channels = stream_config.channels;

        // Le tampon repart de zéro : de l'audio d'une session précédente ne
        // doit pas être pris pour un wake word.
        self.buffer.lock().unwrap().clear();
        let buffer = self.buffer.clone();
        let written = self.written.clone();
        let target = self.sample_rate;
        let err_fn = |error| log::warn!("[voice] flux micro : {error}");
        // Piège corrigé : le flux est **entrelacé** (G D G D… en stéréo) et à
        // la fréquence du périphérique (souvent 48 kHz). Il était stocké tel
        // quel puis découpé comme du mono 16 kHz : Whisper recevait un son
        // ralenti, et la fenêtre de 2,4 s n'en contenait que 0,4 s.
        let stream = match supported.sample_format() {
            cpal::SampleFormat::I16 => {
                let mut mixer = Downmixer::new(channels, device_rate, target);
                device.build_input_stream(
                    &stream_config,
                    move |data: &[i16], _: &cpal::InputCallbackInfo| {
                        push(&buffer, &written, mixer.process(data.iter().map(|s| *s as f32 / 32768.0)));
                    },
                    err_fn,
                    None,
                )
            }
            cpal::SampleFormat::I8 => {
                let mut mixer = Downmixer::new(channels, device_rate, target);
                device.build_input_stream(
                    &stream_config,
                    move |data: &[i8], _: &cpal::InputCallbackInfo| {
                        push(&buffer, &written, mixer.process(data.iter().map(|s| *s as f32 / 128.0)));
                    },
                    err_fn,
                    None,
                )
            }
            cpal::SampleFormat::U16 => {
                let mut mixer = Downmixer::new(channels, device_rate, target);
                device.build_input_stream(
                    &stream_config,
                    move |data: &[u16], _: &cpal::InputCallbackInfo| {
                        push(&buffer, &written, mixer.process(data.iter().map(|s| (*s as f32 - 32768.0) / 32768.0)));
                    },
                    err_fn,
                    None,
                )
            }
            cpal::SampleFormat::F32 => {
                let mut mixer = Downmixer::new(channels, device_rate, target);
                device.build_input_stream(
                    &stream_config,
                    move |data: &[f32], _: &cpal::InputCallbackInfo| {
                        push(&buffer, &written, mixer.process(data.iter().copied()));
                    },
                    err_fn,
                    None,
                )
            }
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
            "[voice] micro ouvert — périphérique à {device_rate} Hz, {channels} canal(aux), analyse en mono à {} Hz",
            self.sample_rate
        );

        *self.capture.lock().unwrap() = Some(Capture {
            stream,
            device_rate,
            _device: device,
        });
        self.generation.fetch_add(1, Ordering::Relaxed);
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

    /// Les `samples` derniers échantillons (mono, `sample_rate`), sans les
    /// consommer. Vide si le réservoir n'en contient pas encore autant.
    pub fn take_window(&self, samples: usize) -> Vec<f32> {
        let buffer = self.buffer.lock().unwrap();
        if samples == 0 || buffer.len() < samples {
            return Vec::new();
        }
        let start = buffer.len() - samples;
        buffer.iter().skip(start).copied().collect()
    }

    /// Consomme tout le buffered pour repartir d'une base propre.
    pub fn drain(&self) {
        self.buffer.lock().unwrap().clear();
    }

    pub fn voice_activity(&self, samples: usize, threshold: f32) -> bool {
        let window = self.take_window(samples);
        !window.is_empty() && is_speech(&window, threshold)
    }

    /// Niveau RMS des `samples` derniers échantillons (0 si pas assez
    /// d'audio). Sert au VAD adaptatif et au vumètre de l'interface.
    pub fn level(&self, samples: usize) -> f32 {
        let window = self.take_window(samples);
        if window.is_empty() {
            return 0.0;
        }
        (window.iter().map(|s| s * s).sum::<f32>() / window.len() as f32).sqrt()
    }

    /// Tests uniquement : marque l'écoute comme active sans ouvrir de micro.
    /// Combiné à [`VoiceRuntime::inject`], il fait tourner la vraie boucle
    /// d'écoute sur un audio connu.
    #[doc(hidden)]
    pub fn start_without_device(&self) {
        self.buffer.lock().unwrap().clear();
        self.generation.fetch_add(1, Ordering::Relaxed);
        self.running.store(true, Ordering::Relaxed);
    }

    /// Tests uniquement : ajoute de l'audio (mono, `sample_rate`) au tampon,
    /// comme s'il venait du micro.
    #[doc(hidden)]
    pub fn inject(&self, samples: &[f32]) {
        push(&self.buffer, &self.written, samples.to_vec());
    }

    /// Position courante d'écriture (voir `read_since`).
    pub fn cursor(&self) -> u64 {
        self.written.load(Ordering::Relaxed)
    }

    /// Audio arrivé depuis `cursor`, puis avance le curseur. Contrairement à
    /// `take_window`, ne rend jamais deux fois le même échantillon, et rend
    /// ce qui est disponible même juste après une purge du tampon.
    pub fn read_since(&self, cursor: &mut u64) -> Vec<f32> {
        let buffer = self.buffer.lock().unwrap();
        let total = self.written.load(Ordering::Relaxed);
        let fresh = (total.saturating_sub(*cursor) as usize).min(buffer.len());
        *cursor = total;
        let start = buffer.len() - fresh;
        buffer.iter().skip(start).copied().collect()
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Fréquence réelle du périphérique (diagnostic).
    pub fn device_rate(&self) -> Option<u32> {
        self.capture.lock().unwrap().as_ref().map(|c| c.device_rate)
    }
}

/// Ramène le flux du micro au format d'analyse : mono, `target` Hz.
///
/// Mixage : moyenne des canaux de chaque trame. Rééchantillonnage : moyenne
/// des échantillons tombant dans chaque période de sortie (filtre « boîte »),
/// avec une phase conservée d'une callback à l'autre — pas de dérive ni de
/// coupure entre deux blocs.
pub struct Downmixer {
    channels: usize,
    /// Échantillons d'entrée par échantillon de sortie (48 kHz → 16 kHz : 3).
    step: f64,
    phase: f64,
    sum: f32,
    count: u32,
    frame_sum: f32,
    frame_fill: usize,
}

impl Downmixer {
    pub fn new(channels: u16, device_rate: u32, target_rate: u32) -> Self {
        Downmixer {
            channels: channels.max(1) as usize,
            step: device_rate.max(1) as f64 / target_rate.max(1) as f64,
            phase: 0.0,
            sum: 0.0,
            count: 0,
            frame_sum: 0.0,
            frame_fill: 0,
        }
    }

    pub fn process(&mut self, interleaved: impl Iterator<Item = f32>) -> Vec<f32> {
        let mut out = Vec::new();
        for sample in interleaved {
            self.frame_sum += sample;
            self.frame_fill += 1;
            if self.frame_fill < self.channels {
                continue;
            }
            let mono = self.frame_sum / self.channels as f32;
            self.frame_sum = 0.0;
            self.frame_fill = 0;

            self.sum += mono;
            self.count += 1;
            self.phase += 1.0;
            if self.phase >= self.step {
                let value = self.sum / self.count as f32;
                // Fréquence d'entrée < cible : on répète l'échantillon.
                while self.phase >= self.step {
                    out.push(value);
                    self.phase -= self.step;
                }
                self.sum = 0.0;
                self.count = 0;
            }
        }
        out
    }
}

fn push(buffer: &Arc<Mutex<VecDeque<f32>>>, written: &AtomicU64, samples: Vec<f32>) {
    if samples.is_empty() {
        return;
    }
    let mut buffer = buffer.lock().unwrap();
    // Sous le verrou : `read_since` voit toujours un compteur cohérent avec
    // le contenu du tampon.
    written.fetch_add(samples.len() as u64, Ordering::Relaxed);
    // On borne le réservoir : au-delà de 30 s, les échantillons sont périmés.
    let limit = buffer.capacity();
    for sample in samples {
        if buffer.len() >= limit {
            buffer.pop_front();
        }
        buffer.push_back(sample);
    }
}

/// Convertit une fenêtre d'échantillons en fichier WAV prêt pour Whisper.
///
/// Pas de normalisation du volume : essayée (crête ramenée à 0,9), elle a
/// dégradé la transcription — « et Jimmy. » devenait « ee uh ! » et le bruit
/// de fond amplifié produisait des phrases inventées.
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
// « je mise » : rendu du modèle `base` mesuré le 3 octobre (« je mise dimanche »).
const KNOWN_PHRASES: &[&str] = &["j ai mis", "j y mise", "j ai mi", "je mise", "je mis", "j y mis", "chemise", "shami"];

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
    // On accepte une interjection d'appel devant le nom : « hé Jimmy »,
    // « ok Jimmy ». Au-delà, un « Jimmy » en milieu de phrase n'est pas un
    // appel.
    // « C'est Jimmy » : rendu fréquent de « Hé Jimmy » (même son), deux tokens.
    let start = if tokens.len() > 2 && tokens[0] == "c" && tokens[1] == "est" {
        2
    } else if tokens.len() > 1 && CALL_WORDS.contains(&tokens[0]) {
        1
    } else {
        0
    };
    let rest = &tokens[start..];

    let target_key = phonetic(&target);
    let first = rest[0];
    if first == target
        || levenshtein(first, &target) <= tolerance_for(&target)
        // Comparaison par la prononciation : Whisper écrit « Guimmi »,
        // « Gimmy » ou « Djimi » pour « Jimmy ». Tous donnent la clé « jimi ».
        || levenshtein(&phonetic(first), &target_key) <= tolerance_for(&target_key).min(1)
    {
        return Some(start + 1);
    }
    for phrase in KNOWN_PHRASES {
        let parts: Vec<&str> = phrase.split(' ').collect();
        if rest.len() >= parts.len() && rest[..parts.len()] == *parts {
            return Some(start + parts.len());
        }
    }
    None
}

/// Interjections qui peuvent précéder le nom (« hé Jimmy »).
// « et » : Whisper écrit souvent « Hé Jimmy » comme « Et Jimmy » (même son).
const CALL_WORDS: &[&str] = &["he", "hey", "et", "ok", "okay", "eh", "dis", "allo", "bonjour", "salut"];

/// Clé phonétique (français) d'un mot normalisé : ce qui s'entend, pas ce qui
/// s'écrit. `gu`+voyelle → `g`, `g`+e/i/y → `j`, `dj` → `j`, `y` → `i`,
/// `ph` → `f`, `h` muet supprimé, lettres doublées fusionnées.
pub fn phonetic(word: &str) -> String {
    let chars: Vec<char> = word.chars().collect();
    let mut out: Vec<char> = Vec::with_capacity(chars.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        let mapped = match c {
            'g' if next == Some('u') && matches!(chars.get(i + 2), Some('e' | 'i' | 'y')) => {
                i += 1; // « gu » devant e/i : le u ne s'entend pas
                'g'
            }
            'g' if matches!(next, Some('e' | 'i' | 'y')) => 'j',
            'd' if next == Some('j') => {
                i += 1;
                'j'
            }
            'p' if next == Some('h') => {
                i += 1;
                'f'
            }
            'c' if next == Some('h') => {
                i += 1;
                's'
            }
            'h' => {
                i += 1;
                continue;
            }
            'y' => 'i',
            other => other,
        };
        if out.last() != Some(&mapped) {
            out.push(mapped);
        }
        i += 1;
    }
    out.into_iter().collect()
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
        // Un mot de pure ponctuation (le « - » de dialogue que Whisper met en
        // tête) ne produit aucun token : il ne doit rien consommer. Le compter
        // pour un décalait tout, et « - Eh, Jimmy ! » donnait « Jimmy ! ».
        let produced = crate::memory::embed::normalize(word)
            .split(' ')
            .filter(|t| !t.is_empty())
            .count();
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
    // Piège corrigé : la sortie est **entrelacée**. Écrire un échantillon mono
    // par case jouait le son deux fois trop vite sur une sortie stéréo — le
    // « mot rapide incompréhensible ». Chaque échantillon est dupliqué sur
    // tous les canaux de la trame.
    let channels = config.channels.max(1) as usize;

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
                drain(&queue_cb, &done_cb, data, channels, |s| s);
            },
            |_err| {},
            None,
        ),
        cpal::SampleFormat::I16 => device.build_output_stream(
            &config,
            move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                drain(&queue_cb, &done_cb, data, channels, |s| (s.clamp(-1.0, 1.0) * 32767.0) as i16);
            },
            |_err| {},
            None,
        ),
        cpal::SampleFormat::U16 => device.build_output_stream(
            &config,
            move |data: &mut [u16], _: &cpal::OutputCallbackInfo| {
                drain(&queue_cb, &done_cb, data, channels, |s| ((s.clamp(-1.0, 1.0) + 1.0) * 32767.5) as u16);
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
    // Le dernier bloc écrit doit encore sortir du périphérique : couper le
    // flux tout de suite tronquait la dernière syllabe.
    std::thread::sleep(std::time::Duration::from_millis(200));
    Ok(())
}


/// Vide la file dans le tampon de sortie, et signale la fin quand elle n'a
/// plus rien à fournir.
fn drain<T: Copy>(
    queue: &Arc<Mutex<VecDeque<f32>>>,
    done: &Arc<AtomicBool>,
    data: &mut [T],
    channels: usize,
    convert: impl Fn(f32) -> T,
) {
    let mut queue = queue.lock().unwrap();
    let silence = convert(0.0);
    let mut exhausted = false;
    for frame in data.chunks_mut(channels) {
        // Une valeur mono par trame, recopiée sur chaque canal.
        let value = if exhausted { None } else { queue.pop_front() };
        match value {
            Some(sample) => frame.fill(convert(sample)),
            None => {
                // Toute la fin du tampon est remise au silence : avant, les
                // cases restantes gardaient l'ancien contenu (bruit).
                exhausted = true;
                frame.fill(silence);
            }
        }
    }
    drop(queue);
    if exhausted {
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

    /// Stéréo 48 kHz → mono 16 kHz : une seconde d'entrée doit donner une
    /// seconde de sortie, et un signal identique sur les deux canaux doit
    /// garder son amplitude.
    #[test]
    fn variantes_phonetiques_du_nom() {
        // Rendus réels de Whisper pour « Jimmy » (mesurés sur ce projet).
        for transcript in ["Guimmi, quelle heure est-il ?", "Gimmy ouvre le dossier", "Djimi, analyse ça", "Jimi"] {
            assert!(matches_wake_word(transcript, "jimmy"), "{transcript}");
        }
        assert_eq!(strip_wake_word("Guimmi, quelle heure est-il ?", "jimmy"), "quelle heure est-il ?");
    }

    #[test]
    fn interjection_avant_le_nom() {
        assert!(matches_wake_word("Hé Jimmy, ouvre mes notes", "jimmy"));
        assert!(matches_wake_word("et Jimmy.", "jimmy"), "« Hé » transcrit « et »");
        assert!(matches_wake_word("C'est Jimmy.", "jimmy"), "« Hé » transcrit « c'est »");
        // Tiret de dialogue ajouté par Whisper en tête de transcription.
        assert_eq!(strip_wake_word("- Eh, Jimmy !", "jimmy"), "");
        assert_eq!(strip_wake_word("- Jimmy, quelle heure est-il ?", "jimmy"), "quelle heure est-il ?");
        assert_eq!(strip_wake_word("C'est Jimmy, dis-moi bonjour", "jimmy"), "dis-moi bonjour");
        assert_eq!(strip_wake_word("je mise, dis-moi bonjour", "jimmy"), "dis-moi bonjour");
        assert_eq!(strip_wake_word("Ok Jimmy, ouvre mes notes", "jimmy"), "ouvre mes notes");
    }

    #[test]
    fn pas_de_faux_positif_phonetique() {
        for transcript in ["Gimou.", "J'ai besoin d'aide", "Jamais de la vie", "Salut tout le monde", "Il est midi"] {
            assert!(!matches_wake_word(transcript, "jimmy"), "{transcript}");
        }
    }

    #[test]
    fn sortie_stereo_duplique_chaque_echantillon() {
        let queue = Arc::new(Mutex::new(VecDeque::from(vec![0.1f32, 0.2, 0.3])));
        let done = Arc::new(AtomicBool::new(false));
        let mut data = [9.0f32; 8];
        drain(&queue, &done, &mut data, 2, |s| s);
        // 3 trames stéréo, puis silence (et non l'ancien contenu « 9 »).
        assert_eq!(data, [0.1, 0.1, 0.2, 0.2, 0.3, 0.3, 0.0, 0.0]);
        assert!(done.load(Ordering::Relaxed));
    }

    #[test]
    fn curseur_sans_doublon_ni_trou() {
        let runtime = VoiceRuntime::new(16_000);
        let mut cursor = runtime.cursor();
        runtime.inject(&[1.0, 2.0, 3.0]);
        assert_eq!(runtime.read_since(&mut cursor), vec![1.0, 2.0, 3.0]);
        assert!(runtime.read_since(&mut cursor).is_empty());
        runtime.inject(&[4.0]);
        runtime.drain();
        runtime.inject(&[5.0, 6.0]);
        // Après une purge : seul ce qui reste est rendu, sans paniquer.
        assert_eq!(runtime.read_since(&mut cursor), vec![5.0, 6.0]);
    }

    #[test]
    fn stereo_48k_vers_mono_16k() {
        let mut mixer = Downmixer::new(2, 48_000, 16_000);
        let mut out = Vec::new();
        // Plusieurs callbacks de tailles irrégulières : la phase doit tenir.
        let frames: Vec<f32> = (0..48_000).flat_map(|_| [0.5f32, 0.5]).collect();
        for chunk in frames.chunks(882) {
            out.extend(mixer.process(chunk.iter().copied()));
        }
        assert_eq!(out.len(), 16_000);
        assert!(out.iter().all(|v| (v - 0.5).abs() < 1e-6));
    }

    /// 44,1 kHz mono → 16 kHz : rapport non entier, aucune dérive.
    #[test]
    fn mono_44k_vers_16k_sans_derive() {
        let mut mixer = Downmixer::new(1, 44_100, 16_000);
        let mut total = 0;
        for _ in 0..10 {
            total += mixer.process(std::iter::repeat(0.1f32).take(44_100)).len();
        }
        assert!((159_999..=160_001).contains(&total), "{total}");
    }

    /// Canaux opposés : le mixage les annule (moyenne, pas somme).
    #[test]
    fn canaux_moyennes() {
        let mut mixer = Downmixer::new(2, 16_000, 16_000);
        let out = mixer.process([1.0f32, -1.0, 0.4, 0.4].into_iter());
        assert_eq!(out, vec![0.0, 0.4]);
    }

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