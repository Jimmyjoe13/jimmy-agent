//! Outils de connaissance : mémoire locale de Jimmy et vault Obsidian.

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
        "Interroge la mémoire personnelle de Jimy (préférences, projets, habitudes apprises à partir des conversations)."
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
            let hits = ctx.memory.recall_semantic(&query, limit).await?;
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
            // Mémoire interne : seule la capacité compte, « mémoire » n'est pas un chemin.
            ctx.check_granted(Capability::Write, "mémoire")?;
            let id = ctx.memory.remember_indexed(kind, &content, "conversation", importance).await?;
            Ok(format!("mémorisé ({}), identifiant {id}", kind.as_str()))
        })
    }
}

// ── Vault Obsidian ───────────────────────────────────────────────────────────

fn require_vault(ctx: &ToolContext) -> Result<&std::sync::Arc<crate::memory::vault::Vault>> {
    ctx.vault
        .as_ref()
        .ok_or_else(|| Error::Tool("Vault Obsidian indisponible (chemin absent ou inexistant)".into()))
}

pub struct VaultSearch;

impl Tool for VaultSearch {
    fn name(&self) -> &str {
        "vault_search"
    }
    fn description(&self) -> &str {
        "Recherche en plein texte dans le vault Obsidian de l'utilisateur, partagé avec d'autres agents (notes, projets, journaux d'agents, souvenirs de Jimy). Renvoie les extraits les plus proches avec leur note et son origine (« ta note » ou « partagée »)."
    }
    fn parameters(&self) -> serde_json::Value {
        schema(
            serde_json::json!({
                "query": {"type": "string", "description": "Sujet ou mots-clés à rechercher."},
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
            let vault = require_vault(ctx)?;
            let query = arg_str(args, "query").ok_or_else(|| Error::Tool("« query » manquant".into()))?;
            let limit = arg_u64(args, "limit", 6).min(20) as usize;
            let hits = vault.search(&query, limit).await;
            if hits.is_empty() {
                return Ok("Rien dans le vault pour cette requête.".into());
            }
            // Origine de chaque note : le vault est partagé avec d'autres agents.
            Ok(hits
                .iter()
                .map(|hit| {
                    let origin = if vault.is_own(&hit.path) { "ta note" } else { "partagée" };
                    format!("- {} [{origin} : {}] — {}", hit.title, hit.path, hit.snippet)
                })
                .take(MAX_TOOL_OUTPUT)
                .collect::<Vec<_>>()
                .join("\n"))
        })
    }
}

pub struct VaultRead;

impl Tool for VaultRead {
    fn name(&self) -> &str {
        "vault_read"
    }
    fn description(&self) -> &str {
        "Lit une note du vault Obsidian en entier, d'après le chemin renvoyé par vault_search."
    }
    fn parameters(&self) -> serde_json::Value {
        schema(
            serde_json::json!({
                "path": {"type": "string", "description": "Chemin de la note relatif au vault, ex. « 1_Projets\\jimmy\\PLAN.md »."}
            }),
            &["path"],
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
            let vault = require_vault(ctx)?;
            let chemin = arg_str(args, "path")
                .ok_or_else(|| Error::Tool("« path » manquant".into()))?;
            let contenu = vault.read(&chemin).await?;
            if contenu.trim().is_empty() {
                return Ok("(note vide)".into());
            }
            Ok(contenu)
        })
    }
}

pub struct VaultWrite;

impl Tool for VaultWrite {
    fn name(&self) -> &str {
        "vault_write"
    }
    fn description(&self) -> &str {
        "Capture un souvenir durable dans l'inbox de Jimy (vault Obsidian) ; il sera rangé automatiquement dans la bonne note PARA. Une ou deux phrases complètes, compréhensibles seules (qui, quoi, quel projet). À n'utiliser que si l'information est stable et utile plus tard."
    }
    fn parameters(&self) -> serde_json::Value {
        schema(
            serde_json::json!({
                "content": {"type": "string", "description": "Le souvenir, formulé à la troisième personne."},
                "kind": {"type": "string", "description": "semantic | procedural | episodic. Défaut : semantic."}
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
            let vault = require_vault(ctx)?;
            let content = arg_str(args, "content").ok_or_else(|| Error::Tool("« content » manquant".into()))?;
            let kind = memory_kind(args, MemoryKind::Semantic);
            // Inbox du vault configuré : peut être hors des chemins autorisés
            // (C:\Obsidian), seule la capacité d'écriture compte.
            ctx.check_granted(Capability::Write, "vault obsidian")?;
            let chemin = vault.remember(kind.as_str(), &content).await?;
            Ok(format!("capturé dans l'inbox du vault : {}", chemin.display()))
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::permissions::Permissions;
    use std::sync::{Arc, Mutex, RwLock};

    /// Contexte d'outil identique à celui de l'agent : permissions par défaut
    /// (chemins limités à `C:\Users\**` et `C:\Dev\**`).
    fn contexte(vault: Option<Arc<crate::memory::vault::Vault>>) -> ToolContext {
        let db = Arc::new(Mutex::new(crate::db::Db::open_in_memory().expect("db")));
        let skills_dir = std::env::temp_dir().join(format!("knowledge-skills-{}", uuid::Uuid::new_v4()));
        ToolContext {
            workspace: std::env::temp_dir(),
            permissions: Arc::new(RwLock::new(Permissions::default())),
            memory: Arc::new(crate::memory::MemoryStore::new(db)),
            skills: Arc::new(crate::skills::SkillStore::new(skills_dir).unwrap()),
            vault,
        }
    }

    /// Régression : « mémoire » n'est pas un chemin, la liste de chemins
    /// autorisés le refusait à chaque appel (« chemin non autorisé : mémoire »).
    #[tokio::test]
    async fn remember_passe_avec_les_permissions_par_defaut() {
        let ctx = contexte(None);
        let args = serde_json::json!({"content": "Jimmy boit son café sans sucre."});
        let sortie = Remember.call(&args, &ctx).await.expect("remember refusé");
        assert!(sortie.starts_with("mémorisé"), "{sortie}");
    }

    /// Même bug pour le vault ; et une écriture coupée reste refusée.
    #[tokio::test]
    async fn vault_write_passe_et_respecte_l_ecriture_coupee() {
        let racine = std::env::temp_dir().join(format!("knowledge-vault-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&racine).unwrap();
        let vault = crate::memory::vault::Vault::open(racine.to_str().unwrap(), "0_Inbox/Jimmy").expect("vault");
        let ctx = contexte(Some(Arc::new(vault)));
        let args = serde_json::json!({"content": "Le vault de test accepte une capture."});
        let sortie = VaultWrite.call(&args, &ctx).await.expect("vault_write refusé");
        assert!(sortie.contains("capturé"), "{sortie}");

        ctx.permissions.write().unwrap().write.granted = false;
        assert!(VaultWrite.call(&args, &ctx).await.is_err());
        assert!(Remember.call(&args, &ctx).await.is_err());
        let _ = std::fs::remove_dir_all(&racine);
    }
}
