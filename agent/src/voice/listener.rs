//! Boucle d'écoute permanente : c'est le cœur du produit.
//!
//! ```text
//! veille ─ parole détectée ─▶ « Jimmy » ? ─▶ commande ─▶ agent ─▶ voix ─┐
//!    ▲                                                                   │
//!    └──── 8 s sans parole ◀── conversation continue (sans « Jimmy ») ◀──┘
//! ```
//!
//! Décisions structurantes, toutes dictées par des mesures sur la machine :
//!
//! * **le VAD évite de transcrire le silence** ([`super::vad`]) : détection
//!   par trames de 50 ms, avec confirmation (un claquement n'est pas de la
//!   parole) et seuil adaptatif au bruit de fond ;
//! * **deux modèles** : `base` réagit vite pendant que l'utilisateur parle,
//!   `small` transcrit la phrase entière et décide. Quand `base` a reconnu
//!   « Jimmy » et que la prise est brève (le nom seul), `small` n'est pas
//!   appelé : « Oui ? » part sans attendre une transcription de plus ;
//! * **l'audio est rogné** avant Whisper : parole + 300 ms de marge. Le
//!   silence autour est ce qui lui fait inventer du texte ;
//! * **conversation continue** : après une réponse, Jimmy écoute encore
//!   quelques secondes sans qu'il faille redire son nom ;
//! * **l'utilisateur voit toujours ce que Jimmy attend** : chaque étape émet
//!   un événement `Listen` (je t'entends, je transcris, à toi, je réfléchis…).

use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::mpsc::Sender;

use super::vad::{frame_len, rms, voiced_fraction, SpeechTracker};
use super::{matches_wake_word, strip_wake_word, window_to_wav, VoiceRuntime};
use crate::core::types::{AgentEvent, AvatarState};
use crate::App;

/// Fréquence de scrutation du micro en veille.
const POLL: Duration = Duration::from_millis(100);
/// Fenêtre d'analyse du déclenchement.
const ONSET_WINDOW_MS: u64 = 600;
/// Part minimale de trames voisées dans cette fenêtre pour déclencher.
const ONSET_VOICED_FRACTION: f32 = 0.30;
/// Audio conservé avant le déclenchement : le début de la phrase.
const PRE_ROLL_MS: u64 = 1000;
/// Marge gardée autour de la parole dans l'extrait envoyé à Whisper.
const PAD_MS: u64 = 300;
/// Durée maximale de parole d'une phrase.
const MAX_SPEECH: Duration = Duration::from_secs(12);
/// Après un déclenchement, la parole doit être confirmée dans ce délai.
const FIRST_SPEECH_WAIT: Duration = Duration::from_secs(3);
/// Après « Oui ? », temps laissé à l'utilisateur pour commencer à parler.
const AFTER_CUE_WAIT: Duration = Duration::from_millis(4500);
/// Parole confirmée minimale pour qu'un extrait soit transcrit.
const MIN_VOICED_MS: u64 = 250;
/// En dessous, une prise reconnue comme « Jimmy » par le modèle rapide est
/// le nom seul : inutile de relancer le modèle précis.
const NAME_ALONE_MAX_MS: u64 = 900;
/// Après avoir parlé, la salle résonne encore : on laisse passer ce délai
/// avant de réécouter (la voix de Jimmy ne doit pas se déclencher elle-même).
const SPEAKER_TAIL: Duration = Duration::from_millis(300);
/// Délai au-delà duquel Jimmy dit « Un instant. » pendant que l'agent travaille.
const THINKING_CUE_AFTER: Duration = Duration::from_secs(5);
/// Silence au-delà duquel une nouvelle commande ouvre une nouvelle session.
const VOICE_SESSION_IDLE: Duration = Duration::from_secs(600);

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
            let window_samples =
                (rate as f64 * Duration::from_millis(settings.stt.wake_window_ms.max(600)).as_secs_f64()) as usize;
            let onset_samples = (rate as u64 * ONSET_WINDOW_MS / 1000) as usize;
            let pre_roll = rate as u64 * PRE_ROLL_MS / 1000;
            let frame = frame_len(rate);

            // Une seule boucle par démarrage du micro : si l'écoute est
            // arrêtée puis relancée, la génération change et cette boucle-ci
            // s'arrête, même si `is_running` est de nouveau vrai.
            let generation = runtime.generation();
            let alive = |runtime: &VoiceRuntime| runtime.is_running() && runtime.generation() == generation;
            log::info!("[voice] écoute démarrée — mot d'activation « {} »", settings.stt.wake_word);

            // Session vocale réutilisée tant que la conversation continue.
            let mut voice_session: Option<(String, Instant)> = None;
            // Bruit de fond mesuré : le seuil de parole en est un multiple.
            let mut noise_floor: f32 = 0.002;
            // Fin de la fenêtre de conversation continue, si elle est ouverte.
            let mut follow_until: Option<Instant> = None;
            phase(&events, "idle", Duration::ZERO).await;

            while alive(&runtime) {
                tokio::time::sleep(POLL).await;
                // Jimmy parle (réponse du chat, synthèse) : on n'écoute pas
                // sa propre voix, et on repart d'un tampon vide ensuite.
                if self.is_speaking() {
                    runtime.drain();
                    continue;
                }

                // La fenêtre de conversation continue s'est refermée.
                if let Some(until) = follow_until {
                    if Instant::now() >= until {
                        follow_until = None;
                        log::info!("[voice] fin de la conversation continue (silence)");
                        let _ = self.avatar.hide_bubble().await;
                        emit(&events, AgentEvent::State { state: AvatarState::Idle, detail: String::new() }).await;
                        phase(&events, "idle", Duration::ZERO).await;
                    }
                }

                let live = self.settings();
                let window = runtime.take_window(onset_samples);
                if window.is_empty() {
                    continue;
                }
                let level = rms(&window);
                let threshold = vad_threshold(noise_floor, live.voice.vad_threshold);
                let voiced = voiced_fraction(&window, frame, threshold);
                if level <= threshold || voiced < ONSET_VOICED_FRACTION {
                    // Pas de parole : le bruit de fond s'adapte (lentement).
                    if level <= threshold && level > 0.0 {
                        noise_floor = noise_floor * 0.98 + level * 0.02;
                    }
                    continue;
                }
                *self.last_vad_threshold.lock().unwrap() = threshold;

                let detected_at = Instant::now();
                let in_conversation = follow_until.is_some();
                // Début de la prise, marqué *avant* toute transcription : si
                // Whisper traîne, le début de la phrase ne doit pas en sortir.
                let segment_start = runtime.cursor().saturating_sub(pre_roll);
                log::info!(
                    "[voice] parole détectée (niveau {level:.4}, seuil {threshold:.4}, {:.0} % de trames voisées{})",
                    voiced * 100.0,
                    if in_conversation { ", conversation continue" } else { "" }
                );
                phase(&events, "capturing", Duration::ZERO).await;

                // 1. Détection rapide (`base`) pendant que l'utilisateur parle
                //    encore — hors conversation, où le nom n'est pas requis.
                let mut early = String::new();
                let mut early_match = false;
                if !in_conversation {
                    early = transcribe_fast(&self, &runtime, window_samples, rate).await;
                    if !alive(&runtime) {
                        continue;
                    }
                    early_match = matches_wake_word(&early, &settings.stt.wake_word);
                    if !early.is_empty() {
                        emit(&events, AgentEvent::Heard { text: early.clone(), matched: early_match }).await;
                    }
                    if early_match {
                        log::info!("[voice] wake word entendu (rapide) : « {early} »");
                        emit(&events, AgentEvent::State { state: AvatarState::Listening, detail: early.clone() }).await;
                    }
                }

                // 2. Prise complète jusqu'au silence.
                let Some(captured) = capture(
                    &runtime,
                    segment_start,
                    segment_start,
                    rate,
                    live.voice.end_of_speech_ms,
                    FIRST_SPEECH_WAIT,
                    threshold,
                )
                .await
                else {
                    log::info!("[voice] fausse alerte : pas de parole confirmée");
                    restore(&events, follow_until).await;
                    continue;
                };
                phase(&events, "transcribing", Duration::ZERO).await;
                if in_conversation || early_match {
                    // L'utilisateur s'adresse bien à Jimmy : il sait qu'on le traite.
                    emit(&events, AgentEvent::State { state: AvatarState::Thinking, detail: "transcription".into() }).await;
                }

                // 3. Qu'a-t-il dit ? « Jimmy » seul et reconnu par le modèle
                //    rapide : on ne relance pas le modèle précis.
                let utterance = if early_match && captured.voiced_ms <= NAME_ALONE_MAX_MS {
                    early.clone()
                } else if !in_conversation && captured.voiced_ms <= NAME_ALONE_MAX_MS {
                    // Prise brève que le modèle rapide n'avait pas reconnue
                    // pendant la parole : on le réessaie sur l'extrait rogné
                    // (0,5 s) avant de payer le modèle précis (2 à 3 s).
                    let quick = transcribe_fast_clip(&self, &captured.samples, rate).await;
                    if matches_wake_word(&quick, &settings.stt.wake_word) {
                        quick
                    } else {
                        transcribe_precise(&self, &captured.samples, rate).await
                    }
                } else {
                    transcribe_precise(&self, &captured.samples, rate).await
                };
                dump_clip(&self, &captured.samples, rate, "phrase", &utterance);
                let full_match = matches_wake_word(&utterance, &settings.stt.wake_word);
                log::info!(
                    "[voice] phrase en {} ms ({} ms de parole) : « {utterance} » (rapide : « {early} »)",
                    detected_at.elapsed().as_millis(),
                    captured.voiced_ms
                );
                if utterance.trim().is_empty() {
                    log::info!("[voice] rien d'intelligible dans cette prise (bruit)");
                    restore(&events, follow_until).await;
                    continue;
                }
                let addressed = early_match || full_match || in_conversation;
                if !addressed {
                    log::debug!("[voice] parole sans wake word : « {utterance} »");
                    emit(&events, AgentEvent::Heard { text: utterance.clone(), matched: false }).await;
                    restore(&events, follow_until).await;
                    continue;
                }
                if in_conversation {
                    emit(&events, AgentEvent::Heard { text: utterance.clone(), matched: true }).await;
                } else if !early_match {
                    // Le modèle rapide l'avait raté : la phrase entière l'a reconnu.
                    log::info!("[voice] wake word entendu (phrase entière) : « {utterance} »");
                    emit(&events, AgentEvent::Heard { text: utterance.clone(), matched: true }).await;
                    emit(&events, AgentEvent::State { state: AvatarState::Listening, detail: utterance.clone() }).await;
                }

                let mut command = strip_wake_word(&utterance, &settings.stt.wake_word);
                let name_alone = (early_match || full_match) && command.trim().is_empty();

                // 4. « Jimmy » seul : Jimmy répond « Oui ? » puis écoute.
                if name_alone {
                    // On écoute depuis la *fin de la prise*, pas depuis maintenant :
                    // pendant la pause qui suit le nom, la transcription et le
                    // « Oui ? » (2 à 4 s), l'utilisateur a pu enchaîner sa
                    // commande. Elle est encore dans le tampon (mesuré : sinon
                    // « Jimmy… dis-moi bonjour » perdait la commande).
                    let from = captured.end_cursor;
                    super::cues::play(&self, super::cues::Cue::Listening).await;
                    let armed = runtime.cursor();
                    emit(&events, AgentEvent::State { state: AvatarState::Listening, detail: "Oui ?".into() }).await;
                    phase(&events, "your_turn", AFTER_CUE_WAIT).await;
                    log::info!("[voice] « Oui ? » joué, écoute de la commande");
                    let heard = match capture(
                        &runtime,
                        from,
                        armed,
                        rate,
                        live.voice.end_of_speech_ms,
                        AFTER_CUE_WAIT,
                        threshold,
                    )
                    .await
                    {
                        Some(c) if c.voiced_ms >= MIN_VOICED_MS => {
                            phase(&events, "transcribing", Duration::ZERO).await;
                            emit(&events, AgentEvent::State { state: AvatarState::Thinking, detail: "transcription".into() }).await;
                            let text = transcribe_precise(&self, &c.samples, rate).await;
                            dump_clip(&self, &c.samples, rate, "commande", &text);
                            text
                        }
                        _ => String::new(),
                    };
                    command = strip_wake_word(&strip_cue_echo(&heard), &settings.stt.wake_word);
                    if command.trim().is_empty() {
                        log::info!("[voice] rien entendu après le nom");
                        emit(&events, AgentEvent::Notice { message: "Je n'ai rien entendu après « Jimmy ».".into() }).await;
                        emit(&events, AgentEvent::State { state: AvatarState::Idle, detail: String::new() }).await;
                        phase(&events, "idle", Duration::ZERO).await;
                        // Sinon le même « Jimmy » serait redétecté au tour suivant.
                        runtime.drain();
                        continue;
                    }
                }

                log::info!(
                    "[voice] commande : « {command} » ({} ms après la détection)",
                    detected_at.elapsed().as_millis()
                );

                // 5. L'agent répond, Jimmy le dit à voix haute.
                follow_until = None;
                let ok = answer_command(&self, &events, &mut voice_session, &command).await;

                // 6. Après la réponse : la salle résonne encore, puis la
                //    conversation continue s'ouvre — sans qu'il faille redire
                //    « Jimmy ». Le tampon contient la voix de Jimmy lui-même :
                //    on repart d'un tampon vide.
                tokio::time::sleep(SPEAKER_TAIL).await;
                runtime.drain();
                let window_ms = self.settings().voice.follow_up_ms;
                if ok && window_ms > 0 {
                    let window = Duration::from_millis(window_ms);
                    follow_until = Some(Instant::now() + window);
                    emit(&events, AgentEvent::State { state: AvatarState::Listening, detail: "À toi".into() }).await;
                    phase(&events, "your_turn", window).await;
                    log::info!("[voice] conversation continue ouverte ({window_ms} ms)");
                } else {
                    emit(&events, AgentEvent::State { state: AvatarState::Idle, detail: String::new() }).await;
                    phase(&events, "idle", Duration::ZERO).await;
                }
            }
            log::info!("[voice] écoute arrêtée");
        })
    }
}

/// Transmet la commande à l'agent (mode vocal), puis la réponse à la voix.
/// Renvoie `true` si l'agent a répondu.
async fn answer_command(
    app: &Arc<App>,
    events: &Sender<AgentEvent>,
    voice_session: &mut Option<(String, Instant)>,
    command: &str,
) -> bool {
    // Affichée dans le chat comme un message de l'utilisateur.
    emit(events, AgentEvent::Spoken { text: command.to_string() }).await;
    phase(events, "thinking", Duration::ZERO).await;

    // Conversation : même session tant que le dernier échange est récent.
    let session = match voice_session {
        Some((id, last)) if last.elapsed() < VOICE_SESSION_IDLE => id.clone(),
        _ => app
            .history
            .create_session("Session vocale")
            .unwrap_or_else(|_| String::from("vocale")),
    };
    *voice_session = Some((session.clone(), Instant::now()));

    let started = Instant::now();
    let run = crate::core::agent::run(
        app.deps_voice(),
        app.settings(),
        session.clone(),
        command.to_string(),
        app.tool_context(),
        events.clone(),
    );
    tokio::pin!(run);
    // Le modèle gratuit répond en 3 s ou en 25 s selon l'heure : si la réponse
    // tarde, Jimmy le dit (« Un instant. ») au lieu de laisser un silence qui
    // ressemble à une panne. L'agent continue pendant ce temps.
    let answer = tokio::select! {
        result = &mut run => result,
        _ = tokio::time::sleep(THINKING_CUE_AFTER) => {
            log::info!("[voice] réponse lente : « Un instant. »");
            super::cues::play(app, super::cues::Cue::Thinking).await;
            (&mut run).await
        }
    };
    log::info!("[voice] réponse de l'agent en {} ms", started.elapsed().as_millis());

    match answer {
        Ok(answer) => {
            phase(events, "speaking", Duration::ZERO).await;
            app.speak(&answer.text).await;
            *voice_session = Some((session, Instant::now()));
            true
        }
        Err(error) => {
            emit(events, AgentEvent::Failed { message: error.to_string() }).await;
            app.speak(&format!("Désolé, je n'ai pas pu faire ça : {error}.")).await;
            false
        }
    }
}

/// Retour à l'état d'attente qui convient : la fenêtre de conversation si
/// elle est encore ouverte, la veille sinon.
async fn restore(events: &Sender<AgentEvent>, follow_until: Option<Instant>) {
    match follow_until {
        Some(until) if Instant::now() < until => {
            // L'avatar avait pris la pose « réflexion » pendant la
            // transcription : il revient à l'écoute, sinon il y reste jusqu'à
            // la fin de la fenêtre.
            emit(events, AgentEvent::State { state: AvatarState::Listening, detail: "À toi".into() }).await;
            phase(events, "your_turn", until - Instant::now()).await;
        }
        _ => {
            emit(events, AgentEvent::State { state: AvatarState::Idle, detail: String::new() }).await;
            phase(events, "idle", Duration::ZERO).await;
        }
    }
}

async fn phase(events: &Sender<AgentEvent>, phase: &str, remaining: Duration) {
    emit(events, AgentEvent::Listen { phase: phase.into(), remaining: remaining.as_millis() as u64 }).await;
}

/// Une prise de parole : l'extrait rogné et la parole confirmée qu'il contient.
struct Captured {
    samples: Vec<f32>,
    voiced_ms: u64,
    /// Curseur de fin : tout l'audio avant a été analysé. Ce qui arrive
    /// ensuite (pendant la transcription, le « Oui ? ») reste à entendre.
    end_cursor: u64,
}

/// Accumule l'audio à partir du curseur `start` jusqu'à la fin de la phrase
/// (silence après parole), et rend l'extrait rogné.
///
/// * `armed_after` : curseur avant lequel la parole ne « démarre » pas la
///   prise (écho possible du « Oui ? ») ; elle reste dans l'extrait si
///   l'utilisateur parle ensuite ;
/// * `wait_for_start` : temps laissé pour que la parole commence ; passé ce
///   délai sans parole, la prise est abandonnée (rien à transcrire : Whisper
///   inventerait du texte).
async fn capture(
    runtime: &Arc<VoiceRuntime>,
    start: u64,
    armed_after: u64,
    rate: u32,
    end_of_speech_ms: u64,
    wait_for_start: Duration,
    threshold: f32,
) -> Option<Captured> {
    let mut cursor = start;
    let mut buffer: Vec<f32> = runtime.read_since(&mut cursor);
    let armed_at = armed_after.saturating_sub(start) as usize;
    let mut tracker = SpeechTracker::new(rate, threshold, end_of_speech_ms, armed_at);
    tracker.feed(&buffer);

    let wait_samples = armed_at + (rate as f64 * wait_for_start.as_secs_f64()) as usize;
    let max_samples = (rate as f64 * MAX_SPEECH.as_secs_f64()) as usize;
    // Garde-fou si le micro cesse de livrer de l'audio sans s'arrêter.
    let wall_limit = Instant::now() + wait_for_start + MAX_SPEECH + Duration::from_secs(5);

    loop {
        if tracker.ended() {
            break;
        }
        // Le temps est celui de l'audio reçu, pas l'horloge.
        match tracker.speech_start() {
            None if buffer.len() > wait_samples => return None,
            Some(first) if buffer.len() > first + max_samples => break,
            _ => {}
        }
        if !runtime.is_running() || Instant::now() > wall_limit {
            return None;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        buffer.extend(runtime.read_since(&mut cursor));
        tracker.feed(&buffer);
    }

    if tracker.voiced_ms() < MIN_VOICED_MS {
        return None;
    }
    let pad = (rate as u64 * PAD_MS / 1000) as usize;
    Some(Captured {
        samples: tracker.clip(&buffer, pad).to_vec(),
        voiced_ms: tracker.voiced_ms(),
        end_cursor: cursor,
    })
}

/// Seuil de parole : 3× le bruit de fond, entre 0,0035 et le plafond réglé.
pub fn vad_threshold(noise_floor: f32, ceiling: f32) -> f32 {
    (noise_floor * 3.0).clamp(0.0035, ceiling.max(0.0035))
}

/// Retire le « Oui ? » de Jimmy capté par le micro en tête de commande.
pub fn strip_cue_echo(text: &str) -> String {
    let trimmed = text.trim();
    let lower = trimmed.to_lowercase();
    for echo in ["oui ?", "oui?", "oui.", "oui,", "oui !", "oui"] {
        if lower.starts_with(echo) {
            let rest = &trimmed[echo.len()..];
            // « ouïe », « ouistiti »… : seul un mot entier est un écho.
            if echo == "oui" && rest.chars().next().is_some_and(|c| c.is_alphanumeric()) {
                return trimmed.to_string();
            }
            return rest.trim_start_matches(|c: char| !c.is_alphanumeric()).to_string();
        }
    }
    trimmed.to_string()
}

/// Transcription rapide (`base`) de la fenêtre la plus récente.
async fn transcribe_fast(app: &Arc<App>, runtime: &Arc<VoiceRuntime>, samples: usize, rate: u32) -> String {
    let window = runtime.take_window(samples);
    if window.len() < rate as usize / 2 {
        return String::new();
    }
    match app.transcribe_wake(window_to_wav(&window, rate)).await {
        Ok(text) => text,
        Err(error) => {
            log::warn!("[voice] transcription rapide impossible : {error}");
            String::new()
        }
    }
}

/// Transcription rapide (`base`) d'un extrait déjà rogné.
async fn transcribe_fast_clip(app: &Arc<App>, samples: &[f32], rate: u32) -> String {
    if samples.len() < rate as usize / 4 {
        return String::new();
    }
    match app.transcribe_wake(window_to_wav(samples, rate)).await {
        Ok(text) => text,
        Err(error) => {
            log::warn!("[voice] transcription rapide impossible : {error}");
            String::new()
        }
    }
}

/// Transcription précise (`small`) d'un extrait.
async fn transcribe_precise(app: &Arc<App>, samples: &[f32], rate: u32) -> String {
    if samples.len() < rate as usize / 4 {
        return String::new();
    }
    match app.transcribe_command(window_to_wav(samples, rate)).await {
        Ok(text) => text,
        Err(error) => {
            log::warn!("[voice] transcription impossible : {error}");
            String::new()
        }
    }
}

/// Garde l'extrait audio (réglage `voice.debug_audio`) : les 40 derniers, en
/// local, pour comprendre une transcription surprenante.
fn dump_clip(app: &App, samples: &[f32], rate: u32, label: &str, text: &str) {
    if !app.settings().voice.debug_audio || samples.is_empty() {
        return;
    }
    let dir = app.paths.audio_dir().join("debug");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let slug: String = text
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-")
        .chars()
        .take(40)
        .collect();
    let name = format!("{}-{label}-{slug}.wav", chrono::Local::now().format("%H%M%S%3f"));
    if std::fs::write(dir.join(name), window_to_wav(samples, rate)).is_err() {
        return;
    }
    // Rotation : on ne garde que les 40 derniers.
    if let Ok(entries) = std::fs::read_dir(&dir) {
        let mut files: Vec<_> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        files.sort();
        let excess = files.len().saturating_sub(40);
        for old in files.into_iter().take(excess) {
            let _ = std::fs::remove_file(old);
        }
    }
}

async fn emit(events: &Sender<AgentEvent>, event: AgentEvent) {
    let _ = events.send(event).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn echo_du_oui_retire() {
        assert_eq!(strip_cue_echo("Oui ? Comment ça va ?"), "Comment ça va ?");
        assert_eq!(strip_cue_echo("oui, quelle heure est-il"), "quelle heure est-il");
        assert_eq!(strip_cue_echo("Ouistiti en vue"), "Ouistiti en vue");
        assert_eq!(strip_cue_echo("Quelle heure est-il ?"), "Quelle heure est-il ?");
    }

    #[test]
    fn seuil_adaptatif_borne() {
        // Micro très silencieux : plancher à 0,0035.
        assert_eq!(vad_threshold(0.0005, 0.012), 0.0035);
        // Pièce bruyante : 3× le bruit de fond.
        assert!((vad_threshold(0.002, 0.012) - 0.006).abs() < 1e-6);
        // Jamais au-dessus du plafond réglé.
        assert_eq!(vad_threshold(0.02, 0.012), 0.012);
    }
}
