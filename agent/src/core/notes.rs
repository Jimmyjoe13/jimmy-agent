//! Carnet de travail d'une conversation : les **acquis vérifiés** d'un tour
//! outillé (chemins, ports, routes, accès, commandes qui marchent, ce qui
//! reste), relus par les tours suivants de la même conversation.
//!
//! Pourquoi (cas réel du 14 octobre, tâche « dashboard » Andy) : l'historique
//! rechargé ne garde que le texte des échanges, jamais les résultats d'outils
//! (piège 45) ; le carnet de bord (`core::recap`) ne voit que ce texte, et le
//! chantier ne garde qu'une phrase. Chaque « ok go » repartait donc de zéro :
//! 3 tours, ~120 appels, 70 min à réinspecter le même VPS (`docker ps`,
//! `docker-compose.yml`, routes de l'API) sans écrire un fichier.
//!
//! Après un tour d'au moins [`NOTES_MIN_TOOLS`] outils, un appel au modèle
//! (en tâche de fond, après la réponse) relit le tour **avec ses résultats
//! d'outils** et réécrit le carnet ; le prompt des tours suivants l'injecte
//! (« pars de là, ne revérifie pas »).

use chrono::Utc;
use rusqlite::{params, OptionalExtension};

use crate::config::Settings;
use crate::core::history::History;
use crate::core::types::{Message, Role};
use crate::error::Result;
use crate::providers::llm::LlmClient;

/// Un tour qui a appelé au moins autant d'outils réécrit le carnet. En
/// dessous (question simple, réponse en parallèle d'une tâche de fond), rien
/// n'a été exploré qui mérite d'être retenu.
pub const NOTES_MIN_TOOLS: usize = 6;

/// Taille maximale du carnet (stocké et injecté).
pub const NOTES_MAX_CHARS: usize = 1_800;

/// Taille maximale du tour relu par l'appel de mise à jour : au-delà, on garde
/// la **fin** du tour (les acquis les plus récents) — mesuré le 14 octobre :
/// ~75 000 caractères de contexte par étape après compaction.
const TRANSCRIPT_MAX_CHARS: usize = 40_000;

/// Part d'un résultat d'outil gardée dans la relecture.
const RESULT_CHARS: usize = 1_500;

/// Réponse du modèle quand le tour n'a rien appris d'utile.
const NOTHING: &str = "RIEN";

const NOTE_TAKER: &str = r#"Tu tiens le carnet de travail de Jimy, l'assistant personnel de l'utilisateur. On te donne le carnet actuel de la conversation (peut être vide) et le dernier tour de travail de Jimy : la demande, ses appels d'outils et leurs résultats.

Réécris le carnet : des puces courtes et factuelles, ce qu'il faut savoir pour **reprendre ce travail sans tout réinspecter**.
- Faits vérifiés par les résultats d'outils : chemins et fichiers clés (lus ou écrits), serveurs, ports, URLs, routes d'API, conteneurs, accès qui marchent (nom de la clé ou du serveur MCP, jamais sa valeur), commandes qui ont marché, ce qui a échoué et pourquoi.
- Décisions prises avec l'utilisateur, et ce qui reste à faire (étapes concrètes, dans l'ordre).
- Garde les acquis du carnet actuel encore valables ; retire ceux que le tour a contredits.
- Jamais de secret (clé, mot de passe, jeton) : seulement son nom ou son emplacement.
- 1 500 caractères au plus. Pas de titre, pas de phrase d'introduction.
Si le tour n'apprend rien d'utile pour la suite, réponds exactement : RIEN"#;

impl History {
    /// Carnet de travail de la conversation (`None` : jamais écrit).
    pub fn work_notes(&self, session_id: &str) -> Result<Option<String>> {
        let conn = self.db.lock().unwrap();
        let notes = conn
            .conn()
            .query_row("SELECT notes FROM work_notes WHERE session_id = ?1", params![session_id], |r| r.get(0))
            .optional()?;
        Ok(notes)
    }

    /// Remplace le carnet de travail de la conversation.
    pub fn save_work_notes(&self, session_id: &str, notes: &str) -> Result<()> {
        self.db.lock().unwrap().conn().execute(
            "INSERT INTO work_notes (session_id, notes, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(session_id) DO UPDATE SET notes = ?2, updated_at = ?3",
            params![session_id, notes, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }
}

/// Bloc « Carnet de travail » du prompt (`None` : carnet vide).
pub fn work_notes_block(notes: Option<&str>) -> Option<String> {
    let notes = notes.map(str::trim).filter(|n| !n.is_empty())?;
    Some(format!(
        "## Carnet de travail de cette conversation\nAcquis vérifiés aux tours précédents (résultats d'outils) : \
         pars de là, ne les revérifie pas — sauf si une action qui s'appuie dessus échoue.\n{}",
        clip(notes, NOTES_MAX_CHARS)
    ))
}

/// Le tour à relire : la demande, les appels d'outils (nom et arguments) et
/// leurs résultats (bornés), puis la réponse. Le prompt système et
/// l'historique d'avant le tour n'y sont pas. Trop long : on garde la fin.
fn turn_transcript(turn: &[Message]) -> String {
    let mut lines = Vec::new();
    for message in turn {
        match message.role {
            Role::System => {}
            Role::User => lines.push(format!("Demande : {}", clip(&message.content, RESULT_CHARS))),
            Role::Assistant => {
                if !message.content.trim().is_empty() {
                    lines.push(format!("Jimy : {}", clip(&message.content, RESULT_CHARS)));
                }
                for call in message.tool_calls.iter().flatten() {
                    lines.push(format!("Appel {} {}", call.name, clip(&call.arguments.to_string(), 400)));
                }
            }
            Role::Tool => {
                let name = message.name.as_deref().unwrap_or("outil");
                lines.push(format!("Résultat {name} : {}", clip(&message.content, RESULT_CHARS)));
            }
        }
    }
    let text = lines.join("\n");
    let count = text.chars().count();
    if count <= TRANSCRIPT_MAX_CHARS {
        return text;
    }
    let tail: String = text.chars().skip(count - TRANSCRIPT_MAX_CHARS).collect();
    format!("[début du tour coupé]\n{tail}")
}

/// Réponse du modèle → carnet à enregistrer (`None` : rien d'utile, on garde
/// l'ancien). Retire une éventuelle clôture de code, borne la taille.
fn clean_notes(raw: &str) -> Option<String> {
    let text = raw.trim().trim_start_matches("```markdown").trim_start_matches("```").trim_end_matches("```").trim();
    if text.is_empty() || text.eq_ignore_ascii_case(NOTHING) || text.trim_end_matches('.').eq_ignore_ascii_case(NOTHING) {
        return None;
    }
    Some(clip(text, NOTES_MAX_CHARS))
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    format!("{}…", text.chars().take(max - 1).collect::<String>())
}

/// Un seul carnet de travail mis à jour à la fois (deux tours qui finissent
/// ensemble ne doivent pas s'écraser).
static RUNNING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Réécrit le carnet de travail à partir du tour qui vient de finir (`turn` :
/// de la demande à la réponse, résultats d'outils compris). Appelé en tâche de
/// fond après la réponse, avec le modèle de la tâche (celui qui sait lire ces
/// résultats). Les clés connues sont masquées avant l'écriture.
pub async fn update(llm: &LlmClient, settings: &Settings, history: &History, session_id: &str, model: &str, turn: &[Message]) {
    let _guard = RUNNING.lock().await;
    let current = history.work_notes(session_id).ok().flatten().unwrap_or_default();
    let messages = vec![
        Message::system(crate::i18n::with_background_lang(NOTE_TAKER, settings.lang())),
        Message::user(format!(
            "Carnet actuel :\n{}\n\nDernier tour de travail :\n{}",
            if current.trim().is_empty() { "(vide)" } else { current.as_str() },
            turn_transcript(turn)
        )),
    ];
    let raw = match llm.complete(model, &messages, settings.llm.max_tokens, settings.llm.temperature).await {
        Ok(raw) => raw,
        Err(error) => {
            log::warn!("[contexte] carnet de travail non mis à jour : {error}");
            return;
        }
    };
    let Some(notes) = clean_notes(&raw) else {
        log::info!("[contexte] carnet de travail inchangé (rien de neuf)");
        return;
    };
    let notes = crate::sensitive::redact_known(&notes);
    match history.save_work_notes(session_id, &notes) {
        Ok(()) => log::info!("[contexte] carnet de travail à jour : {} caractères", notes.chars().count()),
        // Conversation supprimée pendant l'appel (clé étrangère).
        Err(error) => log::info!("[contexte] carnet de travail non écrit (conversation supprimée ?) : {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::ToolCall;

    fn history() -> History {
        History::new(std::sync::Arc::new(std::sync::Mutex::new(crate::db::Db::open_in_memory().unwrap())))
    }

    /// Le carnet survit d'un tour à l'autre et se remplace ; il disparaît
    /// avec sa conversation.
    #[test]
    fn le_carnet_de_travail_s_enregistre_et_suit_la_conversation() {
        let h = history();
        let s = h.create_session("t").unwrap();
        assert_eq!(h.work_notes(&s).unwrap(), None);
        h.save_work_notes(&s, "- API sur 127.0.0.1:18001").unwrap();
        h.save_work_notes(&s, "- API sur 127.0.0.1:18001\n- cockpit/ écrit").unwrap();
        assert_eq!(h.work_notes(&s).unwrap().as_deref(), Some("- API sur 127.0.0.1:18001\n- cockpit/ écrit"));
        h.delete_session(&s).unwrap();
        assert_eq!(h.work_notes(&s).unwrap(), None);
        assert!(h.save_work_notes(&s, "x").is_err(), "conversation supprimée : clé étrangère");
    }

    #[test]
    fn le_bloc_du_carnet_est_injecte_et_borne() {
        assert_eq!(work_notes_block(None), None);
        assert_eq!(work_notes_block(Some("  ")), None);
        let block = work_notes_block(Some(&"a".repeat(5000))).unwrap();
        assert!(block.starts_with("## Carnet de travail de cette conversation"));
        assert!(block.contains("ne les revérifie pas"));
        assert!(block.chars().count() < NOTES_MAX_CHARS + 200, "{}", block.chars().count());
    }

    /// Ce que le recap ne voyait pas : les résultats d'outils. La relecture
    /// les garde (bornés), avec les appels ; le prompt système n'y est pas ;
    /// un tour trop long garde sa fin.
    #[test]
    fn la_relecture_du_tour_garde_les_resultats_d_outils() {
        let mut appel = Message::assistant("Je regarde le serveur.");
        appel.tool_calls = Some(vec![ToolCall {
            id: "c1".into(),
            name: "mcp_call".into(),
            arguments: serde_json::json!({"server": "vps", "tool": "vps_exec", "arguments": {"command": "docker ps"}}),
        }]);
        let tour = vec![
            Message::system("PROMPT SYSTEME"),
            Message::user("crée le dashboard"),
            appel,
            Message::tool_result("c1", "mcp_call", format!("emile-backend-1 0.0.0.0:18001->8000/tcp {}", "x".repeat(5000))),
            Message::assistant("Fait."),
        ];
        let texte = turn_transcript(&tour);
        assert!(!texte.contains("PROMPT SYSTEME"), "{texte}");
        assert!(texte.contains("Demande : crée le dashboard"), "{texte}");
        assert!(texte.contains("Appel mcp_call") && texte.contains("docker ps"), "{texte}");
        assert!(texte.contains("18001->8000"), "{texte}");
        assert!(texte.chars().count() < 2_500, "résultat borné : {}", texte.chars().count());

        let long: Vec<Message> = (0..200).map(|i| Message::tool_result("c", "read_file", format!("bloc {i} {}", "y".repeat(1000)))).collect();
        let texte = turn_transcript(&long);
        assert!(texte.starts_with("[début du tour coupé]"));
        assert!(texte.contains("bloc 199") && !texte.contains("bloc 0 "), "on garde la fin");
    }

    #[test]
    fn la_reponse_du_modele_est_nettoyee() {
        assert_eq!(clean_notes("RIEN"), None);
        assert_eq!(clean_notes(" rien. "), None);
        assert_eq!(clean_notes(""), None);
        assert_eq!(clean_notes("```markdown\n- port 8787 libre\n```").as_deref(), Some("- port 8787 libre"));
        assert_eq!(clean_notes(&"z".repeat(4000)).unwrap().chars().count(), NOTES_MAX_CHARS);
    }
}
