//! Détection de parole par trames de 50 ms.
//!
//! Le détecteur précédent regardait des fenêtres de 900 ms par paliers de
//! 450 ms : la fin d'une phrase n'était connue qu'à 450 ms près, et un
//! claquement de touche suffisait à repousser la fin de la phrase. Mesuré dans
//! le journal : des « parole détectée » sur des niveaux de 0,004 à 0,005, et
//! des phrases qui duraient jusqu'à la limite des 12 s.
//!
//! Ici, chaque trame de 50 ms est classée « voisée » ou non (énergie RMS au-dessus
//! du seuil adaptatif). Deux règles écartent les bruits brefs :
//!
//! * **confirmation** : une trame ne compte comme parole que si la suivante
//!   l'est aussi (≥ 100 ms d'énergie continue) — un claquement dure moins ;
//! * **fin de phrase** : le silence ne s'accumule que sur des trames non
//!   voisées, et se remet à zéro dès qu'une parole *confirmée* revient.
//!
//! Et une **hystérésis** contre les voix qui baissent : une fois de la parole
//! confirmée entendue dans la prise, une trame entre le seuil de maintien
//! (60 % du seuil d'attaque) et le seuil d'attaque ne compte ni comme parole
//! ni comme silence — elle gèle l'état. Une intonation qui retombe ou un
//! « euh » doux ne closent plus la phrase ; seul un vrai silence l'achève.
//! Sans parole confirmée au préalable, la même trame reste du silence (un
//! bruit de fond élevé ne démarre jamais une prise).
//!
//! Le temps est celui de l'audio (nombre d'échantillons), pas l'horloge : si la
//! transcription rapide prend 2 s pendant que l'utilisateur parle, l'audio
//! accumulé est analysé d'un coup, sans retard sur la détection de fin.

/// Durée d'une trame d'analyse.
pub const FRAME_MS: usize = 50;

/// Parole antérieure à l'armement : en dessous, c'est l'écho du « Oui ? ».
pub const PRE_ARMED_MIN_MS: u64 = 700;

/// Délai (depuis le premier son) avant de juger si c'est de la parole ou du bruit.
pub const NOISE_CHECK_MS: u64 = 3000;

/// Part minimale de parole confirmée, en millièmes du temps écoulé, pour que
/// la prise soit de la parole (150 = 15 %).
pub const MIN_DENSITY_PERMILLE: u64 = 150;

/// Parole au-delà de laquelle le silence de fin s'allonge : une longue
/// phrase contient des hésitations (on cherche ses mots), une commande brève
/// non.
pub const LONG_SPEECH_FROM_MS: u64 = 1500;

/// En dessous, la prise est une commande ou une question brève (« Oui »,
/// « Confirmé », « Salut Jimy comment tu vas ? », « Quelle heure est-il ? ») :
/// le silence de fin est plafonné à [`SHORT_SPEECH_END_MS`] au lieu d'attendre
/// le hangover complet d'une longue phrase (fast commit, 17 octobre).
pub const SHORT_SPEECH_UNTIL_MS: u64 = 1200;

/// Silence de fin maximal d'une phrase brève : réactif sans couper une
/// respiration normale entre deux mots.
pub const SHORT_SPEECH_END_MS: u64 = 650;

/// Allongement maximal du silence de fin, atteint à 2,5 s de parole
/// (700 ms réglés → 1,2 s). Cas réel : « Je viens de voir que dans ta mémoire
/// tu as mis... » coupé par une pause de réflexion.
pub const MAX_EXTRA_END_MS: u64 = 500;

/// Part du seuil d'attaque qui maintient une phrase commencée : une trame
/// au-dessus de `seuil × KEEP_RATIO` (parole confirmée déjà entendue) gèle
/// le compteur de silence au lieu de l'incrémenter. 60 % : avec le seuil
/// minimal (0,0035), le maintien est à 0,0021 — au-dessus du bruit de fond
/// typique (0,002), qui reste du vrai silence.
pub const KEEP_RATIO: f32 = 0.6;

/// Énergie RMS d'un bloc d'échantillons.
pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

/// Taille d'une trame, en échantillons.
pub fn frame_len(rate: u32) -> usize {
    (rate as usize * FRAME_MS / 1000).max(1)
}

/// Part des trames de `samples` dont l'énergie dépasse `threshold` (0 à 1).
/// Un claquement bref ne fait voiser qu'une trame sur douze ; une parole
/// continue en fait voiser la majorité.
pub fn voiced_fraction(samples: &[f32], frame: usize, threshold: f32) -> f32 {
    let frames: Vec<&[f32]> = samples.chunks_exact(frame).collect();
    if frames.is_empty() {
        return 0.0;
    }
    let voiced = frames.iter().filter(|f| rms(f) > threshold).count();
    voiced as f32 / frames.len() as f32
}

/// Suit une prise de parole au fil de l'audio qui arrive.
pub struct SpeechTracker {
    frame: usize,
    threshold: f32,
    /// Seuil de maintien (hystérésis) : `threshold × KEEP_RATIO`.
    keep: f32,
    /// Trames de silence qui closent la phrase.
    end_frames: usize,
    /// Les trames *voisées* avant cet échantillon ne déclenchent pas « la
    /// parole a commencé » : c'est l'écho possible du « Oui ? » de Jimmy dans
    /// les haut-parleurs. Elles restent dans l'extrait si l'utilisateur
    /// parle ensuite (il a pu commencer pendant le « Oui ? »).
    armed_at: usize,
    processed: usize,
    run: u32,
    silence: usize,
    voiced_frames: u64,
    /// Trames voisées confirmées *avant* le point d'armement.
    pre_frames: u64,
    first_any: Option<usize>,
    first_armed: Option<usize>,
    last_end: usize,
}

impl SpeechTracker {
    pub fn new(rate: u32, threshold: f32, end_of_speech_ms: u64, armed_at: usize) -> Self {
        let frame = frame_len(rate);
        SpeechTracker {
            frame,
            threshold,
            keep: threshold * KEEP_RATIO,
            end_frames: (end_of_speech_ms as usize / FRAME_MS).max(1),
            armed_at,
            processed: 0,
            run: 0,
            silence: 0,
            voiced_frames: 0,
            pre_frames: 0,
            first_any: None,
            first_armed: None,
            last_end: 0,
        }
    }

    /// Analyse les trames complètes de `buffer` pas encore vues. `buffer` est
    /// l'audio depuis le début de la prise, rallongé à chaque appel.
    pub fn feed(&mut self, buffer: &[f32]) {
        while self.processed + self.frame <= buffer.len() {
            let start = self.processed;
            let end = start + self.frame;
            self.processed = end;
            let level = rms(&buffer[start..end]);
            if level > self.threshold {
                self.run += 1;
                if self.run >= 2 {
                    // Deuxième trame voisée d'affilée : la précédente est
                    // confirmée avec elle.
                    let first_confirmed = if self.run == 2 { start - self.frame } else { start };
                    let newly = if self.run == 2 { 2 } else { 1 };
                    self.first_any.get_or_insert(first_confirmed);
                    self.last_end = end;
                    // Toute parole confirmée remet le silence à zéro : un
                    // extrait qui commence avant l'armement doit pouvoir se
                    // terminer comme les autres.
                    self.silence = 0;
                    if end > self.armed_at {
                        self.voiced_frames += newly;
                        self.first_armed.get_or_insert(first_confirmed.max(self.armed_at.saturating_sub(self.frame)));
                    } else {
                        self.pre_frames += newly;
                    }
                }
            } else if self.first_any.is_some() && level > self.keep {
                // Hystérésis : parole confirmée déjà entendue, et niveau
                // entre maintien et attaque (intonation qui retombe, « euh »
                // doux). Trame gelée : ni parole comptée, ni silence accumulé.
            } else {
                self.run = 0;
                self.silence += 1;
            }
        }
    }

    /// La parole a-t-elle commencé ?
    ///
    /// Oui dès qu'une parole confirmée apparaît après le point d'armement. Une
    /// parole *antérieure* ne compte que si elle est assez longue pour ne pas
    /// être l'écho du « Oui ? » (~0,5 s) : l'utilisateur qui enchaîne sa
    /// commande pendant la pause avant le cue (« Jimmy… dis-moi bonjour »)
    /// n'est ainsi plus perdu.
    pub fn started(&self) -> bool {
        self.first_armed.is_some() || self.pre_frames * FRAME_MS as u64 >= PRE_ARMED_MIN_MS
    }

    /// La phrase est-elle terminée (parole puis silence suffisant) ?
    ///
    /// Trois régimes selon la parole déjà dite :
    /// * brève (`≤ SHORT_SPEECH_UNTIL_MS`, « Oui », « Quelle heure est-il ? ») :
    ///   fast commit à `SHORT_SPEECH_END_MS` (650 ms max) pour ne pas faire
    ///   attendre le hangover complet ;
    /// * normale (`1,2 à 1,5 s`) : le réglage (`end_of_speech_ms`, 1 200 ms par défaut) ;
    /// * longue (`> 1,5 s`) : allongée jusqu'à `+500 ms` pour tolérer une pause
    ///   de réflexion.
    pub fn ended(&self) -> bool {
        self.started() && self.silence >= self.required_end_frames()
    }

    fn required_end_frames(&self) -> usize {
        let voiced = self.voiced_ms();
        let active_samples = self.first_any.map(|f| self.last_end.saturating_sub(f)).unwrap_or(0);
        let active_ms = (active_samples / self.frame.max(1)) as u64 * FRAME_MS as u64;
        // Une vraie phrase brève (« Oui », « Confirmé », « Salut Jimy ça va ? »)
        // a au moins 300 ms de parole continue et une densité ≥ 60 % : un
        // souffle éparse (100 ms par seconde) ne déclenche jamais le fast commit.
        let dense_short = (300..=SHORT_SPEECH_UNTIL_MS).contains(&voiced)
            && active_ms > 0
            && voiced * 100 >= active_ms * 60;
        if dense_short {
            let cap = (SHORT_SPEECH_END_MS as usize / FRAME_MS).max(1);
            self.end_frames.min(cap)
        } else {
            self.end_frames + self.extra_end_frames()
        }
    }

    /// Trames de silence ajoutées pour une longue phrase.
    fn extra_end_frames(&self) -> usize {
        let extra_ms = (self.voiced_ms().saturating_sub(LONG_SPEECH_FROM_MS) / 2).min(MAX_EXTRA_END_MS);
        extra_ms as usize / FRAME_MS
    }

    /// Silence en cours depuis la dernière trame voisée, en ms. Sert à couper
    /// une prise trop longue dans une pause plutôt qu'au milieu d'un mot.
    pub fn silent_ms(&self) -> u64 {
        (self.silence * FRAME_MS) as u64
    }

    /// Durée de parole confirmée, en ms.
    pub fn voiced_ms(&self) -> u64 {
        (self.voiced_frames + if self.started() { self.pre_frames } else { 0 }) * FRAME_MS as u64
    }

    /// Vrai si la « parole » est en réalité du bruit éparse.
    ///
    /// Un bruit ambiant proche du seuil ne se tait jamais assez longtemps pour
    /// clore la prise : elle durait jusqu'à la limite. Mesuré dans le journal :
    /// 19,9 s pour 250 ms de parole confirmée (1 %), Jimmy aveugle pendant ce
    /// temps. La parole est dense (la plupart des trames voisées entre deux
    /// pauses) ; le bruit est clairsemé.
    pub fn looks_like_noise(&self, buffer_len: usize, rate: u32) -> bool {
        let Some(first) = self.speech_start() else {
            return false;
        };
        let elapsed_ms = buffer_len.saturating_sub(first) as u64 * 1000 / rate.max(1) as u64;
        elapsed_ms >= NOISE_CHECK_MS && self.voiced_ms() * 1000 < elapsed_ms * MIN_DENSITY_PERMILLE
    }

    /// Échantillon où la parole a commencé.
    pub fn speech_start(&self) -> Option<usize> {
        if self.first_armed.is_some() {
            self.first_armed
        } else if self.started() {
            self.first_any
        } else {
            None
        }
    }

    /// Extrait rogné : de `pad` avant la première parole à `pad` après la
    /// dernière. Whisper invente du texte sur le silence qui entoure la
    /// phrase ; ne lui donner que la parole (avec une marge) l'en empêche.
    pub fn clip<'a>(&self, buffer: &'a [f32], pad: usize) -> &'a [f32] {
        let Some(first) = self.first_any else {
            return &[];
        };
        let from = first.saturating_sub(pad).min(buffer.len());
        let to = (self.last_end + pad).min(buffer.len()).max(from);
        &buffer[from..to]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 16_000;

    /// `ms` de signal d'amplitude constante `level` (RMS = level).
    fn tone(ms: usize, level: f32) -> Vec<f32> {
        vec![level; RATE as usize * ms / 1000]
    }

    fn concat(parts: &[Vec<f32>]) -> Vec<f32> {
        parts.iter().flatten().copied().collect()
    }

    #[test]
    fn la_phrase_se_termine_apres_le_silence() {
        // 300 ms de bruit, 800 ms de parole, 800 ms de silence.
        let audio = concat(&[tone(300, 0.001), tone(800, 0.02), tone(800, 0.001)]);
        let mut tracker = SpeechTracker::new(RATE, 0.0035, 700, 0);
        tracker.feed(&audio);
        assert!(tracker.started());
        assert!(tracker.ended());
        // 16 trames de parole (800 ms) : mesuré à une trame près.
        assert!((750..=850).contains(&tracker.voiced_ms()), "{}", tracker.voiced_ms());
    }

    #[test]
    fn pas_de_fin_tant_que_le_silence_est_trop_court() {
        let audio = concat(&[tone(800, 0.02), tone(400, 0.001)]);
        let mut tracker = SpeechTracker::new(RATE, 0.0035, 700, 0);
        tracker.feed(&audio);
        assert!(tracker.started());
        assert!(!tracker.ended(), "400 ms de silence ne closent pas une phrase de 700 ms");
    }

    #[test]
    fn une_hesitation_dans_une_longue_phrase_ne_la_coupe_pas() {
        // Cas réel (journal du 4 octobre) : « Je viens de voir que dans ta
        // mémoire tu as mis... » coupé par une hésitation. 2,5 s de parole
        // puis 900 ms de pause : la phrase n'est pas finie.
        let audio = concat(&[tone(2500, 0.02), tone(900, 0.001)]);
        let mut tracker = SpeechTracker::new(RATE, 0.0035, 700, 0);
        tracker.feed(&audio);
        assert!(!tracker.ended(), "une pause de 900 ms après 2,5 s de parole est une hésitation");
        // Un vrai silence finit quand même par la clore.
        tracker.feed(&concat(&[audio.clone(), tone(400, 0.001)]));
        assert!(tracker.ended(), "1,3 s de silence closent la phrase");
    }

    #[test]
    fn une_commande_breve_garde_un_silence_de_fin_court() {
        // « Non, c'est bon » (600 ms) : la réactivité ne doit pas changer.
        let audio = concat(&[tone(600, 0.02), tone(750, 0.001)]);
        let mut tracker = SpeechTracker::new(RATE, 0.0035, 700, 0);
        tracker.feed(&audio);
        assert!(tracker.ended());
    }

    #[test]
    fn une_baisse_de_volume_ne_coupe_pas_la_phrase() {
        // Cas réel (17 octobre) : la voix baisse en fin de phrase (intonation
        // qui retombe, « euh » doux) sans se taire — niveau sous le seuil
        // d'attaque mais au-dessus du seuil de maintien, 1,2 s. Ça ne doit
        // pas clore la prise ; seul le vrai silence final la clôt.
        let debut = concat(&[tone(1000, 0.02), tone(1200, 0.0028)]);
        let mut tracker = SpeechTracker::new(RATE, 0.0035, 1200, 0);
        tracker.feed(&debut);
        assert!(tracker.started());
        assert!(
            !tracker.ended(),
            "1,2 s de voix basse n'est pas un silence de fin"
        );
        // Reprise puis vrai silence : la phrase se clôt normalement (2 s,
        // car l'extension « longue phrase » porte le hangover à 1,7 s ici).
        tracker.feed(&concat(&[debut, tone(1500, 0.02), tone(2000, 0.001)]));
        assert!(tracker.ended(), "le vrai silence final doit clore la phrase");
    }

    #[test]
    fn un_vrai_silence_clot_meme_avec_un_hangover_long() {
        // Garde-fou : l'hystérésis ne doit pas empêcher une vraie fin (phrase
        // de 1,4 s, régime normal à 1 200 ms).
        let audio = concat(&[tone(1400, 0.02), tone(1300, 0.001)]);
        let mut tracker = SpeechTracker::new(RATE, 0.0035, 1200, 0);
        tracker.feed(&audio);
        assert!(tracker.ended(), "1,3 s de vrai silence closent la phrase");
    }

    #[test]
    fn une_question_courte_se_clot_en_fast_commit() {
        // « Salut Jimy, ça va ? » ou « Confirmé » (~800 ms de parole) : même
        // avec le réglage Naturel (1 200 ms) ou Patient (1 600 ms), la prise
        // se clôt après ~650 ms de silence sans faire attendre 1,2 à 1,6 s.
        let audio = concat(&[tone(800, 0.02), tone(700, 0.001)]);
        let mut tracker = SpeechTracker::new(RATE, 0.0035, 1600, 0);
        tracker.feed(&audio);
        assert!(tracker.ended(), "fast commit attendu après 700 ms de silence sur une phrase brève");
        // En revanche, une phrase de 1,5 s garde le hangover complet.
        let moyenne = concat(&[tone(1500, 0.02), tone(900, 0.001)]);
        let mut tracker2 = SpeechTracker::new(RATE, 0.0035, 1200, 0);
        tracker2.feed(&moyenne);
        assert!(!tracker2.ended(), "une phrase de 1,5 s ne doit pas être coupée à 900 ms");
    }

    #[test]
    fn le_silence_courant_est_mesure() {
        let audio = concat(&[tone(1000, 0.02), tone(200, 0.001)]);
        let mut tracker = SpeechTracker::new(RATE, 0.0035, 700, 0);
        tracker.feed(&audio);
        assert!((150..=250).contains(&tracker.silent_ms()), "{}", tracker.silent_ms());
    }

    #[test]
    fn un_claquement_bref_n_est_pas_de_la_parole() {
        // 30 ms de pic (moins qu'une trame de 50 ms), noyé dans le silence.
        let audio = concat(&[tone(500, 0.001), tone(30, 0.08), tone(1500, 0.001)]);
        let mut tracker = SpeechTracker::new(RATE, 0.0035, 700, 0);
        tracker.feed(&audio);
        assert!(!tracker.started(), "un claquement ne doit pas démarrer une prise");
    }

    #[test]
    fn un_claquement_ne_repousse_pas_la_fin() {
        // Parole, silence, claquement, silence : la phrase doit se clore malgré le clic.
        let audio = concat(&[tone(600, 0.02), tone(400, 0.001), tone(40, 0.08), tone(500, 0.001)]);
        let mut tracker = SpeechTracker::new(RATE, 0.0035, 700, 0);
        tracker.feed(&audio);
        assert!(tracker.ended(), "le clic isolé ne doit pas remettre le silence à zéro");
    }

    #[test]
    fn l_echo_avant_l_armement_ne_demarre_pas_la_prise() {
        // 600 ms d'écho du « Oui ? » (armement à 700 ms), puis du silence.
        let armed = RATE as usize * 700 / 1000;
        let audio = concat(&[tone(600, 0.02), tone(2000, 0.001)]);
        let mut tracker = SpeechTracker::new(RATE, 0.0035, 700, armed);
        tracker.feed(&audio);
        assert!(!tracker.started(), "l'écho du cue ne doit pas compter comme parole");
    }

    #[test]
    fn la_commande_deja_dite_avant_la_fin_du_cue_est_prise() {
        // L'utilisateur enchaîne « dis-moi bonjour » (1,2 s) pendant la pause
        // avant le cue, qui finit à 1,6 s : toute la parole est antérieure.
        let armed = RATE as usize * 1600 / 1000;
        let audio = concat(&[tone(200, 0.001), tone(1200, 0.02), tone(2200, 0.001)]);
        let mut tracker = SpeechTracker::new(RATE, 0.0035, 700, armed);
        tracker.feed(&audio);
        assert!(tracker.started(), "1,2 s de parole avant le cue = l'utilisateur, pas un écho");
        assert!(tracker.ended());
        assert!(tracker.voiced_ms() >= 1100, "{}", tracker.voiced_ms());
    }

    #[test]
    fn la_parole_commencee_pendant_le_cue_est_gardee() {
        // L'utilisateur parle de 400 à 1500 ms, le cue finit à 700 ms.
        let armed = RATE as usize * 700 / 1000;
        let audio = concat(&[tone(400, 0.001), tone(1100, 0.02), tone(900, 0.001)]);
        let mut tracker = SpeechTracker::new(RATE, 0.0035, 700, armed);
        tracker.feed(&audio);
        assert!(tracker.started());
        assert!(tracker.ended());
        let clip = tracker.clip(&audio, 0);
        // L'extrait commence avant l'armement : le début de la phrase est là.
        let clip_start = clip.as_ptr() as usize - audio.as_ptr() as usize;
        assert!(clip_start / 4 < armed, "début de phrase perdu");
    }

    #[test]
    fn l_extrait_est_rogne_autour_de_la_parole() {
        let audio = concat(&[tone(2000, 0.001), tone(600, 0.02), tone(2000, 0.001)]);
        let mut tracker = SpeechTracker::new(RATE, 0.0035, 700, 0);
        tracker.feed(&audio);
        let pad = RATE as usize * 300 / 1000;
        let clip = tracker.clip(&audio, pad);
        let ms = clip.len() * 1000 / RATE as usize;
        // 600 ms de parole + 2 × 300 ms de marge, pas les 4,6 s d'origine.
        assert!((1100..=1350).contains(&ms), "{ms} ms");
    }

    #[test]
    fn un_bruit_eparse_est_reconnu_comme_bruit() {
        // Un souffle de 100 ms par seconde pendant 6 s : 10 % de densité, et
        // jamais assez de silence pour clore la prise.
        let mut parts = vec![tone(300, 0.001)];
        for _ in 0..6 {
            parts.push(tone(100, 0.02));
            parts.push(tone(900, 0.001));
        }
        let audio = concat(&parts);
        let mut tracker = SpeechTracker::new(RATE, 0.0035, 1500, 0);
        tracker.feed(&audio);
        assert!(!tracker.ended(), "le bruit éparse ne clôt pas la prise tout seul");
        assert!(tracker.looks_like_noise(audio.len(), RATE));
    }

    #[test]
    fn la_parole_continue_n_est_pas_du_bruit() {
        let audio = concat(&[tone(200, 0.001), tone(3500, 0.02)]);
        let mut tracker = SpeechTracker::new(RATE, 0.0035, 700, 0);
        tracker.feed(&audio);
        assert!(!tracker.looks_like_noise(audio.len(), RATE));
    }

    #[test]
    fn une_phrase_avec_une_pause_n_est_pas_du_bruit() {
        // 1 s de parole, 1,2 s de pause, 1,5 s de parole : 55 % de densité.
        let audio = concat(&[tone(200, 0.001), tone(1000, 0.02), tone(1200, 0.001), tone(1500, 0.02)]);
        let mut tracker = SpeechTracker::new(RATE, 0.0035, 2000, 0);
        tracker.feed(&audio);
        assert!(!tracker.looks_like_noise(audio.len(), RATE));
    }

    #[test]
    fn fraction_voisee() {
        let frame = frame_len(RATE);
        let parole = tone(600, 0.02);
        assert!(voiced_fraction(&parole, frame, 0.0035) > 0.95);
        let clic = concat(&[tone(550, 0.001), tone(50, 0.08)]);
        assert!(voiced_fraction(&clic, frame, 0.0035) < 0.15);
    }
}
