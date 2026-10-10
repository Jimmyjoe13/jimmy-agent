//! Lignes du bloc « Maintenant » du prompt système (lot 5 du contexte).
//!
//! Avant, chaque signal arrivait au modèle à sa façon (contexte de session,
//! bloc de la tâche de fond, rien du navigateur ni des cartes d'accord) : il
//! n'avait aucune vue de ce qui se passe au moment de la demande. Ces
//! fonctions donnent chacune une ligne, assemblées dans un seul bloc par
//! `prompt::system_parts`.

use chrono::{Datelike, Local, Timelike};
use rusqlite::params;

use crate::core::history::History;

/// Au-delà, la dernière page du navigateur n'est plus « ouverte » (il a pu
/// être fermé) : on ne la montre plus.
const BROWSER_FRESH_MINUTES: i64 = 120;

/// « vendredi 10 octobre 2026, 15 h 42 — échange vocal ».
pub fn now_line(voice: bool) -> String {
    const JOURS: [&str; 7] = ["lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche"];
    const MOIS: [&str; 12] = [
        "janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre", "novembre",
        "décembre",
    ];
    let now = Local::now();
    format!(
        "- {} {} {} {}, {} h {:02} — échange {}",
        JOURS[now.weekday().num_days_from_monday() as usize],
        now.day(),
        MOIS[now.month0() as usize],
        now.year(),
        now.hour(),
        now.minute(),
        if voice { "vocal" } else { "écrit" }
    )
}

/// Cartes d'accord sans réponse (une autre conversation, une tâche de fond).
pub fn approvals_line(scopes: &[String]) -> Option<String> {
    if scopes.is_empty() {
        return None;
    }
    let list = scopes.iter().map(|s| format!("« {s} »")).collect::<Vec<_>>().join(", ");
    Some(format!("- Carte d'accord en attente de la réponse de l'utilisateur : {list}"))
}

/// Dernière page ouverte dans le navigateur de Jimmy, si elle est récente.
/// Lue dans l'historique (appels `browser_navigate`, directs ou via
/// `mcp_call`) : le navigateur est commun à toutes les conversations.
pub fn browser_line(history: &History) -> Option<String> {
    let (raw, at): (String, String) = {
        let db = history.db.lock().unwrap_or_else(|e| e.into_inner());
        db.conn()
            .query_row(
                "SELECT tool_calls, created_at FROM messages
                 WHERE tool_calls LIKE '%browser_navigate%' ORDER BY created_at DESC, rowid DESC LIMIT 1",
                params![],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok()?
    };
    let minutes = (chrono::Utc::now() - chrono::DateTime::parse_from_rfc3339(&at).ok()?.with_timezone(&chrono::Utc))
        .num_minutes();
    if minutes > BROWSER_FRESH_MINUTES {
        return None;
    }
    let calls: Vec<serde_json::Value> = serde_json::from_str(&raw).ok()?;
    let url = calls.iter().rev().find_map(navigated_url)?;
    let url: String = url.chars().take(160).collect();
    let when = if minutes < 1 { "à l'instant".to_string() } else { format!("il y a {minutes} min") };
    Some(format!("- Ton navigateur : dernière page ouverte {when} — {url}"))
}

/// URL d'un appel `browser_navigate` : direct (`{url}`) ou passé par
/// `mcp_call` (`{tool: "browser_navigate", arguments: {url}}`).
fn navigated_url(call: &serde_json::Value) -> Option<String> {
    let name = call.get("name")?.as_str()?;
    let args = call.get("arguments")?;
    let args = if name.ends_with("browser_navigate") {
        args
    } else if name == "mcp_call" && args.get("tool")?.as_str()? == "browser_navigate" {
        args.get("arguments")?
    } else {
        return None;
    };
    args.get("url")?.as_str().map(str::to_string)
}

/// Fenêtre que l'utilisateur regarde, si le réglage l'autorise. Bornée à
/// 400 ms : la lister ne doit jamais retarder une réponse.
pub async fn window_line(enabled: bool) -> Option<String> {
    if !enabled {
        return None;
    }
    let excluded = vec![std::process::id()];
    let task = tokio::task::spawn_blocking(move || crate::screen::active_window_label(&excluded));
    let label = tokio::time::timeout(std::time::Duration::from_millis(400), task).await.ok()?.ok()??;
    let label: String = label.chars().take(140).collect();
    Some(format!("- Fenêtre que l'utilisateur a sous les yeux : « {label} »"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::{Message, ToolCall};

    #[test]
    fn la_derniere_page_du_navigateur_est_retrouvee() {
        let h = History::new(std::sync::Arc::new(std::sync::Mutex::new(crate::db::Db::open_in_memory().unwrap())));
        let s = h.create_session("t").unwrap();
        assert_eq!(browser_line(&h), None, "jamais navigué : rien");
        let mut call = Message::assistant("");
        call.tool_calls = Some(vec![ToolCall {
            id: "1".into(),
            name: "mcp_call".into(),
            arguments: serde_json::json!({ "server": "navigateur", "tool": "browser_navigate", "arguments": { "url": "https://elu-scraper.fr/admin" } }),
        }]);
        h.append(&s, &call).unwrap();
        let line = browser_line(&h).unwrap();
        assert!(line.contains("https://elu-scraper.fr/admin") && line.contains("à l'instant"), "{line}");
    }

    #[test]
    fn les_cartes_en_attente_sont_nommees() {
        assert_eq!(approvals_line(&[]), None);
        let line = approvals_line(&["navigateur : clic sur un élément qui engage".into()]).unwrap();
        assert!(line.contains("« navigateur : clic sur un élément qui engage »"), "{line}");
    }

    #[test]
    fn la_date_et_le_canal_sont_dits() {
        assert!(now_line(true).ends_with("échange vocal"));
        assert!(now_line(false).contains(&Local::now().year().to_string()));
    }

    #[tokio::test]
    async fn la_fenetre_ne_part_pas_sans_le_reglage() {
        assert_eq!(window_line(false).await, None);
    }
}
