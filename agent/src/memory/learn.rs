//! Apprentissage de la mémoire personnelle.
//!
//! Après chaque échange, Jimmy demande au modèle d'extraire ce qui mérite
//! d'être retenu : préférences, règles de travail, résultats. Un modèle unique
//! d'extraction est utilisé, très court, qui répond en JSON.
//!
//! Pourquoi un modèle et pas des règles ? Un mot-clé « toujours », « plutôt »,
//! « je préfère » attrape peu de choses, et surtout produit beaucoup de faux
//! positifs. Demander au modèle coûte une requête par échange — acceptable pour
//! un assistant personnel — et produit des souvenirs utilisables.
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
{"kind": "semantic|procedural|episodic", "content": "une phrase à la troisième personne"}

Règles :
- "semantic" : un fait ou une préférence stable de l'utilisateur.
- "procedural" : une règle, une façon de travailler, une leçon.
- "episodic" : ce qui vient d'être fait et qui sert d'exemple.
- Ne retiens ni compliment, ni politesse, ni fait déjà évident, ni détail sans
  portée. Pas de secrets, pas de coordonnées, pas de mot de passe.
- Si rien ne mérite d'être retenu, réponds []."#;

#[derive(Debug, Deserialize)]
struct Extracted {
    kind: String,
    content: String,
}

pub async fn learn(
    llm: &LlmClient,
    settings: &Settings,
    request: &str,
    answer: &str,
) -> Vec<(MemoryKind, String)> {
    if !settings.memory.auto_learn {
        return Vec::new();
    }
    let messages = vec![
        Message::system(EXTRACTOR),
        Message::user(format!(
            "Demande de l'utilisateur :\n{request}\n\nRéponse de Jimmy :\n{answer}"
        )),
    ];
    let Ok(raw) = llm.complete(&settings.llm.model, &messages, 400, Some(0.0)).await else {
        return Vec::new();
    };
    parse(&raw)
}

fn parse(raw: &str) -> Vec<(MemoryKind, String)> {
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
            let kind = match item.kind.as_str() {
                "procedural" => MemoryKind::Procedural,
                "episodic" => MemoryKind::Episodic,
                _ => MemoryKind::Semantic,
            };
            Some((kind, content))
        })
        .take(4)
        .collect()
}

/// Isole le premier tableau JSON d'une réponse qui peut être entourée de texte.
fn extract_json_array(raw: &str) -> Option<String> {
    let start = raw.find('[')?;
    let end = raw.rfind(']')?;
    if end <= start {
        return None;
    }
    Some(raw[start..=end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extraction_propre() {
        let raw = r#"Voici : [{"kind":"semantic","content":"L'utilisateur préfère des réponses courtes."}]"#;
        let items = parse(raw);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].0, MemoryKind::Semantic);
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
}