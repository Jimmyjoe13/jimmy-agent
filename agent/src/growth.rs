//! Revue périodique (mécanisme « curriculum », lot « croissance »).
//!
//! À intervalle réglé, Jimmy relit son journal d'expérience — les leçons
//! tirées de ses échecs d'outils (source « lesson », cf. `memory::learn`) —
//! et rédige dans une session « Revue » du Chat une liste courte de
//! propositions : nouvelle skill, skill à aiguiser, outil qui manque.
//!
//! La revue ne passe PAS par la boucle d'agent : un appel unique de
//! rédaction, aucun outil. Skills et outils restent des propositions que
//! l'utilisateur lit puis décide ; seuls les « Amendement proposé » sont
//! appliqués, inscrits dans les leçons de l'âme (`crate::soul`, garde-fous
//! compris) — l'auto-amélioration voulue par l'utilisateur (28 octobre).
//!
//! Déclenchement : un cycle de 30 minutes vérifie si la revue est due
//! (fenêtre `growth.review_days`, 0 = jamais) **et** s'il y a des leçons
//! nouvelles depuis la précédente revue (`data/growth_state.json`). Jimmy
//! relancé n'oublie pas l'échéance : l'état est sur le disque.

use std::path::PathBuf;

use chrono::{DateTime, Utc};

use crate::core::types::Message;
use crate::error::Result;

const REVIEW: &str = r#"Tu es Jimy, un assistant personnel. Ceci est une revue périodique de ta croissance, écrite pour être lue par ton utilisateur.

Tu reçois tes leçons récentes (tirées de tes échecs d'outils) et la liste de tes skills. Rédige une liste COURTE de propositions concrètes :
- une nouvelle skill à écrire : « name: » un nom en tirets, « description: » quand l'utiliser, et un corps en étapes numérotées ;
- un skill à aiguiser : son nom exact et ce qui lui manque ;
- un outil ou serveur MCP qui manque : le besoin observé, sans solution commerciale précise.

Si les leçons montrent une habitude de travail qui te manque, ajoute une ligne :
« Amendement proposé : » suivi d'UNE phrase impérative, sur la même ligne, puis la leçon qui la motive à la ligne suivante. Elle sera inscrite telle quelle dans les leçons de ton âme : seulement si elle est généralisable, et jamais déjà présente dans ton âme ci-dessous.

Chaque proposition cite la leçon ou l'observation qui la motive. Rien de décoratif : si les leçons ne montrent rien de généralisable, réponds simplement « Rien à proposer cette fois. »
Les skills et outils restent des propositions : l'utilisateur décide."#;

/// Extrait les amendements d'une revue (« Amendement proposé : … », ou sa
/// version anglaise quand la revue est rédigée en anglais), nettoyés du
/// Markdown et des guillemets.
fn proposed_amendments(review: &str) -> Vec<String> {
    review
        .lines()
        .filter_map(|line| {
            let plain = line.replace("**", "");
            let lower = plain.to_lowercase();
            let at = lower.find("amendement proposé").or_else(|| lower.find("proposed amendment"))?;
            // `get` : un indice décalé par la mise en minuscules ne panique pas.
            let rest = plain.get(at..)?.split_once(':')?.1;
            let text = rest.trim().trim_matches(['«', '»', '"', '“', '”', ' ', '\u{a0}']).trim();
            (!text.is_empty()).then(|| text.to_string())
        })
        .collect()
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct GrowthState {
    #[serde(default)]
    pub last_review: Option<String>,
    #[serde(default)]
    pub lessons_seen: usize,
}

fn state_file(app: &crate::App) -> PathBuf {
    app.paths.data.join("growth_state.json")
}

fn load_state(app: &crate::App) -> GrowthState {
    std::fs::read_to_string(state_file(app))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_state(app: &crate::App, state: &GrowthState) {
    if let Ok(raw) = serde_json::to_string_pretty(state) {
        let _ = std::fs::write(state_file(app), raw);
    }
}

/// La revue est-elle due ? Due = fenêtre écoulée (ou première revue) **et**
/// des leçons pas encore relues. Pas de leçon, réglage à 0 : jamais.
pub fn due(review_days: u32, state: &GrowthState, lesson_count: usize) -> bool {
    if review_days == 0 || lesson_count == 0 || lesson_count <= state.lessons_seen {
        return false;
    }
    let elapsed = match &state.last_review {
        None => true,
        Some(ts) => DateTime::parse_from_rfc3339(ts)
            .map(|then| Utc::now().signed_duration_since(then))
            .map(|d| d >= chrono::Duration::days(i64::from(review_days)))
            .unwrap_or(true),
    };
    elapsed
}

/// Écrit la revue dans une session « Revue ». Renvoie le texte proposé, ou
/// `None` si la revue n'était pas due. Un appel unique au modèle, sans
/// outil : la session ne peut rien appliquer.
pub async fn review(app: &crate::App) -> Result<Option<String>> {
    let settings = app.settings();
    let state = load_state(app);
    let lessons = app.memory.lessons(12)?;
    if !due(settings.growth.review_days, &state, lessons.len()) {
        return Ok(None);
    }

    let mut context = String::from("Tes leçons récentes :\n");
    for lesson in lessons.iter().take(12) {
        context.push_str(&format!("- {}\n", lesson.content));
    }
    if lessons.is_empty() {
        context.push_str("- (aucune)\n");
    }
    context.push_str("\nTes skills :\n");
    let skills = app.skills.list()?;
    for skill in skills.iter().take(20) {
        context.push_str(&format!("- {} : {}\n", skill.name, skill.description));
    }
    if skills.is_empty() {
        context.push_str("- (aucun)\n");
    }
    // L'âme actuelle : pour ne pas reproposer une leçon déjà acquise.
    context.push_str("\nTon âme actuelle :\n");
    context.push_str(&crate::soul::read_for_prompt(&app.paths.data));
    context.push('\n');

    let messages = vec![
        Message::system(crate::i18n::with_background_lang(REVIEW, settings.lang())),
        Message::user(context),
    ];
    let proposals = app
        .llm
        .complete(&settings.llm.model, &messages, 800, None)
        .await?;
    let proposals = proposals.trim().to_string();

    // Auto-amélioration : chaque amendement rejoint les leçons de l'âme
    // (garde-fous de `soul` : taille bornée, sans doublon, `.bak`, journal).
    // Le pied de session dit ce qui a été inscrit, et comment revenir dessus.
    let mut applied = Vec::new();
    let mut refused = Vec::new();
    for amendment in proposed_amendments(&proposals) {
        match crate::soul::add_lesson(&app.paths.data, &amendment, "jimy", "revue périodique") {
            Ok(true) => applied.push(amendment),
            Ok(false) => {}
            Err(error) => refused.push(format!("{amendment} ({error})")),
        }
    }
    let mut proposals = proposals;
    if !applied.is_empty() || !refused.is_empty() {
        proposals.push_str("\n\n---\n");
        for lesson in &applied {
            proposals.push_str(&format!("Inscrit dans mon âme : {lesson}\n"));
        }
        for lesson in &refused {
            proposals.push_str(&format!("Non inscrit : {lesson}\n"));
        }
        proposals.push_str("Réglages → Âme pour relire, modifier ou annuler.");
    }

    let session = app.history.create_session("Revue")?;
    app.history
        .append(&session, &Message::assistant(proposals.clone()))?;
    app.history.touch(&session)?;

    save_state(
        app,
        &GrowthState {
            last_review: Some(Utc::now().to_rfc3339()),
            lessons_seen: lessons.len(),
        },
    );
    log::info!("[growth] revue écrite ({} leçons relues)", lessons.len());
    Ok(Some(proposals))
}

/// Boucle de fond : vérifie toutes les 30 minutes si une revue est due.
/// Premier contrôle 5 minutes après le démarrage, pour laisser la mémoire
/// et les serveurs s'installer. Jamais deux revues simultanées.
pub async fn review_loop(app: std::sync::Arc<crate::App>) {
    let mut first = true;
    loop {
        let wait = if first {
            first = false;
            std::time::Duration::from_secs(5 * 60)
        } else {
            std::time::Duration::from_secs(30 * 60)
        };
        tokio::time::sleep(wait).await;
        if let Err(error) = review(&app).await {
            log::warn!("[growth] revue impossible : {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(last: Option<&str>, seen: usize) -> GrowthState {
        GrowthState { last_review: last.map(String::from), lessons_seen: seen }
    }

    #[test]
    fn pas_de_lecon_pas_de_revue() {
        assert!(!due(7, &state(None, 0), 0));
    }

    #[test]
    fn premiere_lecon_declenche_premiere_revue() {
        assert!(due(7, &state(None, 0), 2));
    }

    #[test]
    fn fenetre_non_ecoulee_pas_de_revue() {
        let recent = Utc::now().to_rfc3339();
        assert!(!due(7, &state(Some(&recent), 0), 2));
    }

    #[test]
    fn fenetre_ecoulee_avec_lecons_nouvelles() {
        let old = (Utc::now() - chrono::Duration::days(8)).to_rfc3339();
        assert!(due(7, &state(Some(&old), 1), 3));
    }

    #[test]
    fn fenetre_ecoulee_sans_lecon_nouvelle() {
        let old = (Utc::now() - chrono::Duration::days(8)).to_rfc3339();
        assert!(!due(7, &state(Some(&old), 3), 3));
    }

    #[test]
    fn regle_desactivee() {
        assert!(!due(0, &state(None, 0), 5));
    }

    #[test]
    fn amendements_extraits_de_la_revue() {
        let review = "- Skill à aiguiser : deploy-vps\n\
                      **Amendement proposé :** « Lis le journal d'erreur avant de relancer une commande. »\n\
                      Leçon : trois relances à l'aveugle.\n\
                      Proposed amendment: Check the port before starting a server.\n\
                      Amendement proposé :\n";
        assert_eq!(
            proposed_amendments(review),
            vec![
                "Lis le journal d'erreur avant de relancer une commande.".to_string(),
                "Check the port before starting a server.".to_string(),
            ]
        );
        assert!(proposed_amendments("Rien à proposer cette fois.").is_empty());
    }

    #[test]
    fn horodatage_illisible_on_reprend() {
        assert!(due(7, &state(Some("pas une date"), 0), 2));
    }
}
