//! Outils de fichiers : lire, écrire, lister, chercher.

use std::fs;
use std::time::{Duration, Instant};

use super::{arg_str, arg_u64, clip, schema, BoxFuture, Tool, ToolContext, MAX_TOOL_OUTPUT};
use crate::error::{Error, Result};
use crate::permissions::Capability;

/// Dossiers systématiquement ignorés lors d'une recherche : le bruit y est
/// massif et Jimmy n'y trouve jamais rien d'utile.
const IGNORED: &[&str] = &[
    "node_modules", "target", ".git", ".godot", "dist", "build", ".venv", "venv", "__pycache__",
    ".next", ".cache", "AppData", "$RECYCLE.BIN", "System Volume Information", ".idea", ".vscode",
    // Caches d'outils Python : `search_files` y trouvait des doublons
    // (cas réel du 4 octobre : `.pytest_cache` dans chaque recherche).
    ".pytest_cache", ".mypy_cache", ".ruff_cache", ".tox",
];

/// Fenêtre de lecture d'un fichier texte : à partir de `start_line` (1 = début),
/// au plus `max_lines` lignes et `max_chars` caractères, coupée en fin de ligne.
///
/// Avant, `read_file` renvoyait les 12 000 premiers caractères et rien d'autre :
/// sur un fichier de 23 Ko, la fin était invisible et le modèle la cherchait à
/// coups de recherches et de commandes, en brûlant ses étapes (cas réel du
/// 4 octobre, `storage.py`). Un fichier qui tient entier est renvoyé tel quel ;
/// sinon un en-tête dit quelles lignes sont montrées et comment lire la suite.
pub fn read_window(text: &str, start_line: usize, max_lines: Option<usize>, max_chars: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let total = lines.len();
    let start = start_line.max(1);
    if start == 1 && max_lines.is_none() && text.chars().count() <= max_chars {
        return text.to_string();
    }
    if start > total {
        return format!("[le fichier n'a que {total} lignes : rien à partir de la ligne {start}]");
    }
    // Place réservée à l'en-tête.
    let budget = max_chars.saturating_sub(120);
    let mut out = String::new();
    let mut used = 0usize;
    let mut last = start - 1;
    for (i, line) in lines.iter().enumerate().skip(start - 1) {
        if max_lines.is_some_and(|m| i + 1 - start >= m) {
            break;
        }
        let size = line.chars().count() + 1;
        if used + size > budget && last >= start {
            break;
        }
        out.push_str(line);
        out.push('\n');
        used += size;
        last = i + 1;
    }
    let suite = if last < total {
        format!(" · suite : start_line={}", last + 1)
    } else {
        " · fin du fichier".to_string()
    };
    format!("[lignes {start}–{last} sur {total}{suite}]\n{out}")
}

/// Lignes de `text` qui contiennent `query` (déjà en minuscules), préfixées de
/// leur numéro : `read_file` peut ensuite lire autour (`start_line`).
pub fn matching_lines(text: &str, query: &str, max: usize) -> Vec<String> {
    text.lines()
        .enumerate()
        .filter(|(_, l)| l.to_lowercase().contains(query))
        .take(max)
        .map(|(i, l)| format!("      L{}: {}", i + 1, l.trim().chars().take(160).collect::<String>()))
        .collect()
}

/// Budget de temps d'une recherche. Sans borne, un scan du dossier de travail
/// entier a pris **4 min 22 s** (mesuré le 3 octobre 2026, dossier de travail
/// avec 73 projets) : la conversation gelait, l'utilisateur n'entendait rien.
/// Passé le budget, on rend les résultats partiels — c'est au modèle d'affiner
/// sa racine ou son motif.
const SEARCH_BUDGET: Duration = Duration::from_secs(10);

/// Taille maximale d'un fichier dont on lit le contenu pour y chercher une
/// chaîne : au-delà, c'est un journal ou un export, pas une source.
const MAX_INSPECT_BYTES: u64 = 2 * 1024 * 1024;

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
        "Lit un fichier texte. Un gros fichier est renvoyé par morceaux : l'en-tête indique les lignes montrées et le `start_line` de la suite. Pour lire autour d'une ligne trouvée par search_files, passe start_line. Les fichiers binaires sont refusés."
    }
    fn parameters(&self) -> serde_json::Value {
        schema(
            serde_json::json!({
                "path": {"type": "string", "description": "Chemin du fichier."},
                "start_line": {"type": "integer", "description": "Première ligne à lire (1 = début). Par défaut 1."},
                "max_lines": {"type": "integer", "description": "Nombre maximal de lignes. Par défaut : autant que la taille de sortie le permet."}
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
            let start = arg_u64(args, "start_line", 1) as usize;
            let max_lines = args.get("max_lines").and_then(|v| v.as_u64()).map(|n| n.max(1) as usize);
            Ok(read_window(&text, start, max_lines, MAX_TOOL_OUTPUT))
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
                "content": {"type": "string", "description": "Contenu complet à écrire (ou le morceau à ajouter si append=true)."},
                "append": {"type": "boolean", "description": "Ajouter à la fin du fichier au lieu de le remplacer : pour écrire un gros fichier en plusieurs fois."}
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
            // Un gros fichier s'écrit en plusieurs fois : une seule réponse du
            // modèle peut être coupée par sa limite de longueur.
            let append = args.get("append").and_then(|v| v.as_bool()).unwrap_or(false);
            if append {
                use std::io::Write;
                let mut handle = fs::OpenOptions::new().create(true).append(true).open(&file)?;
                handle.write_all(content.as_bytes())?;
                return Ok(format!("ajouté {} octets à la fin de {}", content.len(), file.display()));
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
        "Cherche des fichiers et du texte dans une arborescence. Utilise un motif simple (extension) et une chaîne à trouver. Chaque ligne trouvée porte son numéro (L532) : lis autour avec read_file et start_line."
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
            let started = Instant::now();
            let mut interrompue = false;
            while let Some((dir, depth)) = stack.pop() {
                if depth > 8 || hits.len() >= max {
                    continue;
                }
                if started.elapsed() > SEARCH_BUDGET {
                    interrompue = true;
                    break;
                }
                let Ok(entries) = fs::read_dir(&dir) else {
                    continue;
                };
                for entry in entries.flatten() {
                    if hits.len() >= max {
                        break;
                    }
                    if started.elapsed() > SEARCH_BUDGET {
                        interrompue = true;
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
                        // Un gros fichier n'est pas une source : on le saute
                        // plutôt que de le lire en entier.
                        let trop_gros = entry
                            .metadata()
                            .map(|m| m.len() > MAX_INSPECT_BYTES)
                            .unwrap_or(true);
                        if trop_gros {
                            continue;
                        }
                        let matched = fs::read_to_string(&path)
                            .map(|text| matching_lines(&text, &query, 3))
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
            let note = if interrompue {
                format!(
                    "\n(recherche interrompue après {} s : résultats partiels — affine la racine ou le motif)",
                    SEARCH_BUDGET.as_secs()
                )
            } else {
                String::new()
            };
            Ok(clip(
                &format!("{} résultat(s)\n{}{note}", hits.len(), hits.join("\n")),
                MAX_TOOL_OUTPUT,
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fichier(n: usize) -> String {
        (1..=n).map(|i| format!("ligne {i} {}", "x".repeat(40))).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn un_petit_fichier_est_renvoye_tel_quel() {
        let text = "a\nb\nc";
        assert_eq!(read_window(text, 1, None, 12_000), text);
    }

    /// Cas réel : `storage.py` (23 Ko) coupé à 12 000 caractères, la fin
    /// invisible. La fenêtre dit où reprendre, et la suite se lit.
    #[test]
    fn un_gros_fichier_se_lit_par_morceaux() {
        let text = fichier(600);
        let first = read_window(&text, 1, None, 12_000);
        assert!(first.starts_with("[lignes 1–"), "{}", &first[..60]);
        assert!(first.contains("suite : start_line="), "{}", &first[..80]);
        assert!(first.chars().count() <= 12_000);
        // Lire la suite à partir de l'indication donnée.
        let next: usize = first.split("start_line=").nth(1).unwrap().split(']').next().unwrap().parse().unwrap();
        let second = read_window(&text, next, None, 12_000);
        assert!(second.contains(&format!("ligne {next} ")), "la suite commence au bon endroit");
        // Autour d'une ligne précise, quelques lignes seulement.
        let around = read_window(&text, 532, Some(5), 12_000);
        assert!(around.starts_with("[lignes 532–536 sur 600"), "{around}");
        assert!(around.contains("ligne 534 ") && !around.contains("ligne 537 "));
        assert!(read_window(&text, 900, None, 12_000).contains("n'a que 600 lignes"));
    }

    #[test]
    fn la_recherche_donne_les_numeros_de_ligne() {
        let text = "import x\n\ndef record_alert(idea_id):\n    pass";
        let hits = matching_lines(text, "record_alert", 3);
        assert_eq!(hits, vec!["      L3: def record_alert(idea_id):".to_string()]);
    }
}
