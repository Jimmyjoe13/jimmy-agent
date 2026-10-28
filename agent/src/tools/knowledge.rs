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
            // Souvenirs remis au modèle : ils comptent comme utilisés.
            let ids: Vec<String> = hits.iter().map(|hit| hit.memory.id.clone()).collect();
            if let Err(e) = ctx.memory.mark_used(&ids) {
                log::warn!("[memory] compteur d'usage non mis à jour : {e}");
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

// ── Sessions et Projets (Lots B et C) ────────────────────────────────────────

pub struct SearchSessions;

impl Tool for SearchSessions {
    fn name(&self) -> &str {
        "search_sessions"
    }
    fn description(&self) -> &str {
        "Cherche dans les conversations passées de Jimy (titres, projets, résumés et texte des échanges) pour retrouver ce qui a été dit ou fait sur un sujet. Sans « query » : les conversations les plus récentes avec leur date, pour résumer une journée."
    }
    fn parameters(&self) -> serde_json::Value {
        schema(
            serde_json::json!({
                "query": {"type": "string", "description": "Mots-clés ou sujet à chercher. Vide ou absent : conversations les plus récentes."},
                "limit": {"type": "integer", "description": "Nombre maximal de conversations (défaut 5)."}
            }),
            // `query` facultatif : un résumé de journée n'a pas de mot-clé (piège 143).
            &[],
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
            ctx.check_granted(Capability::Read, "historique des conversations")?;
            let history = ctx
                .history
                .as_ref()
                .ok_or_else(|| Error::Tool("historique indisponible".into()))?;
            // Sans mot-clé, `search_sessions` liste par date de mise à jour.
            let query = arg_str(args, "query");
            let limit = arg_u64(args, "limit", 5).min(15) as usize;
            let found = history.search_sessions(query.as_deref(), false, limit)?;
            if found.is_empty() {
                return Ok("Aucune conversation trouvée pour cette recherche.".into());
            }
            let mut out = Vec::new();
            for s in found {
                let project = s.project.as_deref().unwrap_or("sans projet");
                let recap = history.recap(&s.id).ok().map(|r| r.summary).unwrap_or_default();
                let msgs = history.conversation(&s.id, 4).unwrap_or_default();
                let tail: String = msgs
                    .iter()
                    .map(|m| {
                        let snippet: String = m.content.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(160).collect();
                        format!("  {} : {snippet}", m.role.as_str())
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                let mut block = format!("- « {} » [id={}, projet={}, {} message(s), màj {}]", s.title, s.id, project, s.message_count, s.updated_at);
                if !recap.trim().is_empty() {
                    let r: String = recap.chars().take(300).collect();
                    block.push_str(&format!("\n  Résumé : {r}"));
                }
                if !tail.is_empty() {
                    block.push_str(&format!("\n{tail}"));
                }
                out.push(block);
            }
            Ok(super::clip(&out.join("\n\n"), MAX_TOOL_OUTPUT))
        })
    }
}

pub struct SetProject;

impl Tool for SetProject {
    fn name(&self) -> &str {
        "set_project"
    }
    fn description(&self) -> &str {
        "Rattache la conversation courante à un projet (dossier sur le disque), notamment quand l'utilisateur dit à la voix « on bosse sur X » ou « passe sur le projet Y ». Accepte un nom de projet approximatif ou un chemin."
    }
    fn parameters(&self) -> serde_json::Value {
        schema(
            serde_json::json!({
                "project": {"type": "string", "description": "Nom du projet (ex. « jimmy-agent », « synaptiq ») ou chemin complet du dossier."}
            }),
            &["project"],
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
            let query = arg_str(args, "project").ok_or_else(|| Error::Tool("« project » manquant".into()))?;
            let history = ctx
                .history
                .as_ref()
                .ok_or_else(|| Error::Tool("historique indisponible".into()))?;
            let session_id = ctx
                .session_id
                .as_deref()
                .ok_or_else(|| Error::Tool("aucune conversation active".into()))?;

            // 1. Chemin direct existant ?
            let direct = std::path::PathBuf::from(&query);
            let chosen = if direct.is_dir() {
                Some(direct)
            } else {
                // 2. Cherche parmi les projets récents de l'historique et les sous-dossiers de `workspace`.
                let needle = crate::memory::embed::normalize(&query);
                let mut candidates: Vec<std::path::PathBuf> = Vec::new();
                if let Ok(recents) = history.recent_projects(30) {
                    for p in recents {
                        let pb = std::path::PathBuf::from(p);
                        if pb.is_dir() && !candidates.contains(&pb) {
                            candidates.push(pb);
                        }
                    }
                }
                if let Ok(entries) = std::fs::read_dir(&ctx.workspace) {
                    for entry in entries.flatten() {
                        let p = entry.path();
                        if p.is_dir() && !candidates.contains(&p) {
                            candidates.push(p);
                        }
                    }
                }
                if let Some(parent) = ctx.workspace.parent() {
                    if let Ok(entries) = std::fs::read_dir(parent) {
                        for entry in entries.flatten() {
                            let p = entry.path();
                            if p.is_dir() && !candidates.contains(&p) {
                                candidates.push(p);
                            }
                        }
                    }
                }
                candidates.into_iter().find(|p| {
                    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    let norm = crate::memory::embed::normalize(name);
                    !norm.is_empty() && (norm == needle || norm.contains(&needle) || needle.contains(&norm))
                })
            };

            let Some(folder) = chosen else {
                return Err(Error::Tool(format!(
                    "Projet « {query} » introuvable parmi les projets récents et le dossier de travail ({}).",
                    ctx.workspace.display()
                )));
            };
            let folder_str = folder.to_string_lossy().to_string();
            ctx.check(Capability::Read, &folder_str)?;
            history.set_project(session_id, Some(&folder_str))?;
            let extra = crate::core::prompt::read_project_instructions(&folder_str)
                .map(|ins| format!("\n\n{ins}"))
                .unwrap_or_default();
            Ok(format!("Projet de la conversation réglé sur : {folder_str}{extra}"))
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
        let mut perms = Permissions::default();
        perms.write.granted = true;
        perms.read.granted = true;
        ToolContext::with_empty_images(
            std::env::temp_dir(),
            Arc::new(RwLock::new(perms)),
            Arc::new(crate::memory::MemoryStore::new(db)),
            Arc::new(crate::skills::SkillStore::new(skills_dir).unwrap()),
            vault,
        )
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

    #[tokio::test]
    async fn search_sessions_et_set_project_fonctionnent() {
        let racine = std::env::temp_dir().join(format!("knowledge-proj-{}", uuid::Uuid::new_v4()));
        let proj = racine.join("Maxence_Leads");
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(proj.join("AGENTS.md"), "# Consignes Maxence\nRelancer à J+3.").unwrap();

        let db = Arc::new(Mutex::new(crate::db::Db::open_in_memory().expect("db")));
        let history = Arc::new(crate::core::history::History::new(db.clone()));
        let s1 = history.create_session("Relance des leads Maxence").unwrap();
        history.append(&s1, &crate::core::types::Message::user("On prépare les 3 brouillons Maxence")).unwrap();

        let s2 = history.create_session("Session vocale").unwrap();
        let skills_dir = racine.join("skills");
        let mut perms = Permissions::default();
        perms.read.granted = true;
        perms.read.allow_paths = vec![format!("{}/**", racine.to_string_lossy().replace('\\', "/"))];
        let ctx = ToolContext::with_empty_images(
            racine.clone(),
            Arc::new(RwLock::new(perms)),
            Arc::new(crate::memory::MemoryStore::new(db)),
            Arc::new(crate::skills::SkillStore::new(skills_dir).unwrap()),
            None,
        )
        .with_session(history.clone(), s2.clone());

        let found = SearchSessions
            .call(&serde_json::json!({"query": "brouillons Maxence"}), &ctx)
            .await
            .unwrap();
        assert!(found.contains("Relance des leads Maxence"), "{found}");

        let switched = SetProject
            .call(&serde_json::json!({"project": "maxence"}), &ctx)
            .await
            .unwrap();
        assert!(switched.contains("Maxence_Leads"), "{switched}");
        assert!(switched.contains("Consignes Maxence"), "{switched}");
        assert_eq!(history.project(&s2).unwrap().as_deref(), Some(proj.to_string_lossy().as_ref()));

        let _ = std::fs::remove_dir_all(&racine);
    }
}
        // Sans mot-clé : les conversations les plus récentes (résumer une
        // journée, piège 143) au lieu d'une erreur « query manquant ».
        let recent = SearchSessions.call(&serde_json::json!({}), &ctx).await.unwrap();
        assert!(recent.contains("Relance des leads Maxence") && recent.contains("Session vocale"), "{recent}");

