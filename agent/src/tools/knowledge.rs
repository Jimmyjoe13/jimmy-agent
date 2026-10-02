//! Outils de connaissance : mémoire locale de Jimmy et Synaptiq.

use super::{arg_str, arg_u64, schema, BoxFuture, Tool, ToolContext, MAX_TOOL_OUTPUT};
use crate::error::{Error, Result};
use crate::memory::MemoryKind;
use crate::permissions::Capability;

fn memory_kind(args: &serde_json::Value, default: MemoryKind) -> MemoryKind {
    match arg_str(args, "kind").as_deref() {
        Some("semantic") => MemoryKind::Semantic,
        Some("procedural") => MemoryKind::Procedural,
        Some("episodic") => MemoryKind::Episodic,
        _ => default,
    }
}

pub struct SearchMemory;

impl Tool for SearchMemory {
    fn name(&self) -> &str {
        "search_memory"
    }
    fn description(&self) -> &str {
        "Interroge la mémoire personnelle de Jimmy (préférences, projets, habitudes apprises à partir des conversations)."
    }
    fn parameters(&self) -> serde_json::Value {
        schema(
            serde_json::json!({
                "query": {"type": "string", "description": "Ce que tu cherches."},
                "limit": {"type": "integer", "description": "Nombre de résultats (défaut 6)."}
            }),
            &["query"],
        )
    }
    fn capability(&self) -> Capability {
        Capability::Read
    }
    fn call<'a>(
        &'a self,
        args: &'a serde_json::Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            let query = arg_str(args, "query").ok_or_else(|| Error::Tool("« query » manquant".into()))?;
            let limit = arg_u64(args, "limit", 6).min(20) as usize;
            let hits = ctx.memory.recall(&query, limit)?;
            if hits.is_empty() {
                return Ok("Rien dans la mémoire pour cette requête.".into());
            }
            Ok(hits
                .iter()
                .map(|hit| format!("- ({:.2}) [{}] {}", hit.score, hit.memory.kind.as_str(), hit.memory.content))
                .take(MAX_TOOL_OUTPUT)
                .collect::<Vec<_>>()
                .join("\n"))
        })
    }
}

pub struct Remember;

impl Tool for Remember {
    fn name(&self) -> &str {
        "remember"
    }
    fn description(&self) -> &str {
        "Enregistre durablement un fait, une préférence, une règle ou une leçon apprise. À n'utiliser que si l'information est stable et utile plus tard."
    }
    fn parameters(&self) -> serde_json::Value {
        schema(
            serde_json::json!({
                "content": {"type": "string", "description": "Le souvenir, formulé à la troisième personne, ex. « L'utilisateur préfère des réponses courtes »."},
                "kind": {"type": "string", "description": "semantic | procedural | episodic. Défaut : semantic."},
                "importance": {"type": "number", "description": "Importance entre 0 et 1 (défaut 0.6)."}
            }),
            &["content"],
        )
    }
    fn capability(&self) -> Capability {
        Capability::Write
    }
    fn call<'a>(
        &'a self,
        args: &'a serde_json::Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            let content = arg_str(args, "content").ok_or_else(|| Error::Tool("« content » manquant".into()))?;
            let kind = memory_kind(args, MemoryKind::Semantic);
            let importance = args
                .get("importance")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.6) as f32;
            ctx.check(Capability::Write, "mémoire")?;
            let id = ctx.memory.remember(kind, &content, "conversation", importance)?;
            Ok(format!("mémorisé ({}), identifiant {id}", kind.as_str()))
        })
    }
}

// ── Synaptiq ─────────────────────────────────────────────────────────────────

fn require_synaptiq(ctx: &ToolContext) -> Result<&std::sync::Arc<crate::synaptiq::SynaptiqClient>> {
    ctx.synaptiq
        .as_ref()
        .ok_or_else(|| Error::Tool("Synaptiq n'est pas configuré".into()))
}

pub struct SynaptiqSearch;

impl Tool for SynaptiqSearch {
    fn name(&self) -> &str {
        "synaptiq_search"
    }
    fn description(&self) -> &str {
        "Interroge Synaptiq, le moteur local de réflexion : souvenirs, règles et décisions enregistrées lors de tâches précédentes. À utiliser quand la demande fait référence à un contexte antérieur."
    }
    fn parameters(&self) -> serde_json::Value {
        schema(
            serde_json::json!({
                "query": {"type": "string", "description": "Sujet ou mot-clé à rechercher."},
                "limit": {"type": "integer", "description": "Nombre de résultats (défaut 6)."}
            }),
            &["query"],
        )
    }
    fn capability(&self) -> Capability {
        Capability::Network
    }
    fn call<'a>(
        &'a self,
        args: &'a serde_json::Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            let client = require_synaptiq(ctx)?;
            let query = arg_str(args, "query").ok_or_else(|| Error::Tool("« query » manquant".into()))?;
            let limit = arg_u64(args, "limit", 6).min(20) as usize;
            client.retrieve(&query, limit).await
        })
    }
}

pub struct SynaptiqRemember;

impl Tool for SynaptiqRemember {
    fn name(&self) -> &str {
        "synaptiq_remember"
    }
    fn description(&self) -> &str {
        "Enregistre une leçon, une règle ou un résultat dans Synaptiq, afin qu'une tâche similaire soit plus rapide la prochaine fois."
    }
    fn parameters(&self) -> serde_json::Value {
        schema(
            serde_json::json!({
                "content": {"type": "string", "description": "Le fait, la règle ou le résultat à retenir."},
                "type": {"type": "string", "description": "semantic | procedural | episodic. Défaut : procedural."},
                "collection": {"type": "string", "description": "Nom du rayon, ex. « preference » ou « rule »."}
            }),
            &["content"],
        )
    }
    fn capability(&self) -> Capability {
        Capability::Network
    }
    fn call<'a>(
        &'a self,
        args: &'a serde_json::Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            let client = require_synaptiq(ctx)?;
            let content = arg_str(args, "content").ok_or_else(|| Error::Tool("« content » manquant".into()))?;
            let memory_type = arg_str(args, "type").unwrap_or_else(|| "procedural".into());
            let collection = arg_str(args, "collection");
            client.remember(&content, &memory_type, collection.as_deref()).await
        })
    }
}