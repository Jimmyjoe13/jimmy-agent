//! Outils de skills : consulter, créer, améliorer.

use super::{arg_str, schema, BoxFuture, Tool, ToolContext, MAX_TOOL_OUTPUT};
use crate::error::{Error, Result};
use crate::permissions::Capability;

pub struct ListSkills;

impl Tool for ListSkills {
    fn name(&self) -> &str {
        "list_skills"
    }
    fn description(&self) -> &str {
        "Liste les skills dont Jimmy dispose, avec leur description. Utile avant d'en créer un nouveau."
    }
    fn parameters(&self) -> serde_json::Value {
        schema(serde_json::json!({}), &[])
    }
    fn capability(&self) -> Capability {
        Capability::Read
    }
    fn call<'a>(
        &'a self,
        _args: &'a serde_json::Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            let skills = ctx.skills.list()?;
            if skills.is_empty() {
                return Ok("Aucun skill pour l'instant.".into());
            }
            Ok(skills
                .iter()
                .map(|s| format!("- {} : {}", s.name, s.description))
                .take(MAX_TOOL_OUTPUT)
                .collect::<Vec<_>>()
                .join("\n"))
        })
    }
}

pub struct ReadSkill;

impl Tool for ReadSkill {
    fn name(&self) -> &str {
        "read_skill"
    }
    fn description(&self) -> &str {
        "Charge le contenu complet d'un skill pour l'appliquer à la tâche en cours."
    }
    fn parameters(&self) -> serde_json::Value {
        schema(
            serde_json::json!({ "name": {"type": "string", "description": "Nom du skill."}}),
            &["name"],
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
            let name = arg_str(args, "name").ok_or_else(|| Error::Tool("« name » manquant".into()))?;
            let skill = ctx.skills.load(&name)?;
            Ok(skill.body)
        })
    }
}

pub struct CreateSkill;

impl Tool for CreateSkill {
    fn name(&self) -> &str {
        "create_skill"
    }
    fn description(&self) -> &str {
        "Crée un skill réutilisable lorsqu'un même besoin revient. La description doit dire en une phrase QUAND l'utiliser ; le corps doit contenir les étapes concrètes."
    }
    fn parameters(&self) -> serde_json::Value {
        schema(
            serde_json::json!({
                "name": {"type": "string", "description": "Nom court et explicite, ex. « deployer-le-site »."},
                "description": {"type": "string", "description": "Une phrase expliquant quand utiliser ce skill."},
                "body": {"type": "string", "description": "Les instructions, en étapes numérotées."}
            }),
            &["name", "description", "body"],
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
            let name = arg_str(args, "name").ok_or_else(|| Error::Tool("« name » manquant".into()))?;
            let description =
                arg_str(args, "description").ok_or_else(|| Error::Tool("« description » manquant".into()))?;
            let body = arg_str(args, "body").ok_or_else(|| Error::Tool("« body » manquant".into()))?;
            let dir = ctx.skills.skill_dir(&name);
            ctx.check(Capability::Write, &dir.to_string_lossy())?;
            let written = ctx.skills.write(&name, &description, &body)?;
            Ok(format!("skill créé dans {}", written.display()))
        })
    }
}

pub struct UpdateSkill;

impl Tool for UpdateSkill {
    fn name(&self) -> &str {
        "update_skill"
    }
    fn description(&self) -> &str {
        "Réécrit le corps d'un skill existant lorsqu'il s'avère incomplet, obsolète ou trop long. Conserve sa description."
    }
    fn parameters(&self) -> serde_json::Value {
        schema(
            serde_json::json!({
                "name": {"type": "string", "description": "Nom du skill à améliorer."},
                "body": {"type": "string", "description": "Nouveau corps complet du skill."}
            }),
            &["name", "body"],
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
            let name = arg_str(args, "name").ok_or_else(|| Error::Tool("« name » manquant".into()))?;
            let body = arg_str(args, "body").ok_or_else(|| Error::Tool("« body » manquant".into()))?;
            let dir = ctx.skills.skill_dir(&name);
            ctx.check(Capability::Write, &dir.to_string_lossy())?;
            let written = ctx.skills.update_body(&name, &body)?;
            Ok(format!("skill mis à jour dans {}", written.display()))
        })
    }
}