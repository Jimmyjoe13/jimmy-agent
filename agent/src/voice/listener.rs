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

            // Une seule boucle par démarrage du micro : si l'écoute est
            // arrêtée puis relancée, la génération change et cette boucle-ci
            // s'arrête, même si `is_running` est de nouveau vrai.
            let generation = runtime.generation();
            let alive = |runtime: &VoiceRuntime| runtime.is_running() && runtime.generation() == generation;
            log::info!("[voice] écoute démarrée — mot d'activation « {} »", settings.stt.wake_word);

            let window_samples = (rate as f64 * wake_window.as_secs_f64()) as usize;
            // Session vocale réutilisée tant que la conversation continue.
            // Avant, chaque commande ouvrait une session neuve : Jimmy
            // oubliait l'échange précédent, aucune conversation possible.
            let mut voice_session: Option<(String, Instant)> = None;
            // Le VAD regarde le passé récent (2 pas) : sur toute la fenêtre de
            // 2,4 s, un « Jimmy » de 0,5 s était dilué sous le seuil.
            let recent_samples = (rate as f64 * hop.as_secs_f64() * 2.0) as usize;

            // VAD adaptatif : le seuil suit le bruit de fond mesuré (3×),
            // plafonné par le réglage. Un seuil fixe de 0,012 ne se
            // déclenchait jamais sur un micro peu sensible (micro intégré
            // mesuré à 0,002 au repos, 0,004 avec de la parole à distance).
            let mut noise_floor: f32 = 0.002;

            while alive(&runtime) {
                tokio::time::sleep(hop).await;
                // Jimmy parle (réponse du chat, synthèse) : on n'écoute pas
                // sa propre voix, et on repart d'un tampon vide ensuite.
                if self.is_speaking() {
                    runtime.drain();
                    continue;
                }
                let level = runtime.level(recent_samples);
                let threshold = vad_threshold(noise_floor, settings.voice.vad_threshold);
                if level <= threshold {
                    // Pas de parole : le bruit de fond s'adapte (lentement).
                    if level > 0.0 {
                        noise_floor = noise_floor * 0.9 + level * 0.1;
                    }
                    continue;
                }
                *self.last_vad_threshold.lock().unwrap() = threshold;

                // Début de la phrase, marqué *avant* toute transcription. Piège
                // corrigé : la fenêtre était prise après la transcription
                // rapide ; si Whisper traînait (machine chargée), le début de
                // la phrase — donc « Jimmy » — en sortait et se perdait.
                let segment_start = runtime.cursor().saturating_sub(window_samples as u64);
                let detected_at = Instant::now();
                log::info!("[voice] parole détectée (niveau {level:.4}, seuil {threshold:.4})");

                // 1. Détection rapide (`base`, fenêtre courte) : réagir pendant
                //    que l'utilisateur parle encore.
                let early = transcribe(&self, &runtime, window_samples, rate).await.unwrap_or_default();
                if !alive(&runtime) {
                    continue;
                }
                let early_match = !early.is_empty() && matches_wake_word(&early, &settings.stt.wake_word);
                if !early.is_empty() {
                    emit(&events, AgentEvent::Heard { text: early.clone(), matched: early_match }).await;
                }
                if early_match {
                    log::info!("[voice] wake word entendu (rapide) : « {early} »");
                    emit(&events, AgentEvent::State { state: AvatarState::Listening, detail: early.clone() }).await;
                }

                // 2. Phrase entière jusqu'au silence, transcrite par le modèle
                //    précis (`small`) : c'est elle qui décide. Mesuré : `base`
                //    se trompe souvent sur les fenêtres courtes (« je m'y en ai
                //    dit », « je suis à la fois de la nuit » pour « Jimmy… »),
                //    alors que `small` entend le nom sur la phrase complète.
                //    Écouter jusqu'au silence évite aussi de conclure « Jimmy
                //    seul » quand la détection tombe en milieu de phrase.
                let utterance =
                    collect_utterance(&self, &runtime, segment_start, true, rate, hop, end_of_speech, MIN_UTTERANCE, threshold)
                        .await
                        .unwrap_or_default();
                let full_match = matches_wake_word(&utterance, &settings.stt.wake_word);
                log::info!(
                    "[voice] phrase entière en {} ms : « {utterance} » (rapide : « {early} »)",
                    detected_at.elapsed().as_millis()
                );
                if !early_match && !full_match {
                    log::debug!("[voice] parole sans wake word : « {utterance} »");
                    continue;
                }
                if !early_match {
                    // Le modèle rapide l'avait raté : la phrase entière l'a reconnu.
                    log::info!("[voice] wake word entendu (phrase entière) : « {utterance} »");
                    emit(&events, AgentEvent::Heard { text: utterance.clone(), matched: true }).await;
                    emit(&events, AgentEvent::State { state: AvatarState::Listening, detail: utterance.clone() }).await;
                }

                let command = if strip_wake_word(&utterance, &settings.stt.wake_word).trim().is_empty() {
                    // Vraiment « Jimmy » seul, suivi d'une pause : Jimmy répond
                    // « Oui ? », puis écoute. Le micro est purgé après le son
                    // (sa propre voix ne doit pas entrer dans la commande), et
                    // un délai de grâce laisse le temps de commencer à parler.
                    // L'audio est gardé *depuis le début du son* : on commence
                    // souvent à parler pendant le « Oui ? ». Avant, la purge
                    // qui suivait le son effaçait ce début de commande
                    // (« Comment ça va » devenait « Commence à voir »). Le
                    // « oui » éventuellement capté est retiré du texte.
                    let from = runtime.cursor();
                    super::cues::play(&self, super::cues::Cue::Listening).await;
                    // Signal « parle maintenant » : l'interface l'affiche.
                    emit(&events, AgentEvent::State { state: AvatarState::Listening, detail: "Oui ?".into() }).await;
                    log::info!("[voice] « Oui ? » joué, écoute de la commande");
                    let heard = collect_utterance(&self, &runtime, from, false, rate, hop, end_of_speech, AFTER_CUE_GRACE, threshold)
                        .await
                        .unwrap_or_default();
                    strip_cue_echo(&heard)
                } else {
                    utterance
                };
                let command = strip_wake_word(&command, &settings.stt.wake_word);
                if command.trim().is_empty() {
                    log::info!("[voice] commande vide après le nom");
                    emit(&events, AgentEvent::Notice { message: "Je n'ai rien compris après « Jimmy ».".into() }).await;
                    emit(&events, AgentEvent::State { state: AvatarState::Idle, detail: String::new() }).await;
                    // Sinon le même « Jimmy » serait redétecté au tour suivant.
                    runtime.drain();
                    continue;
                }
                log::info!(
                    "[voice] commande : « {command} » ({} ms après la détection)",
                    detected_at.elapsed().as_millis()
                );
                // Affichée dans le chat comme un message de l'utilisateur.
                emit(&events, AgentEvent::Spoken { text: command.clone() }).await;

                // 3. L'agent prend le relais (mode vocal), puis Jimmy répond.
                let session = match &voice_session {
                    Some((id, last)) if last.elapsed() < VOICE_SESSION_IDLE => id.clone(),
                    _ => self
                        .history
                        .create_session("Session vocale")
                        .unwrap_or_else(|_| String::from("vocale")),
                };
                voice_session = Some((session.clone(), Instant::now()));
                let agent_started = Instant::now();
                let answer = crate::core::agent::run(
                    self.deps_voice(),
                    self.settings(),
                    session.clone(),
                    command.clone(),
                    self.tool_context(),
                    events.clone(),
                )
                .await;
                log::info!("[voice] réponse de l'agent en {} ms", agent_started.elapsed().as_millis());

                match answer {
                    Ok(answer) => {
                        self.speak(&answer.text).await;
                        voice_session = Some((session, Instant::now()));
                        emit(&events, AgentEvent::State { state: AvatarState::Idle, detail: String::new() }).await;
                    }
                    Err(error) => {
                        emit(&events, AgentEvent::Failed { message: error.to_string() }).await;
                        self.speak(&format!("Désolé, je n'ai pas pu faire ça : {error}.")).await;
                    }
                }
                // Le tampon contient maintenant la voix de Jimmy lui-même
                // (haut-parleur → micro) : s'il prononce « Jimmy », il se
                // redéclencherait. On repart d'un tampon vide.
                runtime.drain();
            }
            log::info!("[voice] écoute arrêtée");
        })
    }
}

/// Silence au-delà duquel une nouvelle commande ouvre une nouvelle session.
const VOICE_SESSION_IDLE: Duration = Duration::from_secs(600);

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
}

/// Seuil de parole : 3× le bruit de fond, entre 0,0035 et le plafond réglé.
pub fn vad_threshold(noise_floor: f32, ceiling: f32) -> f32 {
    (noise_floor * 3.0).clamp(0.0035, ceiling.max(0.0035))
}

/// Accumule des échantillons jusqu'au silence, puis transcrit la phrase entière.
/// `start` : curseur à partir duquel l'audio fait partie de la phrase.
/// `speech_started` : la parole a déjà commencé (le mot d'éveil est dedans).
async fn collect_utterance(
    app: &Arc<App>,
    runtime: &Arc<VoiceRuntime>,
    start: u64,
    speech_started: bool,
    rate: u32,
    hop: Duration,
    end_of_speech: Duration,
    min_duration: Duration,
    threshold: f32,
) -> Option<String> {
    let mut cursor = start;
    let mut buffer: Vec<f32> = runtime.read_since(&mut cursor);
    let mut heard_speech = speech_started && !buffer.is_empty();
    let started = Instant::now();
    let mut silence = Duration::ZERO;

    while started.elapsed() < MAX_UTTERANCE && runtime.is_running() {
        tokio::time::sleep(hop).await;
        // Lecture par curseur : exactement l'audio nouveau. Piège corrigé :
        // juste après la purge qui suit « Oui ? », une fenêtre fixe de
        // 900 ms était vide et la boucle s'arrêtait aussitôt — Jimmy
        // réagissait à son nom, puis plus rien.
        let fresh = runtime.read_since(&mut cursor);
        if fresh.is_empty() {
            silence += hop;
        } else {
            // Même seuil que celui qui a déclenché l'écoute : la fin de
            // phrase est détectée relativement au bruit de ce micro-ci.
            let talking = crate::providers::stt::is_speech(&fresh, threshold);
            heard_speech |= talking;
            silence = if talking { Duration::ZERO } else { silence + hop };
            buffer.extend_from_slice(&fresh);
        }
        if silence >= end_of_speech && started.elapsed() > min_duration {
            break;
        }
    }
    // Personne n'a parlé (après « Oui ? ») : ne rien transcrire. Whisper
    // invente volontiers du texte sur du silence (« Sous-titres réalisés… »),
    // qui serait exécuté comme une commande.
    if !heard_speech {
        return None;
    }
    transcribe_command(app, &buffer, rate).await
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

/// Transcription de la commande : modèle précis (second serveur) si présent.
async fn transcribe_command(app: &Arc<App>, samples: &[f32], rate: u32) -> Option<String> {
    if samples.len() < rate as usize / 2 {
        return None;
    }
    match app.transcribe_command(window_to_wav(samples, rate)).await {
        Ok(text) => Some(text),
        Err(error) => {
            log::warn!("[voice] transcription de la commande impossible : {error}");
            None
        }
    }
}

/// Transcription du wake word : modèle rapide.
async fn transcribe_samples(app: &Arc<App>, samples: &[f32], rate: u32) -> Option<String> {
    if samples.len() < rate as usize / 2 {
        return None;
    }
    let wav = window_to_wav(samples, rate);
    match app.transcribe_wake(wav).await {
        Ok(text) => Some(text),
        Err(error) => {
            log::warn!("[voice] transcription impossible : {error}");
            None
        }
    }
}

async fn emit(events: &Sender<AgentEvent>, event: AgentEvent) {
    let _ = events.send(event).await;
}