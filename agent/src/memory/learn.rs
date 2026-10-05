//! Apprentissage de la mémoire personnelle.
//!
//! Après chaque échange, Jimmy demande au modèle d'extraire ce qui mérite
//! d'être retenu : préférences, règles de travail, résultats — et, s'il y a
//! eu un échec d'outil, une **leçon** (mécanisme « Reflexion » : l'échec
//! observé + la façon validée de s'y prendre). Un modèle unique d'extraction
//! est utilisé, très court, qui répond en JSON.
//!
//! Pourquoi un modèle et pas des règles ? Un mot-clé « toujours », « plutôt »,
//! « je préfère » attrape peu de choses, et surtout produit beaucoup de faux
//! positifs. Demander au modèle coûte une requête par échange — acceptable pour
//! un assistant personnel — et produit des souvenirs utilisables. Un appel
//! unique couvre faits et leçons : deux appels doubleraient l'exposition à la
//! dérive du modèle (observé : l'extracteur de leçons séparé revenait
//! illisible une fois sur deux).
//!
//! Garde-fous : réponse illisible = rien n'est appris, jamais d'échec. Et
//! l'extraction ne s'appuie que sur l'échange courant, pas sur l'historique
//! complet : pas de dérive, pas de coût explosif.

use serde::Deserialize;

use crate::config::Settings;
use crate::core::types::Message;
use crate::memory::MemoryKind;
use crate::providers::llm::LlmClient;

const EXTRACTOR: &str = r#"Tu extrais ce qui mérite d'être retenu d'un échange, pour un assistant personnel.

Réponds uniquement par un tableau JSON. Un élément par souvenir :
{"kind": "semantic|procedural|episodic|lesson", "content": "une phrase à la troisième personne"}

Règles :
- "semantic" : un fait ou une préférence stable de l'utilisateur.
- "procedural" : une règle, une façon de travailler.
- "episodic" : ce qui vient d'être fait et qui sert d'exemple.
- "lesson" : SEULEMENT si des échecs figurent dans « Outils en échec »
  ci-dessous : l'échec observé et la façon validée de s'y prendre la
  prochaine fois. Au plus deux leçons, généralisées sans rien inventer
  (un pattern suffit : « ce dossier », « l'outil X », pas de chemin précis).
- Ne retiens ni compliment, ni politesse, ni fait déjà évident, ni détail sans
  portée. Pas de secrets, pas de coordonnées, pas de mot de passe.
- Si rien ne mérite d'être retenu, réponds []."#;

#[derive(Debug, Deserialize)]
struct Extracted {
    kind: String,
    content: String,
}

/// Extracte les souvenirs d'un échange. Renvoie (genre, source, contenu) ;
/// la source « lesson » distingue les leçons de l'expérience des souvenirs
/// ordinaires (« auto »).
pub async fn learn(
    llm: &LlmClient,
    settings: &Settings,
    request: &str,
    answer: &str,
    tool_errors: &[(String, String)],
) -> Vec<(MemoryKind, &'static str, String)> {
    if !settings.memory.auto_learn {
        return Vec::new();
    }
    // Une conversation sur `auto_learn_every` seulement : le réglage existait
    // mais n'était jamais lu, et chaque tour payait un appel de plus au modèle
    // — qui, sur un modèle gratuit, retarde aussi la réponse suivante.
    // Exception : un tour qui a connu des échecs d'outils extrait TOUJOURS
    // (la leçon ne doit pas être perdue à la stat d'échantillonnage).
    static TURNS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let every = u64::from(settings.memory.auto_learn_every.max(1));
    if tool_errors.is_empty() && TURNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % every != 0 {
        return Vec::new();
    }
    let mut digest = String::new();
    for (name, error) in tool_errors.iter().take(6) {
        digest.push_str(&format!("- {} : {}\n", name, error));
    }
    let incidents = if digest.is_empty() {
        String::new()
    } else {
        format!("\n\nOutils en échec :\n{digest}")
    };
    let messages = vec![
        Message::system(EXTRACTOR),
        Message::user(format!(
            "Demande de l'utilisateur :\n{request}\n\nRéponse de Jimmy :\n{answer}{incidents}"
        )),
    ];
    // L'extraction tourne en tâche de fond (aucune latence pour
    // l'utilisateur) mais le fournisseur est capricieux : erreurs passagères
    // et réponses illisibles. Deux tentatives, jamais plus — au-delà, on
    // renonce sans bruit, rien n'est appris. Le délai avant la seconde
    // dépend de la cause : après une erreur réseau (panne du fournisseur),
    // 800 ms ne peuvent pas suffire (observé à l'usage : les deux
    // tentatives se cassaient dans la même seconde) → 5 s, le temps que la
    // coupure passe ; sur une réponse illisible, 800 ms suffisent.
    for attempt in 1..=2 {
        let mut wait = SHORT_RETRY;
        // `max_tokens` : 400 coupait un JSON bien formaté d'un ```fence``` en
        // pleine trajectoire incidente (observé à l'usage, 12:50) — la tâche
        // est de fond, 800 ne retardent jamais une réponse.
        match llm.complete(&settings.llm.model, &messages, 800, Some(0.0)).await {
            Ok(raw) => {
                if extract_json_array(&raw).is_some() {
                    return parse(&raw);
                }
                log::warn!(
                    "[memory] extraction illisible (tentative {attempt}) : {}",
                    raw.chars().take(200).collect::<String>()
                );
            }
            Err(error) => {
                // Panne réseau : la seconde tentative 800 ms plus tard ne
                // passerait jamais (observé à l'usage, 10:38) → on attend
                // NETWORK_RETRY avant de rejouer. Leçon définitivement perdue
                // si la seconde tente aussi dans la coupure : la revue la
                // racontera (les outils échoués restent dans la trajectoire).
                if is_network_error(&error) {
                    wait = NETWORK_RETRY;
                    log::warn!("[memory] extraction en échec réseau (tentative {attempt}) : {error}");
                } else {
                    log::warn!("[memory] extraction en échec (tentative {attempt}) : {error}");
                }
            }
        }
        if attempt < 2 {
            tokio::time::sleep(wait).await;
        }
    }
    Vec::new()
}

/// Délai avant la seconde tentative d'extraction.
pub const SHORT_RETRY: std::time::Duration = std::time::Duration::from_millis(800);
/// Après une panne réseau, 800 ms ne suffisent pas (les deux tentatives se
/// cassaient dans la même seconde) : on laisse la coupure passer.
pub const NETWORK_RETRY: std::time::Duration = std::time::Duration::from_secs(5);

/// Une erreur « le service n'a pas répondu » (réseau, DNS, coupure) ?
/// Distingue l'échec de connexion d'une réponse cassée : le rejeu après
/// coupure doit attendre, pas le rejeu d'une réponse dérivée.
pub fn is_network_error(error: &crate::error::Error) -> bool {
    let text = error.to_string();
    text.contains("error sending request")
        || text.contains("timed out")
        || text.contains("timeout")
        || text.contains("connection")
        || text.contains("DNS")
        || text.contains("refused")
}

fn parse(raw: &str) -> Vec<(MemoryKind, &'static str, String)> {
    let json = extract_json_array(raw);
    let Some(json) = json else {
        log::debug!("[memory] extraction illisible, ignorée");
        return Vec::new();
    };
    let Ok(items) = serde_json::from_str::<Vec<Extracted>>(&json) else {
        return Vec::new();
    };
    items
        .into_iter()
        .filter_map(|item| {
            let content = item.content.trim().to_string();
            if content.len() < 12 || content.len() > 400 {
                return None;
            }
            let (kind, source) = match item.kind.as_str() {
                "procedural" => (MemoryKind::Procedural, "auto"),
                "episodic" => (MemoryKind::Episodic, "auto"),
                "lesson" => (MemoryKind::Procedural, "lesson"),
                _ => (MemoryKind::Semantic, "auto"),
            };
            Some((kind, source, content))
        })
        .take(4)
        .collect()
}

/// Isole le premier bloc JSON (objet ou tableau) d'une réponse qui peut être
/// entourée de texte.
pub fn extract_json_block(raw: &str) -> Option<String> {
    let start = raw.find(['[', '{'])?;
    let close = match raw[start..].chars().next()? {
        '[' => ']',
        '{' => '}',
        _ => return None,
    };
    let end = raw.rfind(close)?;
    if end <= start {
        return None;
    }
    Some(raw[start..=end].to_string())
}

/// Isole le premier tableau JSON d'une réponse qui peut être entourée de texte.
fn extract_json_array(raw: &str) -> Option<String> {
    let json = extract_json_block(raw)?;
    json.starts_with('[').then_some(json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panne_reseau_reconnue_et_distinguee_dune_reponse_fausse() {
        let coupure = crate::error::Error::provider(
            "OpenCode Go",
            "error sending request for url (https://opencode.ai/zen/go/v1/chat/completions)",
        );
        assert!(is_network_error(&coupure));
        let mauvaise_reponse = crate::error::Error::Tool("le modèle a répondu '[]'".into());
        assert!(!is_network_error(&mauvaise_reponse));
    }

    #[test]
    fn delais_reseau_et_court() {
        assert_eq!(SHORT_RETRY, std::time::Duration::from_millis(800));
        assert_eq!(NETWORK_RETRY, std::time::Duration::from_secs(5));
    }

    #[test]
    fn extraction_propre() {
        let raw = r#"Voici : [{"kind":"semantic","content":"L'utilisateur préfère des réponses courtes."}]"#;
        let items = parse(raw);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].0, MemoryKind::Semantic);
        assert_eq!(items[0].1, "auto");
    }

    #[test]
    fn tableau_vide_ignore() {
        assert!(parse("[]").is_empty());
    }

    #[test]
    fn reponse_illisible_ignoree() {
        assert!(parse("je ne sais pas").is_empty());
    }

    #[test]
    fn souvenirs_trop_courts_ecartes() {
        let raw = r#"[{"kind":"semantic","content":"oui"}]"#;
        assert!(parse(raw).is_empty());
    }

    #[test]
    fn lecons_extraites_et_bornees() {
        let raw = r#"Voici : [{"kind":"lesson","content":"search_files a besoin d'une requête non vide ; vérifier l'orthographe du chemin avant d'appeler."},{"kind":"lesson","content":"trop court"}]"#;
        let items = parse(raw);
        assert_eq!(items.len(), 1, "{items:?}");
        assert_eq!(items[0].0, MemoryKind::Procedural);
        assert_eq!(items[0].1, "lesson");
        assert!(items[0].2.contains("search_files"));
    }

    #[test]
    fn lecons_illisibles_ignorees() {
        assert!(parse("aucune leçon ici").is_empty());
        assert!(parse("[{pas du json}]").is_empty());
    }
}