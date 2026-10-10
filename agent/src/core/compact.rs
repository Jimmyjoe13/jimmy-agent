//! Compaction du contexte **pendant** une demande.
//!
//! Une sortie d'outil peut peser 12 000 caractères (`MAX_TOOL_OUTPUT`) et une
//! tâche de fond enchaîner jusqu'à 80 étapes : sans compaction, la liste de
//! messages renvoyée au modèle à chaque étape grossit sans fin, jusqu'à
//! dépasser la fenêtre du modèle (requête refusée) ou le rendre lent et cher.
//!
//! La règle est mécanique, sans appel au modèle (aucune latence, aucune
//! dérive) : au-delà de [`BUDGET_CHARS`], les **vieux** résultats d'outils
//! sont remplacés, du plus ancien au plus récent, par une ligne qui dit ce
//! qu'ils étaient ; puis, si ça ne suffit pas, les gros arguments des vieux
//! appels (le contenu d'un `write_file`). Les derniers tours restent intacts :
//! c'est sur eux que le modèle raisonne. Chaque appel d'outil garde son
//! résultat (même réduit), sinon l'API refuse la séquence.

use crate::core::types::{Message, Role};

/// Budget du contexte envoyé à chaque étape, en caractères (~20 k jetons) :
/// large pour un travail normal, et la compaction ne touche que le passé.
pub const BUDGET_CHARS: usize = 80_000;
/// Derniers tours d'outils jamais compactés.
pub const KEEP_ROUNDS: usize = 2;
/// En dessous, un résultat est laissé tel quel (la ligne de remplacement
/// coûterait presque autant).
const MIN_COMPACT_CHARS: usize = 400;
/// Début du résultat gardé dans la ligne de remplacement.
const HEAD_CHARS: usize = 160;
/// Marque d'un résultat déjà compacté (on ne le refait pas).
const MARK: &str = "[résultat compacté";

/// Taille du contexte : textes et arguments d'appels d'outils.
pub fn context_chars(messages: &[Message]) -> usize {
    messages
        .iter()
        .map(|m| {
            m.content.chars().count()
                + m.tool_calls
                    .iter()
                    .flatten()
                    .map(|call| call.arguments.to_string().chars().count())
                    .sum::<usize>()
        })
        .sum()
}

/// Ramène le contexte sous `budget` en compactant le passé. Renvoie le
/// nombre d'éléments réduits (0 : rien à faire ou rien de compactable).
pub fn fit(messages: &mut [Message], budget: usize, keep_rounds: usize) -> usize {
    let mut total = context_chars(messages);
    if total <= budget {
        return 0;
    }
    // Frontière : le début des `keep_rounds` derniers tours d'outils (un tour
    // = un message de l'assistant qui appelle des outils).
    let rounds: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter(|(_, m)| m.role == Role::Assistant && m.tool_calls.as_ref().is_some_and(|c| !c.is_empty()))
        .map(|(i, _)| i)
        .collect();
    let frontier = if rounds.len() > keep_rounds { rounds[rounds.len() - keep_rounds] } else { return 0 };

    let mut reduced = 0usize;
    // 1. Les vieux résultats d'outils, du plus ancien au plus récent.
    for message in messages[..frontier].iter_mut() {
        if total <= budget {
            return reduced;
        }
        let size = message.content.chars().count();
        if message.role != Role::Tool || size < MIN_COMPACT_CHARS || message.content.starts_with(MARK) {
            continue;
        }
        let head: String = message.content.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(HEAD_CHARS).collect();
        message.content = format!(
            "{MARK} pour tenir dans le contexte : {size} caractères à l'origine. Début : « {head}… ». \
             Relance l'outil si tu as besoin du détail.]"
        );
        total = total - size + message.content.chars().count();
        reduced += 1;
    }
    // 2. Les gros arguments des vieux appels (contenu écrit, scripts).
    for message in messages[..frontier].iter_mut() {
        if total <= budget {
            return reduced;
        }
        for call in message.tool_calls.iter_mut().flatten() {
            let Some(object) = call.arguments.as_object_mut() else { continue };
            for value in object.values_mut() {
                let Some(text) = value.as_str() else { continue };
                let size = text.chars().count();
                if size < MIN_COMPACT_CHARS {
                    continue;
                }
                let stub = format!("[argument compacté : {size} caractères, déjà transmis]");
                // Taille sérialisée (guillemets, échappements) : on la recalcule.
                let before = value.to_string().chars().count();
                *value = serde_json::Value::String(stub);
                total = total - before + value.to_string().chars().count();
                reduced += 1;
            }
        }
    }
    reduced
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::ToolCall;

    /// Une tâche longue : `rounds` tours, chacun écrit un fichier de 5 000
    /// caractères puis lit une sortie de 12 000 (le plafond d'un outil).
    fn longue_tache(rounds: usize) -> Vec<Message> {
        let mut messages = vec![Message::system("système"), Message::user("fais le travail")];
        for i in 0..rounds {
            let mut assistant = Message::assistant(format!("étape {i}"));
            assistant.tool_calls = Some(vec![ToolCall {
                id: format!("c{i}"),
                name: "write_file".into(),
                arguments: serde_json::json!({ "path": format!("f{i}.txt"), "content": "x".repeat(5_000) }),
            }]);
            messages.push(assistant);
            messages.push(Message::tool_result(format!("c{i}"), "write_file", format!("sortie {i} ") + &"y".repeat(12_000)));
        }
        messages
    }

    #[test]
    fn une_longue_tache_reste_dans_le_budget() {
        let mut messages = longue_tache(30);
        assert!(context_chars(&messages) > 400_000, "le scénario doit déborder");
        let reduced = fit(&mut messages, BUDGET_CHARS, KEEP_ROUNDS);
        assert!(reduced > 0);
        assert!(context_chars(&messages) <= BUDGET_CHARS, "{}", context_chars(&messages));
        // Chaque appel garde un résultat avec son identifiant (sinon HTTP 400).
        for i in 0..30 {
            assert!(messages.iter().any(|m| m.tool_call_id.as_deref() == Some(&format!("c{i}"))));
        }
        // Le plus ancien dit ce qu'il était et comment le retrouver…
        let first = messages.iter().find(|m| m.tool_call_id.as_deref() == Some("c0")).unwrap();
        assert!(first.content.contains("12009 caractères") && first.content.contains("Relance l'outil"), "{}", first.content);
        assert!(first.content.contains("sortie 0"), "{}", first.content);
        // …et les deux derniers tours sont intacts.
        let last = messages.last().unwrap();
        assert!(last.content.len() > 12_000);
        let avant_dernier = &messages[messages.len() - 3];
        assert!(avant_dernier.content.len() > 12_000);
    }

    #[test]
    fn sous_le_budget_rien_ne_change() {
        let mut messages = longue_tache(3);
        let avant = context_chars(&messages);
        assert_eq!(fit(&mut messages, BUDGET_CHARS, KEEP_ROUNDS), 0);
        assert_eq!(context_chars(&messages), avant);
    }

    #[test]
    fn les_gros_arguments_sont_compactes_si_les_resultats_ne_suffisent_pas() {
        // Résultats courts, arguments énormes : seule la passe 2 peut aider.
        let mut messages = longue_tache(12);
        for m in messages.iter_mut().filter(|m| m.role == Role::Tool) {
            m.content = "ok".into();
        }
        assert!(context_chars(&messages) > 50_000);
        fit(&mut messages, 20_000, KEEP_ROUNDS);
        assert!(context_chars(&messages) <= 20_000, "{}", context_chars(&messages));
        let first_args = messages[2].tool_calls.as_ref().unwrap()[0].arguments.to_string();
        assert!(first_args.contains("argument compacté") && first_args.contains("f0.txt"), "{first_args}");
    }
}
