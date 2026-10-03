//! Boucle d'écoute permanente : c'est le cœur du produit.
//!
//! ```text
//! micro → fenêtre glissante → VAD → whisper → « Jimmy » ? → phrase → agent → voix
//! ```
//!
//! Trois décisions structurantes, toutes dictées par la latence mesurée sur la
//! machine cible :
//!
//! * **le VAD évite de transcrire le silence.** Entre deux phrases, rien ne
//!   tourne : le CPU est libre. Sans cela, il faudrait transcrire en continu et
//!   le coût devenait inacceptable ;
//! * **le wake word et la commande passent par le même modèle**, celui choisi
//!   dans les paramètres. Deux modèles signifieraient deux serveurs, donc deux
//!   fois plus de mémoire et une bascule complexe. Whisper s'appuie fortement
//!   sur le contexte : la commande transcrit bien mieux que le mot isolé, et la
//!   fenêtre large qui suit le wake word lui donne ce contexte ;
//! * **la boucle ne bloque jamais l'agent** : chaque tour est une tâche
//!   séquentielle, et une erreur de transcription est ignorée au lieu de
//!   couper l'écoute.

use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::mpsc::Sender;

use super::{matches_wake_word, strip_wake_word, window_to_wav, VoiceRuntime};
use crate::config::Settings;
use crate::core::types::{AgentEvent, AvatarState};
use crate::App;

/// Durée maximale d'une phrase : au-delà, on transcrit ce qu'on a.
const MAX_UTTERANCE: Duration = Duration::from_secs(12);
/// Durée minimale d'écoute avant qu'un silence puisse clore la phrase.
const MIN_UTTERANCE: Duration = Duration::from_millis(600);
/// Après le son « Oui ? », l'utilisateur n'a pas encore commencé à parler :
/// le silence initial ne doit pas clore l'écoute.
const AFTER_CUE_GRACE: Duration = Duration::from_millis(3500);

impl App {
    /// Démarre la boucle d'écoute. Ne rend pas la main : elle tourne jusqu'à
    /// ce que le microphone soit arrêté.
    pub fn spawn_voice_listener(
        self: Arc<Self>,
        runtime: Arc<VoiceRuntime>,
        events: Sender<AgentEvent>,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let settings = self.settings();
            let rate = settings.voice.input_sample_rate;
            let wake_window = Duration::from_millis(settings.stt.wake_window_ms.max(600));
            let end_of_speech = Duration::from_millis(settings.voice.end_of_speech_ms);
            let hop = Duration::from_millis(450);

            let mut last_beat = Instant::now();
            log::info!("[voice] écoute démarrée — mot d'activation « {} »", settings.stt.wake_word);

            while runtime.is_running() {
                tokio::time::sleep(hop).await;
                if last_beat.elapsed() > Duration::from_secs(300) {
                    emit(&events, AgentEvent::State { state: AvatarState::Idle, detail: String::new() }).await;
                    last_beat = Instant::now();
                    continue;
                }
                last_beat = Instant::now();

                let window_samples = (rate as f64 * wake_window.as_secs_f64()) as usize;
                if !runtime.voice_activity(window_samples, settings.voice.vad_threshold) {
                    continue;
                }

                // 1. Fenêtre courte : le wake word est-il là ?
                let Some(text) = transcribe(&self, &runtime, window_samples, rate).await else {
                    continue;
                };
                if text.is_empty() {
                    continue;
                }
                if !matches_wake_word(&text, &settings.stt.wake_word) {
                    log::trace!("[voice] parole sans wake word : « {text} »");
                    continue;
                }

                // 2. Le wake word est passé : on attend la fin de la phrase.
                emit(&events, AgentEvent::State { state: AvatarState::Listening, detail: text.clone() }).await;
                let command = if strip_wake_word(&text, &settings.stt.wake_word).trim().is_empty() {
                    // « Jimmy » seul, suivi d'une pause : Jimmy répond « Oui ? »,
                    // puis écoute. Le micro est purgé après le son, sinon sa
                    // propre voix (haut-parleur → micro) entrerait dans la
                    // commande. Délai de grâce : laisser le temps de commencer.
                    super::cues::play(&self, super::cues::Cue::Listening).await;
                    runtime.drain();
                    collect_utterance(&self, &runtime, 0, rate, hop, end_of_speech, AFTER_CUE_GRACE).await
                } else {
                    // La commande suit déjà le wake word : aucun son, il
                    // couperait la parole.
                    collect_utterance(&self, &runtime, window_samples, rate, hop, end_of_speech, MIN_UTTERANCE).await
                }
                .unwrap_or_default();
                let command = strip_wake_word(&command, &settings.stt.wake_word);
                if command.trim().is_empty() {
                    emit(&events, AgentEvent::Notice { message: "rien d comprehensible dans la phrase".into() }).await;
                    emit(&events, AgentEvent::State { state: AvatarState::Idle, detail: String::new() }).await;
                    continue;
                }
                log::info!("[voice] commande : « {command} »");

                // 3. L'agent prend le relais, puis Jimmy répond à la voix.
                let session = self
                    .history
                    .create_session("Session vocale")
                    .unwrap_or_else(|_| String::from("vocale"));
                let answer = crate::core::agent::run(
                    self.deps(),
                    self.settings(),
                    session,
                    command.clone(),
                    self.tool_context(),
                    events.clone(),
                )
                .await;

                match answer {
                    Ok(answer) => {
                        speak(&self, &answer.text).await;
                        emit(&events, AgentEvent::State { state: AvatarState::Idle, detail: String::new() }).await;
                    }
                    Err(error) => {
                        emit(&events, AgentEvent::Failed { message: error.to_string() }).await;
                        speak(&self, &format!("Désolé, je n'ai pas pu faire ça : {error}.")).await;
                    }
                }
            }
            log::info!("[voice] écoute arrêtée");
        })
    }
}

/// Accumule des échantillons jusqu'au silence, puis transcrit la phrase entière.
async fn collect_utterance(
    app: &Arc<App>,
    runtime: &Arc<VoiceRuntime>,
    initial_samples: usize,
    rate: u32,
    hop: Duration,
    end_of_speech: Duration,
    min_duration: Duration,
) -> Option<String> {
    let mut buffer: Vec<f32> = runtime.take_window(initial_samples);
    let started = Instant::now();
    let mut silence = Duration::ZERO;

    while started.elapsed() < MAX_UTTERANCE {
        tokio::time::sleep(hop).await;
        let chunk = (rate as f64 * hop.as_secs_f64() * 2.0) as usize;
        let recent = runtime.take_window(chunk.max(rate as usize / 4));
        if recent.is_empty() {
            break;
        }
        let talking = crate::providers::stt::is_speech(
            &recent,
            app.settings().voice.vad_threshold * 1.4,
        );
        silence = if talking { Duration::ZERO } else { silence + hop };
        buffer.extend_from_slice(&recent);
        if silence >= end_of_speech && started.elapsed() > min_duration {
            break;
        }
    }
    transcribe_samples(app, &buffer, rate).await
}

async fn transcribe(
    app: &Arc<App>,
    runtime: &Arc<VoiceRuntime>,
    samples: usize,
    rate: u32,
) -> Option<String> {
    let window = runtime.take_window(samples);
    if window.is_empty() {
        return None;
    }
    transcribe_samples(app, &window, rate).await
}

async fn transcribe_samples(app: &Arc<App>, samples: &[f32], rate: u32) -> Option<String> {
    if samples.len() < rate as usize / 2 {
        return None;
    }
    let wav = window_to_wav(samples, rate);
    let guard = app.stt.lock().await;
    let stt = guard.as_ref()?;
    match stt.transcribe_wav(wav).await {
        Ok(text) => Some(text),
        Err(error) => {
            log::warn!("[voice] transcription impossible : {error}");
            None
        }
    }
}

/// Synthèse puis lecture. La lecture est bloquante pour `cpal`, donc isolée.
async fn speak(app: &Arc<App>, text: &str) {
    let settings: Settings = app.settings();
    if !settings.tts.enabled {
        return;
    }
    let cleaned = crate::providers::tts::prepare_for_speech(text);
    if cleaned.is_empty() {
        return;
    }
    let speech = match app
        .tts
        .speak(
            &app.secrets.openrouter_api_key,
            &settings.tts.model,
            &settings.tts.voice,
            &cleaned,
            settings.tts.chars_per_minute,
        )
        .await
    {
        Ok(speech) => speech,
        Err(error) => {
            log::warn!("[tts] {error}");
            return;
        }
    };
    let bytes = speech.bytes;
    if bytes.is_empty() {
        return;
    }
    let sample_rate = speech.sample_rate;
    let _ = app.avatar.say(text, speech.estimated_ms).await;
    let outcome = tokio::task::spawn_blocking(move || super::play_bytes(&bytes, sample_rate)).await;
    if let Ok(Err(error)) = outcome {
        log::warn!("[voice] lecture impossible : {error}");
    }
}

async fn emit(events: &Sender<AgentEvent>, event: AgentEvent) {
    let _ = events.send(event).await;
}