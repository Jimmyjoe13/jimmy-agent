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
use super::{is_stop_command, matches_wake_word, strip_wake_word, window_to_wav, VoiceRuntime};
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
/// Durée maximale de parole d'un segment. Le serveur de commande ne voit que
/// 12,8 s d'audio (`stt.command_audio_ctx` = 640) : au-delà, Whisper tronque.
/// Une parole plus longue est découpée en segments, recollés ensuite.
const MAX_SPEECH: Duration = Duration::from_secs(12);
/// Au-delà, le segment est coupé dès la première pause (100 ms) plutôt
/// qu'au milieu d'un mot à la limite.
const SOFT_MAX_SPEECH_MS: u64 = 10_000;
/// Pause qui suffit à couper un segment trop long.
const SOFT_CUT_SILENCE_MS: u64 = 100;
/// Segments au plus par commande (~1 min de parole).
const MAX_SEGMENTS: usize = 6;
/// Après une coupure à la limite, l'utilisateur parle encore : la suite doit
/// reprendre vite.
const CUT_WAIT: Duration = Duration::from_millis(1500);
/// Après une phrase qui semble inachevée (« ... », « que », « euh »), temps
/// laissé pour reprendre — en plus du silence de fin déjà écoulé.
const HESITATION_WAIT: Duration = Duration::from_millis(2500);
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
/// Intervalle minimal entre deux phrases de progression dites à voix haute.
const PROGRESS_EVERY: Duration = Duration::from_secs(20);
/// Silence au-delà duquel Jimmy dit qu'il travaille toujours.
const STILL_WORKING_AFTER: Duration = Duration::from_secs(45);
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

            // Session vocale portée par `App` (voir son commentaire) : elle
            // survit à un arrêt/relance de l'écoute, sinon une simple reprise
            // du micro faisait tout oublier à Jimmy.
            // Bruit de fond mesuré : le seuil de parole en est un multiple.
            let mut noise_floor: f32 = 0.002;
            // Seuil appris des fausses alertes : un bruit ambiant proche du
            // seuil (souffle, ventilateur) déclenchait la prise sans arrêt.
            // Chaque fausse alerte relève le seuil juste au-dessus de son
            // niveau ; il redescend lentement (~2 min) quand la pièce se calme.
            let mut learned_floor: f32 = 0.0;
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

                learned_floor *= 0.999;
                let live = self.settings();
                let window = runtime.take_window(onset_samples);
                if window.is_empty() {
                    continue;
                }
                let level = rms(&window);
                let threshold = vad_threshold(noise_floor, learned_floor, live.voice.vad_threshold);
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
                    learned_floor = learn_from_false_alarm(learned_floor, level);
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
                    learned_floor = learn_from_false_alarm(learned_floor, level);
                    restore(&events, follow_until).await;
                    continue;
                }
                // Arrêt d'urgence : « STOP » seul, sans le nom, à tout moment —
                // arrête une tâche lancée depuis le Chat, ou fait taire Jimmy.
                if is_stop_command(&utterance) || is_stop_command(&early) {
                    log::info!("[voice] arrêt d'urgence entendu : « {utterance} »");
                    if self.request_stop() {
                        self.speak(STOPPED_REPLY).await;
                    }
                    runtime.drain();
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
                if !name_alone {
                    // Phrase coupée par une hésitation ou par la limite de
                    // durée : on écoute la suite avant de répondre.
                    command = continue_speech(
                        &self,
                        &runtime,
                        &events,
                        rate,
                        live.voice.end_of_speech_ms,
                        threshold,
                        command,
                        captured.end_cursor,
                        captured.cut,
                    )
                    .await;
                }

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
                            Some((text, c.end_cursor, c.cut))
                        }
                        _ => None,
                    };
                    command = match heard {
                        Some((text, cursor, cut)) => {
                            let first = strip_wake_word(&strip_cue_echo(&text), &settings.stt.wake_word);
                            if first.trim().is_empty() {
                                first
                            } else {
                                continue_speech(
                                    &self,
                                    &runtime,
                                    &events,
                                    rate,
                                    live.voice.end_of_speech_ms,
                                    threshold,
                                    first,
                                    cursor,
                                    cut,
                                )
                                .await
                            }
                        }
                        None => String::new(),
                    };
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
                let ok = answer_command(&self, &events, &command, &runtime, rate, threshold).await;

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
    command: &str,
    runtime: &Arc<VoiceRuntime>,
    rate: u32,
    threshold: f32,
) -> bool {
    // Pendant toute la tâche et la réponse parlée, la boucle d'écoute attend :
    // un guetteur écoute le micro pour l'arrêt d'urgence (« STOP »).
    let stop_generation = app.stop_generation();
    let watcher = tokio::spawn(watch_for_stop(app.clone(), runtime.clone(), rate, threshold));
    // Conversation : même session tant que le dernier échange est récent.
    // L'état vit sur `App` : une relance de l'écoute ne coupe plus le fil.
    let session = {
        let mut guard = app.voice_session.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some((id, last)) if last.elapsed() < VOICE_SESSION_IDLE => {
                let id = id.clone();
                *guard = Some((id.clone(), Instant::now()));
                id
            }
            _ => {
                let id = app
                    .history
                    .create_session("Session vocale")
                    .unwrap_or_else(|_| String::from("vocale"));
                *guard = Some((id.clone(), Instant::now()));
                id
            }
        }
    };

    // Affichée dans le chat comme un message de l'utilisateur ; le Chat adopte
    // la session vocale (projet compris).
    emit(events, AgentEvent::Spoken { text: command.to_string(), session_id: session.clone() }).await;
    phase(events, "thinking", Duration::ZERO).await;

    let started = Instant::now();
    // Le projet de la session vocale (choisi dans le Chat) s'applique aussi.
    let (settings, tool_context) = app.session_context(&session);
    // Relais des événements : tout part vers l'interface, et la progression
    // d'une longue tâche est dite à voix haute, une phrase au plus toutes les
    // `PROGRESS_EVERY`. Une tâche de code prend plusieurs minutes : sans cela,
    // l'utilisateur n'entendait qu'« Un instant » puis un long silence.
    let (relay_tx, mut relay_rx) = tokio::sync::mpsc::channel::<AgentEvent>(64);
    let relay_events = events.clone();
    let narrator = app.clone();
    let relay = tokio::spawn(async move {
        let mut last_said = Instant::now();
        loop {
            // Un seul appel au modèle peut durer plus d'une minute (écriture
            // d'un fichier) sans aucun événement : signe de vie si rien n'a été
            // dit depuis `STILL_WORKING_AFTER`.
            let event = match tokio::time::timeout(STILL_WORKING_AFTER, relay_rx.recv()).await {
                Ok(Some(event)) => event,
                Ok(None) => break,
                Err(_) => {
                    if last_said.elapsed() >= STILL_WORKING_AFTER {
                        log::info!("[voice] progression : « Je travaille toujours dessus. »");
                        narrator.speak("Je travaille toujours dessus.").await;
                        last_said = Instant::now();
                    }
                    continue;
                }
            };
            if let AgentEvent::Progress { text } = &event {
                if last_said.elapsed() >= PROGRESS_EVERY {
                    log::info!("[voice] progression : « {text} »");
                    narrator.speak(text).await;
                    last_said = Instant::now();
                }
            }
            let _ = relay_events.send(event).await;
        }
    });
    // Interruptible par l'arrêt d'urgence (`App::cancellable`).
    let run = app.cancellable(crate::core::agent::run(
        app.deps_voice(),
        settings,
        session.clone(),
        command.to_string(),
        tool_context,
        relay_tx,
    ));
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
    // L'agent a fini (son émetteur est fermé) : on attend la fin d'une phrase
    // de progression en cours avant de dire la réponse, sans chevauchement.
    let _ = relay.await;

    let outcome = match answer {
        Ok(answer) => {
            phase(events, "speaking", Duration::ZERO).await;
            app.speak(&answer.text).await;
            // « STOP » pendant la réponse : Jimmy s'est tu, il le confirme.
            if app.stop_generation() != stop_generation {
                app.speak("D'accord, je t'écoute.").await;
            }
            // Le fil est relancé pour 10 min après la réponse.
            if let Ok(mut guard) = app.voice_session.lock() {
                *guard = Some((session, Instant::now()));
            }
            true
        }
        // Arrêt d'urgence pendant la tâche : on la note dans l'historique (le
        // modèle doit savoir qu'elle n'est pas faite) et on attend la suite.
        Err(crate::error::Error::Cancelled) => {
            log::info!("[voice] tâche arrêtée par l'arrêt d'urgence");
            let _ = app.history.append(
                &session,
                &crate::core::types::Message::assistant("(Tâche arrêtée à la demande de l'utilisateur, avant la fin.)"),
            );
            emit(events, AgentEvent::Final { text: STOPPED_REPLY.into() }).await;
            app.speak(STOPPED_REPLY).await;
            if let Ok(mut guard) = app.voice_session.lock() {
                *guard = Some((session, Instant::now()));
            }
            true
        }
        Err(error) => {
            emit(events, AgentEvent::Failed { message: error.to_string() }).await;
            // Dit à voix haute : court et compréhensible. Le JSON du fournisseur
            // était lu en entier (« HTTP 400 Bad Request — {"error"… »).
            app.speak(&spoken_error(&error)).await;
            false
        }
    };
    watcher.abort();
    outcome
}

/// Phrase dite après un arrêt d'urgence.
const STOPPED_REPLY: &str = "D'accord, j'arrête. Je t'écoute.";
/// Parole maximale d'un ordre d'arrêt : au-delà, ce n'est pas « STOP ».
const STOP_MAX_VOICED_MS: u64 = 1500;

/// Écoute le micro pendant qu'une tâche tourne (la boucle d'écoute est alors
/// en attente) : à chaque courte prise de parole, transcription rapide ; si
/// c'est « STOP », arrêt d'urgence. Les longues prises (Jimmy qui parle, une
/// phrase entière) sont ignorées.
async fn watch_for_stop(app: Arc<App>, runtime: Arc<VoiceRuntime>, rate: u32, threshold: f32) {
    let mut cursor = runtime.cursor();
    let mut buffer: Vec<f32> = Vec::new();
    let max_samples = rate as usize * 6;
    let pad = (rate as u64 * PAD_MS / 1000) as usize;
    loop {
        tokio::time::sleep(Duration::from_millis(150)).await;
        if !runtime.is_running() {
            return;
        }
        buffer.extend(runtime.read_since(&mut cursor));
        let mut tracker = SpeechTracker::new(rate, threshold, 400, 0);
        tracker.feed(&buffer);
        if tracker.ended() {
            let voiced = tracker.voiced_ms();
            if (MIN_VOICED_MS..=STOP_MAX_VOICED_MS).contains(&voiced) {
                let clip = tracker.clip(&buffer, pad).to_vec();
                let text = transcribe_fast_clip(&app, &clip, rate).await;
                log::debug!("[voice] guetteur d'arrêt : « {text} »");
                if is_stop_command(&text) {
                    log::info!("[voice] arrêt d'urgence entendu pendant la tâche : « {text} »");
                    app.request_stop();
                    return;
                }
            }
            buffer.clear();
        } else if buffer.len() > max_samples {
            // Prise trop longue (Jimmy qui parle) ou bruit : on repart d'un
            // tampon court pour ne garder que l'audio récent.
            let keep = buffer.len() - rate as usize;
            buffer.drain(..keep);
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
    /// La prise a été coupée par la limite de durée, pas par un silence :
    /// l'utilisateur parlait encore.
    cut: bool,
}

/// Écoute la suite d'une phrase tant qu'elle paraît coupée, et recolle les
/// morceaux.
///
/// Deux cas : la prise a atteint la limite de durée (`cut`, l'utilisateur
/// parlait encore), ou la transcription semble inachevée (« ... », « que »,
/// « euh » : une pause de réflexion a clos la prise). La suite est écoutée
/// depuis `cursor`, la fin de la prise précédente : ce qui a été dit pendant la
/// transcription est encore dans le tampon. Sans suite dans le délai, la
/// phrase part telle quelle. Phrase complète : aucune attente ajoutée.
#[allow(clippy::too_many_arguments)]
async fn continue_speech(
    app: &Arc<App>,
    runtime: &Arc<VoiceRuntime>,
    events: &Sender<AgentEvent>,
    rate: u32,
    end_of_speech_ms: u64,
    threshold: f32,
    mut text: String,
    mut cursor: u64,
    mut cut: bool,
) -> String {
    for _ in 1..MAX_SEGMENTS {
        if !cut && !looks_unfinished(&text) {
            break;
        }
        let wait = if cut { CUT_WAIT } else { HESITATION_WAIT };
        log::info!(
            "[voice] phrase {} : j'écoute la suite (« {text} »)",
            if cut { "longue, découpée" } else { "inachevée" }
        );
        emit(events, AgentEvent::State { state: AvatarState::Listening, detail: "Je t'écoute".into() }).await;
        phase(events, "your_turn", wait).await;
        let Some(next) = capture(runtime, cursor, cursor, rate, end_of_speech_ms, wait, threshold).await else {
            log::info!("[voice] pas de suite : la phrase part telle quelle");
            break;
        };
        phase(events, "transcribing", Duration::ZERO).await;
        emit(events, AgentEvent::State { state: AvatarState::Thinking, detail: "transcription".into() }).await;
        let more = transcribe_precise(app, &next.samples, rate).await;
        dump_clip(app, &next.samples, rate, "suite", &more);
        log::info!("[voice] suite : « {more} » ({} ms de parole)", next.voiced_ms);
        text = join_segments(&text, &more);
        cursor = next.end_cursor;
        cut = next.cut;
    }
    text
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

    let soft_samples = (rate as u64 * SOFT_MAX_SPEECH_MS / 1000) as usize;
    let mut cut = false;
    loop {
        if tracker.ended() {
            break;
        }
        if tracker.looks_like_noise(buffer.len(), rate) {
            log::info!("[voice] bruit ambiant (parole éparse) : prise abandonnée");
            return None;
        }
        // Le temps est celui de l'audio reçu, pas l'horloge.
        match tracker.speech_start() {
            None if buffer.len() > wait_samples => return None,
            Some(first) if buffer.len() > first + max_samples => {
                cut = true;
                break;
            }
            // Segment déjà long : on coupe dans la première pause, la suite
            // sera écoutée et recollée (`continue_speech`).
            Some(first) if buffer.len() > first + soft_samples && tracker.silent_ms() >= SOFT_CUT_SILENCE_MS => {
                cut = true;
                break;
            }
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
        cut,
    })
}

/// Seuil de parole : 3× le bruit de fond, ou le niveau appris des fausses
/// alertes s'il est plus haut ; entre 0,0035 et le plafond réglé.
pub fn vad_threshold(noise_floor: f32, learned_floor: f32, ceiling: f32) -> f32 {
    (noise_floor * 3.0).max(learned_floor).clamp(0.0035, ceiling.max(0.0035))
}

/// Relève le seuil appris juste au-dessus du niveau d'une fausse alerte, sans
/// jamais dépasser 0,007 : une claque de porte ne doit pas rendre Jimmy sourd
/// à une voix douce (0,005 à 0,03 mesuré).
fn learn_from_false_alarm(learned: f32, level: f32) -> f32 {
    learned.max(level * 1.25).min(0.007)
}

/// Message d'erreur dit à voix haute. Le détail technique (codes HTTP, JSON du
/// fournisseur) reste dans le chat et dans le journal.
pub fn spoken_error(error: &crate::Error) -> String {
    use crate::Error;
    let raison = match error {
        Error::Provider { .. } => "le service du modèle n'a pas répondu correctement",
        Error::Timeout => "la demande a pris trop de temps",
        Error::PermissionDenied(_) => "je n'ai pas la permission de faire ça",
        Error::Http(_) => "la connexion a échoué",
        _ => "une erreur est survenue",
    };
    format!("Désolé, {raison}. Les détails sont dans le chat.")
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

/// Mots qui ne terminent jamais une phrase française : une transcription qui
/// finit par l'un d'eux a été coupée par une hésitation.
const DANGLING_WORDS: &[&str] = &[
    "euh", "heu", "hum", "bah", "que", "qu'", "de", "du", "des", "d'", "le", "la", "les", "l'", "un", "une",
    "et", "ou", "mais", "à", "au", "aux", "pour", "dans", "avec", "sur", "par", "mon", "ma", "mes", "ton",
    "ta", "tes", "son", "sa", "ses", "ce", "cette", "ces", "quand", "comme", "parce", "je", "j'", "tu",
    "il", "elle", "est-ce",
];

/// La transcription semble-t-elle coupée au milieu d'une phrase ?
///
/// Whisper ajoute un point à presque tout, même à une phrase coupée : seuls
/// « ? » et « ! » prouvent une fin. Une coupure se reconnaît aux points de
/// suspension (que Whisper met lui-même), à une virgule finale, ou à un
/// dernier mot qui ne termine jamais une phrase (« que », « de », « euh »…).
pub fn looks_unfinished(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() || trimmed.ends_with('?') || trimmed.ends_with('!') {
        return false;
    }
    if trimmed.ends_with("...") || trimmed.ends_with('…') || trimmed.ends_with(',') {
        return true;
    }
    // Dernier mot entier, sans la ponctuation finale. « Dis-le » reste un seul
    // mot : un pronom accroché par un trait d'union ne compte pas.
    let last = trimmed
        .split_whitespace()
        .last()
        .unwrap_or("")
        .trim_end_matches(|c: char| !c.is_alphanumeric() && c != '\'')
        .to_lowercase();
    DANGLING_WORDS.contains(&last.as_str())
}

/// Recolle la suite d'une phrase à son début. Si le début était coupé, ses
/// points de suspension (ou le point ajouté par Whisper) disparaissent et la
/// suite reprend en minuscule : c'est une seule phrase.
pub fn join_segments(head: &str, tail: &str) -> String {
    let head = head.trim();
    let tail = tail.trim();
    if head.is_empty() {
        return tail.to_string();
    }
    if tail.is_empty() {
        return head.to_string();
    }
    if !looks_unfinished(head) {
        return format!("{head} {tail}");
    }
    let head = head.trim_end_matches(['.', '…', ',']).trim_end();
    let mut chars = tail.chars();
    let first = chars.next().map(|c| c.to_lowercase().collect::<String>()).unwrap_or_default();
    format!("{head} {first}{}", chars.as_str())
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
    fn une_phrase_inachevee_est_reconnue() {
        // Cas réel : Whisper marque lui-même la coupure par « ... ».
        assert!(looks_unfinished("Je viens de voir que dans ta mémoire tu as mis..."));
        assert!(looks_unfinished("Ouvre le dossier de"));
        assert!(looks_unfinished("Cherche dans le vault la note sur le."));
        assert!(looks_unfinished("Euh,"));
        assert!(looks_unfinished("Regarde, euh"));
        // Phrases complètes : aucune attente ajoutée.
        assert!(!looks_unfinished("Non, c'est bon."));
        assert!(!looks_unfinished("Quelle heure est-il ?"));
        assert!(!looks_unfinished("Et donc ?"));
        assert!(!looks_unfinished("Dis-le."));
        assert!(!looks_unfinished("On tue le processus warp."));
        assert!(!looks_unfinished(""));
    }

    #[test]
    fn les_segments_sont_recolles() {
        assert_eq!(
            join_segments("Je viens de voir que dans ta mémoire tu as mis...", "Des choses fausses."),
            "Je viens de voir que dans ta mémoire tu as mis des choses fausses."
        );
        assert_eq!(join_segments("Ouvre le dossier de.", "Projet."), "Ouvre le dossier de projet.");
        // Coupure à la limite de durée après une phrase complète : rien ne change.
        assert_eq!(join_segments("Fais ceci.", "Ensuite cela."), "Fais ceci. Ensuite cela.");
        assert_eq!(join_segments("", "Bonjour."), "Bonjour.");
        assert_eq!(join_segments("Bonjour.", "  "), "Bonjour.");
    }

    #[test]
    fn l_erreur_dite_a_voix_haute_est_courte_et_sans_json() {
        let erreur = crate::Error::provider(
            "OpenCode Go",
            "HTTP 400 Bad Request — {\"error\":{\"type\":\"invalid_request_error\"}}",
        );
        let dit = spoken_error(&erreur);
        assert!(!dit.contains('{') && !dit.contains("HTTP") && !dit.contains("400"), "{dit}");
        assert!(dit.chars().count() < 100, "{dit}");
        assert!(spoken_error(&crate::Error::Timeout).contains("trop de temps"));
    }

    #[test]
    fn seuil_adaptatif_borne() {
        // Micro très silencieux : plancher à 0,0035.
        assert_eq!(vad_threshold(0.0005, 0.0, 0.012), 0.0035);
        // Pièce bruyante : 3× le bruit de fond.
        assert!((vad_threshold(0.002, 0.0, 0.012) - 0.006).abs() < 1e-6);
        // Jamais au-dessus du plafond réglé.
        assert_eq!(vad_threshold(0.02, 0.0, 0.012), 0.012);
    }

    #[test]
    fn le_seuil_apprend_des_fausses_alertes() {
        // Fausse alerte à 0,0036 (seuil de 0,0035) : le seuil passe à 0,0045.
        let learned = learn_from_false_alarm(0.0, 0.0036);
        assert!((learned - 0.0045).abs() < 1e-6);
        assert!(vad_threshold(0.0010, learned, 0.012) > 0.0036);
        // Une claque (0,05) ne rend pas Jimmy sourd : plafonné à 0,007.
        assert_eq!(learn_from_false_alarm(0.0, 0.05), 0.007);
        // Un niveau déjà appris plus haut n'est pas abaissé.
        assert_eq!(learn_from_false_alarm(0.006, 0.0036), 0.006);
    }
}
