//! Construction du prompt système.
//!
//! Le prompt n'est pas un bloc figé : il est assemblé à chaque tour à partir de
//! ce que Jimmy sait *au moment précis* de la demande — identité, outils
//! disponibles, skills catalogue, souvenirs pertinents, contexte Synaptiq.
//!
//! Assembler à la demande (et non une fois au démarrage) évite deux pièges :
//! un prompt qui devient faux après une modification des paramètres, et un
//! contexte inutile qui coûte des tokens à chaque échange.

use std::sync::Arc;

use crate::config::Settings;
use crate::memory::MemoryStore;
use crate::skills::SkillStore;
use crate::tools::ToolRegistry;

pub const IDENTITY: &str = r#"Tu es Jimmy, un assistant personnel agentique qui vit sur le bureau de l'utilisateur.

## Qui tu es
Tu n'es pas un simple chatbot : tu as un caractère. Ton avatar est un renard
humanoïde qui réagit à ce que tu fais. Ton nom est Jimmy ; « Jimmy » est aussi
ce qu'on t'appelle pour t'activer à la voix.

## Ce que tu sais de toi
Tu tournes en local sur la machine de l'utilisateur. Tu as accès à ses fichiers,
à ses commandes, au web, à une mémoire personnelle et à Synaptiq, un moteur de
réflexion qui conserve le contexte des tâches précédentes. Aucun serveur Jimmy
n'existe : c'est un programme personnel.

## Comment travailler
1. Une demande = un objectif. Decompose-le, puis agis : appelle des outils
   plutôt que de supposer.
2. Utilise un outil dès que la réponse dépend d'une information que tu n'as pas :
   ne devine jamais le contenu d'un fichier, d'une commande ou d'une page.
3. Après chaque appel d'outil, lis réellement le résultat avant de continuer.
   Une erreur se corrige en corrigeant, pas en expliquant.
4. Enchaîne autant d'étapes qu'il le faut. Tu disposes d'un budget d'itérations
   par demande : s'il est presque épuisé, livre le meilleur résultat partiel
   possible et dis clairement ce qui reste à faire.
5. Réponds en français, sauf indication contraire. Sois direct : pas de
   préambule, pas de reformulation de la question.

## Mémoire et skills
- Avant d'inventer une préférence de l'utilisateur, cherche-la dans
  `search_memory`. Après avoir appris un fait stable, enregistre-le avec
  `remember`.
- Si la demande reprend un contexte antérieur, `synaptiq_search` peut ramener
  une décision ou une leçon déjà prise : n'impose pas de refaire l'erreur.
- Si un même besoin revient, crée un skill avec `create_skill` : c'est mieux
  qu'une longue conversation.
- Un skill ne te donne aucun droit supplémentaire : les permissions de
  l'utilisateur s'appliquent à toutes les actions, y compris celles décrites
  dans un skill.

## Voix
Ta réponse est lue à voix haute. Écris donc pour l'oral : une idée par phrase,
pas de symboles, pas de tableau, pas de parenthèses. Le texte affiché peut être
plus détaillé que ce qui est dit.
"#;

pub struct PromptContext<'a> {
    pub settings: &'a Settings,
    pub memory: &'a MemoryStore,
    pub skills: &'a SkillStore,
    pub registry: &'a ToolRegistry,
    pub request: &'a str,
    pub memory_block: Option<String>,
    pub synaptiq_block: Option<String>,
}

pub fn build_system(ctx: &PromptContext<'_>) -> String {
    let mut parts: Vec<String> = vec![IDENTITY.to_string()];

    parts.push(format!(
        "## Contexte de la session\n- Dossier de travail par défaut : {}\n- Modèle : {}",
        ctx.settings.workspace, ctx.settings.llm.model
    ));

    parts.push(format!(
        "## Outils disponibles ({})\n{}",
        ctx.registry.len(),
        ctx.registry
            .names()
            .iter()
            .map(|n| format!("- {n}"))
            .collect::<Vec<_>>()
            .join("\n")
    ));

    if !ctx.skills.list().map(|s| s.is_empty()).unwrap_or(true) {
        parts.push(ctx.skills.catalogue());
    }

    if let Some(block) = &ctx.synaptiq_block {
        if !block.trim().is_empty() {
            parts.push(block.clone());
        }
    }

    if let Some(block) = &ctx.memory_block {
        if !block.trim().is_empty() {
            parts.push(block.clone());
        }
    }

    // Rappel des skills les plus proches de la demande : gain de temps réel,
    // le modèle n'a plus à deciding which ones to read.
    let suggestions = ctx.skills.suggest(ctx.request, 3);
    if !suggestions.is_empty() {
        parts.push(format!(
            "Skills probablement utiles pour cette demande : {}",
            suggestions
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    parts.join("\n\n")
}

/// Message utilisateur enrichi des souvenirs, quand le rappel doit être visible
/// de l'agent plutôt que du contexte système.
pub async fn recall_for(memory: &MemoryStore, settings: &Settings, request: &str) -> Option<String> {
    if !settings.memory.enabled {
        return None;
    }
    let block = memory
        .context_block(request, settings.memory.recall_limit as usize)
        .await
        .ok()?;
    (!block.trim().is_empty()).then_some(block)
}

/// Contexte Synaptiq à injecter, si la règle de déclenchement le justifie.
pub async fn synaptiq_context(
    client: Option<&Arc<crate::synaptiq::SynaptiqClient>>,
    settings: &Settings,
    request: &str,
) -> Option<String> {
    if !settings.synaptiq.enabled {
        return None;
    }
    let client = client?;
    if !client.has_key() {
        return None;
    }
    let markers = crate::synaptiq::has_context_markers(request);
    let decision = crate::synaptiq::should_consult(request, settings.synaptiq.min_request_chars, markers);
    if decision == crate::synaptiq::Usefulness::No {
        log::debug!("[synaptiq] non consulté (demande courte, sans marqueur de contexte)");
        return None;
    }
    match client.build_context(request, request, 1200).await {
        Ok(block) if !block.trim().is_empty() => Some(block),
        Ok(_) => None,
        Err(error) => {
            // Synaptiq est un complément : son absence ne doit jamais faire
            // échouer la demande.
            log::warn!("[synaptiq] contexte indisponible : {error}");
            None
        }
    }
}