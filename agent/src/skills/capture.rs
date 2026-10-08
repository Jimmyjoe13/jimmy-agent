//! Capture de compétence (mécanisme « Voyager »).
//!
//! La bibliothèque de compétences de Jimmy a deux moitiés : l'écriture à la
//! demande (`create_skill`, le modèle juge utile d'empiler une procédure) et
//! la capture post-trajectoire, décrite ici. Après une demande qui a mobilisé
//! plusieurs outils et s'est conclue par une réponse, un appel unique demande
//! au modèle de condenser **la démarche** en un skill réutilisable — ou de
//! renoncer. La capacité se cumule alors au lieu de repartir de zéro à chaque
//! tâche : la mémoire devient exécutable.
//!
//! Deux garde-fous, même esprit que l'extraction de mémoire :
//!
//! * le modèle désigne d'abord le skill existant le plus proche (`proche:`,
//!   vérifié côté code : un nom inconnu est ignoré) ; à défaut, une similarité
//!   stricte (0,6, calibrée sur le corpus réel) force la fusion au lieu de
//!   créer un quasi-doublon ;
//! * réponse illisible = rien n'est capturé, jamais d'échec. Deux tentatives,
//!   jamais plus — l'extraction tourne en tâche de fond, aucune latence pour
//!   l'utilisateur.

use crate::config::Settings;
use crate::core::types::Message;
use crate::providers::llm::LlmClient;
use crate::skills::SkillStore;

const CAPTURE: &str = r#"Tu condenses une trajectoire réussie d'un assistant en une compétence réutilisable.

Réponds exactement avec ces quatre lignes (ou uniquement [] si rien ne mérite un skill) :
proche: NOM exact du skill existant le plus proche, ou aucun
name: nom-court-en-tirets
description: quand utiliser ce skill, une phrase
body: les étapes, numérotées, chaque étape sur sa propre ligne

Règles :
- La trajectoire a mobilisé plusieurs outils : ce que le skill doit transmettre est la DÉMARCHE (comment chercher, quoi vérifier, dans quel ordre), pas le résultat de la fois passée.
- Un skill de la liste couvre déjà ce besoin ? Nomme-le dans `proche:` et
  réutilise son NOM exact dans `name:` (aiguisage : son corps sera réécrit,
  pas de doublon). Tu ne crées un nom nouveau que si aucun existant ne
  convient, et tu l'écris dans `proche: aucun`.
- Le corps : étapes généralisables (patterns plutôt que chemins d'un seul usage), sans secret, sans coordonnées.
- Une trajectoire banale (une lecture, une question sans outils) → []."#;

#[derive(Debug, Clone)]
pub struct ProposedSkill {
    pub name: String,
    pub description: String,
    pub body: String,
    /// Nom existant désigné par le modèle (`proche:`), le cas échéant.
    /// Vérifié par l'appelant : un nom inconnu est ignoré, pas créé.
    pub close_to: Option<String>,
}

/// Tente de condenser la trajectoire en skill. `tools_used` = noms des outils
/// appelés ; l'appelant ne déclenche la capture qu'au-delà de quelques outils.
pub async fn capture(
    llm: &LlmClient,
    settings: &Settings,
    request: &str,
    answer: &str,
    tools_used: &[String],
    skills: &SkillStore,
) -> Option<ProposedSkill> {
    if !settings.skills.auto_capture {
        return None;
    }
    let existing: Vec<String> = skills
        .list()
        .unwrap_or_default()
        .iter()
        .map(|s| format!("- {} : {}", s.name, s.description))
        .collect();
    let catalogue = if existing.is_empty() {
        "Aucun skill existant.".to_string()
    } else {
        format!("Skills existants :\n{}", existing.join("\n"))
    };
    let messages = vec![
        Message::system(CAPTURE),
        Message::user(format!(
            "Demande de l'utilisateur :\n{request}\n\nOutils utilisés :\n{}\n\nRéponse donnée :\n{answer}\n\n{catalogue}",
            tools_used.iter().map(|t| format!("- {t}")).collect::<Vec<_>>().join("\n")
        )),
    ];
    for attempt in 1..=2 {
        // Panne réseau : la seconde tentative 800 ms plus tard ne passerait
        // jamais (inclurait la même coupure) → on attend NETWORK_RETRY.
        let mut wait = SHORT_RETRY;
        // Le corps d'un skill peut être long : 1 200 jetons le couvrent sans
        // couper (le format trois lignes est devenu prolixe à l'usage).
        match llm.complete(&settings.llm.model, &messages, 1200, Some(0.0)).await {
            Ok(raw) => match parse(&raw) {
                Some(proposal) => return Some(proposal),
                None => {
                    // [] = renoncement légitime, pas une panne.
                    if raw.trim().starts_with("[]") {
                        return None;
                    }
                    log::warn!("[skills] capture illisible (tentative {attempt}) : {}",
                        crate::sensitive::mask_text(&raw.chars().take(200).collect::<String>()));
                }
            },
            Err(error) => {
                if crate::memory::learn::is_network_error(&error) {
                    wait = NETWORK_RETRY;
                    log::warn!("[skills] capture en échec réseau (tentative {attempt}) : {error}");
                } else {
                    log::warn!("[skills] capture en échec (tentative {attempt}) : {error}");
                }
            }
        }
        if attempt < 2 {
            tokio::time::sleep(wait).await;
        }
    }
    None
}

/// Délai avant la seconde tentative, partagé avec l'extraction de mémoire
/// (`memory::learn`) : même jugement d'échec réseau, même attente.
use crate::memory::learn::{NETWORK_RETRY, SHORT_RETRY};

/// Le format à trois lignes, volontairement pas du JSON : le corps d'un
/// skill est multi-lignes, et le modèle gratuit pretty-printait ses chaînes
/// avec de vrais sauts de ligne — JSON invalide, capture perdue. Ici, la
/// prose autour est ignorée, et seule la ligne `body:` amorce un bloc libre.
fn parse(raw: &str) -> Option<ProposedSkill> {
    if raw.trim().starts_with("[]") {
        return None;
    }
    let mut name = String::new();
    let mut description = String::new();
    let mut body = String::new();
    let mut close_to: Option<String> = None;
    let mut in_body = false;
    for line in raw.lines() {
        if in_body {
            body.push_str(line);
            body.push('\n');
            continue;
        }
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("name:") {
            name = SkillStore::slugify(rest.trim());
        } else if let Some(rest) = trimmed.strip_prefix("proche:") {
            // `aucun`, vide ou illisible = pas de désignation.
            let slug = SkillStore::slugify(rest.trim());
            if !slug.is_empty() && slug != "skill" && slug != "aucun" {
                close_to = Some(slug);
            }
        } else if let Some(rest) = trimmed.strip_prefix("description:") {
            description = rest.trim().to_string();
        } else if trimmed.starts_with("body:") {
            in_body = true;
            body = trimmed.strip_prefix("body:").unwrap_or("").trim().to_string();
            if !body.is_empty() {
                body.push('\n');
            }
        }
    }
    if name.is_empty() || name == "skill" || description.is_empty() || body.trim().is_empty() {
        return None;
    }
    let description = description.to_string();
    let body = body.trim_end().to_string();
    if description.chars().count() < 12 || body.chars().count() < 30 {
        return None;
    }
    // Trop long = tronqué, pas rejeté : à l'usage, le modèle répète ou
    // reformule après le corps et l'excédent faisait perdre toute la capture
    // (12:55). La troncature coupe à la ligne précédente, sans orphelin.
    let truncate = |text: &str, max: usize| -> String {
        if text.chars().count() <= max {
            return text.to_string();
        }
        let cut: String = text.chars().take(max).collect();
        match cut.rfind('\n') {
            Some(pos) if pos > max / 2 => cut[..pos].trim_end().to_string(),
            _ => cut.trim_end().to_string(),
        }
    };
    let description = truncate(&description, 200);
    let body = truncate(&body, 3000);
    Some(ProposedSkill { name, description, body, close_to })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_propre_avec_prose_autour() {
        let raw = "Bien sûr !\nname: Auditer du code Rust\ndescription: Quand l'utilisateur demande un audit de code Rust.\nbody: 1. Lire le Cargo.toml.\n2. Chercher les unwrap.\n3. Vérifier les erreurs.";
        let proposal = parse(raw).expect("proposé");
        assert_eq!(proposal.name, "auditer-du-code-rust");
        assert!(proposal.body.contains("Cargo.toml"));
        assert!(proposal.body.lines().count() >= 3);
    }

    #[test]
    fn renoncement_explicite() {
        assert!(parse("[]").is_none());
    }

    #[test]
    fn proche_designe_ou_aucun() {
        let raw = "proche: diagnostiquer-demarrage-docker\nname: diagnostiquer-demarrage-docker\ndescription: Quand Docker refuse de demarrer.\nbody: 1. Regarder les journaux du service.";
        let proposal = parse(raw).expect("propose");
        assert_eq!(proposal.close_to.as_deref(), Some("diagnostiquer-demarrage-docker"));
        let raw = "proche: aucun\nname: truc-neuf\ndescription: Un besoin jamais vu nulle part ailleurs.\nbody: 1. Faire la chose demandeuse en premier lieu.";
        let proposal = parse(raw).expect("propose");
        assert!(proposal.close_to.is_none());
        // Sans ligne proche : creation, comme avant.
        let raw = "name: truc-neuf\ndescription: Un besoin jamais vu nulle part ailleurs.\nbody: 1. Faire la chose demandeuse en premier lieu.";
        let proposal = parse(raw).expect("propose");
        assert!(proposal.close_to.is_none());
    }

    #[test]
    fn corps_trop_long_tronque_a_la_ligne_pas_rejete() {
        let lines: Vec<String> = (0..120).map(|i| format!("{}. Étape {i} : lire, vérifier, écrire, recommencer proprement.", i + 1)).collect();
        let raw = format!("name: gros-skill\ndescription: Une description parfaitement nette et utile.\nbody: {}", lines.join("\n"));
        let proposal = parse(&raw).expect("accepté");
        assert!(proposal.body.chars().count() <= 3000);
        assert!(proposal.body.lines().count() < 120, "la troncature coupe à la ligne");
        assert!(proposal.body.ends_with("proprement."));
    }

    #[test]
    fn illisible_rejete() {
        assert!(parse("aucun skill ici").is_none());
        assert!(parse("name: audit\ndescription: court\nbody: rien").is_none());
    }

    #[test]
    fn trop_court_rejete() {
        let raw = "name: audit\ndescription: court mais ok\ncorps vide";
        assert!(parse(raw).is_none());
    }
}
