//! Langue de l'application côté Rust (version anglaise, 22 octobre).
//!
//! La langue effective vient de `settings.ui.language` :
//! - `"en"` → anglais ([`Lang::En`]) ;
//! - `"fr"` → français ([`Lang::Fr`]) ;
//! - `"auto"` (ou toute autre valeur) → langue de Windows lue une fois au
//!   démarrage dans `HKCU\Control Panel\International` (`LocaleName`), avec le
//!   français par défaut si la lecture échoue (aucun changement de comportement
//!   sur les installations existantes).

use std::sync::OnceLock;

/// Langue effective de l'interface et des réponses de Jimy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Fr,
    En,
}

impl Lang {
    pub fn is_en(self) -> bool {
        matches!(self, Lang::En)
    }

    pub fn code(self) -> &'static str {
        match self {
            Lang::Fr => "fr",
            Lang::En => "en",
        }
    }
}

/// Détermine la langue effective à partir du réglage (`"auto"`, `"fr"`, `"en"`)
/// et de la locale système. Pur, pour les tests.
pub fn resolve_with(setting: &str, system_locale: &str) -> Lang {
    let s = setting.trim().to_ascii_lowercase();
    if s.starts_with("en") {
        return Lang::En;
    }
    if s.starts_with("fr") {
        return Lang::Fr;
    }
    if system_locale.trim().to_ascii_lowercase().starts_with("en") {
        Lang::En
    } else {
        Lang::Fr
    }
}

/// Langue effective selon `settings.ui.language`.
pub fn resolve(setting: &str) -> Lang {
    let s = setting.trim().to_ascii_lowercase();
    if s.starts_with("en") {
        return Lang::En;
    }
    if s.starts_with("fr") {
        return Lang::Fr;
    }
    system_lang()
}

fn system_lang() -> Lang {
    static DETECTED: OnceLock<Lang> = OnceLock::new();
    *DETECTED.get_or_init(|| {
        let locale = read_system_locale().unwrap_or_default();
        resolve_with("auto", &locale)
    })
}

#[cfg(windows)]
fn read_system_locale() -> Option<String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let out = std::process::Command::new("reg")
        .args(["query", r"HKCU\Control Panel\International", "/v", "LocaleName"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        if line.contains("LocaleName") {
            return line.split_whitespace().last().map(str::to_string);
        }
    }
    None
}

#[cfg(not(windows))]
fn read_system_locale() -> Option<String> {
    std::env::var("LC_ALL")
        .or_else(|_| std::env::var("LANG"))
        .ok()
}

/// Consigne ajoutée au prompt système de l'agent quand l'interface est en
/// anglais. Les instructions système restent en français (testées et calibrées),
/// seule la langue de sortie change.
pub fn system_output_directive(lang: Lang) -> Option<&'static str> {
    match lang {
        Lang::Fr => None,
        Lang::En => Some(
            "## Output language\n\
             The user's interface is in English: reply, announce your steps, and write all user-facing \
             explanations in English (unless the user explicitly speaks another language in their message).",
        ),
    }
}

/// Ajoute aux prompts d'extraction en tâche de fond (carnet, mémoire, tri du
/// vault, capture de skill, revue) la consigne de rédiger le texte libre en
/// anglais tout en gardant les clés JSON et marqueurs attendus par le parseur.
pub fn with_background_lang(prompt: &str, lang: Lang) -> String {
    match lang {
        Lang::Fr => prompt.to_string(),
        Lang::En => format!(
            "{prompt}\n\n\
             IMPORTANT — Output language: write all human-readable text values (titles, summaries, next steps, \
             memories, lessons, notes, skill descriptions and bodies, review proposals) in English. \
             Keep all JSON field names (`titre`, `resume`, `chantiers`, `statut`, `prochaine`, `kind`, `content`, \
             `dest`, `dossier`), status enums (`ouvert`, `attente`, `fini`), and format markers (`proche:`, \
             `name:`, `description:`, `body:`, `aucun`, `RIEN`) exactly as specified above."
        ),
    }
}

// ── Phrases dites ou affichées par Jimy côté Rust ────────────────────────────

pub fn stopped_reply(lang: Lang) -> &'static str {
    match lang {
        Lang::Fr => "D'accord, j'arrête. Je t'écoute.",
        Lang::En => "Alright, stopping. I'm listening.",
    }
}

pub fn confirmed_reply(lang: Lang) -> &'static str {
    match lang {
        Lang::Fr => "C'est autorisé, je continue.",
        Lang::En => "Approved, continuing.",
    }
}

pub fn detached_reply(lang: Lang) -> &'static str {
    match lang {
        Lang::Fr => "Je m'en occupe en arrière-plan, je te préviens quand c'est fini.",
        Lang::En => "I'm handling this in the background, I'll let you know when it's done.",
    }
}

pub fn still_working(lang: Lang) -> &'static str {
    match lang {
        Lang::Fr => "Je travaille toujours dessus.",
        Lang::En => "I'm still working on it.",
    }
}

pub fn listening_ack(lang: Lang) -> &'static str {
    match lang {
        Lang::Fr => "D'accord, je t'écoute.",
        Lang::En => "Okay, I'm listening.",
    }
}

pub fn nothing_after_wake(lang: Lang, wake: &str) -> String {
    match lang {
        Lang::Fr => format!("Je n'ai rien entendu après « {wake} »."),
        Lang::En => format!("I didn't hear anything after “{wake}”."),
    }
}

pub fn window_attached_notice(lang: Lang, label: &str) -> String {
    match lang {
        Lang::Fr => format!("Fenêtre « {label} » jointe à la demande."),
        Lang::En => format!("Window “{label}” attached to the request."),
    }
}

pub fn chat_stopped_reply(lang: Lang) -> &'static str {
    match lang {
        Lang::Fr => "Arrêté à ta demande. Dis-moi quoi faire.",
        Lang::En => "Stopped at your request. Tell me what to do.",
    }
}

pub fn update_stopped_reply(lang: Lang) -> &'static str {
    match lang {
        Lang::Fr => "Arrêté à ta demande. Le Jimy actuel tourne toujours.",
        Lang::En => "Stopped at your request. The current Jimy is still running.",
    }
}

pub fn bg_done_speech(lang: Lang, text: &str) -> String {
    match lang {
        Lang::Fr => format!("Tâche de fond terminée. {text}"),
        Lang::En => format!("Background task finished. {text}"),
    }
}

pub fn bg_failed_speech(lang: Lang) -> &'static str {
    match lang {
        Lang::Fr => "La tâche de fond a échoué. Le détail est dans le Chat.",
        Lang::En => "The background task failed. Details are in the Chat.",
    }
}

pub fn new_session_reply(lang: Lang) -> &'static str {
    match lang {
        Lang::Fr => "Nouvelle conversation ouverte. Dis-moi de quoi on parle.",
        Lang::En => "New conversation started. Tell me what's next.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_langue_suit_le_reglage_puis_le_systeme() {
        assert_eq!(resolve_with("en", "fr-FR"), Lang::En);
        assert_eq!(resolve_with("fr", "en-US"), Lang::Fr);
        assert_eq!(resolve_with("auto", "en-US"), Lang::En);
        assert_eq!(resolve_with("auto", "en_GB.UTF-8"), Lang::En);
        assert_eq!(resolve_with("auto", "fr-FR"), Lang::Fr);
        assert_eq!(resolve_with("", ""), Lang::Fr);
    }

    #[test]
    fn la_consigne_anglaise_preserve_les_marqueurs_de_format() {
        assert!(system_output_directive(Lang::Fr).is_none());
        assert!(system_output_directive(Lang::En).unwrap().contains("English"));
        let bg = with_background_lang(" Test", Lang::En);
        assert!(bg.contains("English"));
        assert!(bg.contains("proche:"));
        assert!(bg.contains("RIEN"));
    }
}
