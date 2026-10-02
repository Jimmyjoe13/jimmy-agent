//! Outils de fichiers : lire, écrire, lister, chercher.

use std::fs;

use super::{arg_str, arg_u64, clip, schema, BoxFuture, Tool, ToolContext, MAX_TOOL_OUTPUT};
use crate::error::{Error, Result};
use crate::permissions::Capability;

/// Dossiers systématiquement ignorés lors d'une recherche : le bruit y est
/// massif et Jimmy n'y trouve jamais rien d'utile.
const IGNORED: &[&str] = &[
    "node_modules", "target", ".git", ".godot", "dist", "build", ".venv", "venv", "__pycache__",
    ".next", ".cache", "AppData", "$RECYCLE.BIN", "System Volume Information", ".idea", ".vscode",
];

pub struct ListDirectory;

impl Tool for ListDirectory {
    fn name(&self) -> &str {
        "list_directory"
    }
    fn description(&self) -> &str {
        "Liste le contenu d'un dossier. Utilise cet outil pour découvrir la structure d'un répertoire avant d'aller plus loin."
    }
    fn parameters(&self) -> serde_json::Value {
        schema(
            serde_json::json!({
                "path": {"type": "string", "description": "Chemin du dossier. Absolu ou relatif au dossier de travail."}
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
            let path = arg_str(args, "path").unwrap_or_else(|| ".".into());
            let dir = ctx.resolve(&path);
            ctx.check(Capability::Read, &dir.to_string_lossy())?;
            if !dir.is_dir() {
                return Err(Error::Tool(format!("ce n'est pas un dossier : {}", dir.display())));
            }
            let mut entries: Vec<String> = Vec::new();
            for entry in fs::read_dir(&dir)? {
                let entry = entry?;
                let name = entry.file_name().to_string_lossy().to_string();
                let metadata = entry.metadata().ok();
                let (kind, size) = match metadata {
                    Some(m) if m.is_dir() => ("dossier", 0u64),
                    Some(m) => ("fichier", m.len()),
                    None => ("inconnu", 0),
                };
                entries.push(format!("{kind:8} {size:>10}  {name}"));
            }
            entries.sort();
            Ok(clip(
                &format!("{} ({})\n{}", dir.display(), entries.len(), entries.join("\n")),
                MAX_TOOL_OUTPUT,
            ))
        })
    }
}

pub struct ReadFile;

impl Tool for ReadFile {
    fn name(&self) -> &str {
        "read_file"
    }
    fn description(&self) -> &str {
        "Lit le contenu d'un fichier texte. Les fichiers binaires sont refusés."
    }
    fn parameters(&self) -> serde_json::Value {
        schema(
            serde_json::json!({
                "path": {"type": "string", "description": "Chemin du fichier."}
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
            let path = arg_str(args, "path").ok_or_else(|| Error::Tool("« path » manquant".into()))?;
            let file = ctx.resolve(&path);
            ctx.check(Capability::Read, &file.to_string_lossy())?;
            let bytes = fs::read(&file)?;
            if bytes.contains(&0) {
                return Err(Error::Tool(format!(
                    "{} semble être un fichier binaire",
                    file.display()
                )));
            }
            let text = String::from_utf8_lossy(&bytes);
            Ok(clip(&text, MAX_TOOL_OUTPUT))
        })
    }
}

pub struct WriteFile;

impl Tool for WriteFile {
    fn name(&self) -> &str {
        "write_file"
    }
    fn description(&self) -> &str {
        "Écrit un fichier, en le créant ou en le remplaçant. Utilise cette outil uniquement quand l'utilisateur l'a demandé ou quand la tâche l'exige explicitement."
    }
    fn parameters(&self) -> serde_json::Value {
        schema(
            serde_json::json!({
                "path": {"type": "string", "description": "Chemin du fichier à écrire."},
                "content": {"type": "string", "description": "Contenu complet à écrire."}
            }),
            &["path", "content"],
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
            let path = arg_str(args, "path").ok_or_else(|| Error::Tool("« path » manquant".into()))?;
            let content = args
                .get("content")
                .and_then(|v| v.as_str())
                .ok_or_else(|| Error::Tool("« content » manquant".into()))?;
            let file = ctx.resolve(&path);
            ctx.check(Capability::Write, &file.to_string_lossy())?;
            if let Some(parent) = file.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&file, content)?;
            Ok(format!("écrit {} octets dans {}", content.len(), file.display()))
        })
    }
}

pub struct SearchFiles;

impl Tool for SearchFiles {
    fn name(&self) -> &str {
        "search_files"
    }
    fn description(&self) -> &str {
        "Cherche des fichiers et du texte dans une arborescence. Utilise un motif simple (extension) et une chaîne à trouver."
    }
    fn parameters(&self) -> serde_json::Value {
        schema(
            serde_json::json!({
                "root": {"type": "string", "description": "Racine de la recherche. Par défaut le dossier de travail."},
                "pattern": {"type": "string", "description": "Filtre sur le nom de fichier, ex. « .rs ». Vide = tous."},
                "query": {"type": "string", "description": "Texte à chercher dans le contenu des fichiers texte."},
                "max_results": {"type": "integer", "description": "Nombre maximal de résultats (défaut 50)."}
            }),
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
            let root = ctx.resolve(&arg_str(args, "root").unwrap_or_else(|| ".".into()));
            let pattern = arg_str(args, "pattern").unwrap_or_default().to_lowercase();
            let query = arg_str(args, "query").unwrap_or_default().to_lowercase();
            let max = arg_u64(args, "max_results", 50).min(500) as usize;
            ctx.check(Capability::Read, &root.to_string_lossy())?;
            if !root.is_dir() {
                return Err(Error::Tool(format!("dossier introuvable : {}", root.display())));
            }

            let mut hits: Vec<String> = Vec::new();
            let mut stack = vec![(root.clone(), 0usize)];
            while let Some((dir, depth)) = stack.pop() {
                if depth > 8 || hits.len() >= max {
                    continue;
                }
                let Ok(entries) = fs::read_dir(&dir) else {
                    continue;
                };
                for entry in entries.flatten() {
                    if hits.len() >= max {
                        break;
                    }
                    let name = entry.file_name().to_string_lossy().to_string();
                    if IGNORED.iter().any(|i| name.eq_ignore_ascii_case(i)) {
                        continue;
                    }
                    let path = entry.path();
                    if path.is_dir() {
                        stack.push((path, depth + 1));
                        continue;
                    }
                    let file_name = name.to_lowercase();
                    if !pattern.is_empty() && !file_name.contains(&pattern) {
                        continue;
                    }
                    if !query.is_empty() {
                        let matched = fs::read_to_string(&path)
                            .map(|text| {
                                text.lines()
                                    .filter(|l| l.to_lowercase().contains(&query))
                                    .take(3)
                                    .map(|l| format!("      {}", l.trim().chars().take(160).collect::<String>()))
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();
                        if matched.is_empty() {
                            continue;
                        }
                        hits.push(format!("{}\n{}", path.display(), matched.join("\n")));
                    } else {
                        hits.push(path.display().to_string());
                    }
                }
            }
            hits.sort();
            Ok(clip(
                &format!("{} résultat(s)\n{}", hits.len(), hits.join("\n")),
                MAX_TOOL_OUTPUT,
            ))
        })
    }
}