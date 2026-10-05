//! Skills : petites procédures que Jimmy sait appliquer, et qu'il peut
//! lui-même créer ou améliorer.
//!
//! Un skill est un dossier :
//!
//! ```text
//! skills/
//!   audit-code-rust/
//!     SKILL.md          <- frontmatter + instructions
//!     scripts/          <- facultatif
//! ```
//!
//! Le format suit celui des skills d'agents déjà présents sur la machine
//! (`.claude/skills`, `.config/opencode/skills`) : un `SKILL.md` avec un
//! frontmatter `name` / `description`, ce qui rend les skills de Jimmy
//! compatibles avec les autres outils plutôt que propriétaires.
//!
//! Points de conception retenus :
//!
//! * seul le *résumé* (nom + description) est injecté dans le prompt ; le
//!   contenu n'est chargé que si le modèle le demande. Un catalogue de dix
//!   skills coûte alors quelques lignes, pas plusieurs milliers ;
//! * un skill ne donne aucun droit nouveau : il passe par les mêmes
//!   permissions que n'importe quel outil (PLAN, risque n° 5).

pub mod capture;

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub body: String,
    pub path: PathBuf,
}

pub struct SkillStore {
    root: PathBuf,
}

impl SkillStore {
    pub fn new(root: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&root)?;
        Ok(SkillStore { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Transforme un nom libre en identifiant de dossier sûr.
    ///
    /// Les accents sont repliés (`évasion` devient `evasion`) et tout caractère
    /// non alphanumérique devient un tiret : un nom ne peut donc jamais
    /// s'échapper du dossier des skills.
    pub fn slugify(name: &str) -> String {
        let normalized = crate::memory::embed::normalize(name);
        let mut out = String::new();
        let mut previous_dash = true;
        for ch in normalized.chars() {
            if ch.is_ascii_alphanumeric() {
                out.push(ch);
                previous_dash = false;
            } else if !previous_dash {
                out.push('-');
                previous_dash = true;
            }
        }
        let trimmed = out.trim_matches('-').to_string();
        if trimmed.is_empty() {
            "skill".to_string()
        } else {
            trimmed.chars().take(60).collect()
        }
    }

    pub fn skill_dir(&self, name: &str) -> PathBuf {
        self.root.join(SkillStore::slugify(name))
    }

    pub fn list(&self) -> Result<Vec<Skill>> {
        let mut skills = Vec::new();
        for entry in std::fs::read_dir(&self.root)? {
            let entry = entry?;
            if !entry.path().is_dir() {
                continue;
            }
            if let Some(skill) = self.load_dir(&entry.path()) {
                skills.push(skill);
            }
        }
        skills.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        Ok(skills)
    }

    pub fn load(&self, name: &str) -> Result<Skill> {
        let dir = self.skill_dir(name);
        let skill = self
            .load_dir(&dir)
            .ok_or_else(|| Error::Tool(format!("skill inconnu : {name}")))?;
        Ok(skill)
    }

    fn load_dir(&self, dir: &Path) -> Option<Skill> {
        let file = dir.join("SKILL.md");
        let raw = std::fs::read_to_string(&file).ok()?;
        let (front, body) = split_frontmatter(&raw);
        let name = front
            .iter()
            .find(|(k, _)| k == "name")
            .map(|(_, v)| v.clone())
            .unwrap_or_else(|| {
                dir.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "skill".to_string())
            });
        let description = front
            .iter()
            .find(|(k, _)| k == "description")
            .map(|(_, v)| v.clone())
            .unwrap_or_default();
        Some(Skill {
            name,
            description,
            body,
            path: dir.to_path_buf(),
        })
    }

    /// Catalogue compact pour le prompt système.
    pub fn catalogue(&self) -> String {
        match self.list() {
            Ok(skills) if !skills.is_empty() => {
                let mut out = String::from("Skills disponibles (utilise read_skill pour charger le contenu) :\n");
                for skill in skills {
                    out.push_str(&format!("- {} : {}\n", skill.name, skill.description));
                }
                out
            }
            _ => String::new(),
        }
    }

    /// Sélection grossière : les skills dont le nom ou la description partage
    /// un mot avec la demande. Sert à proposer un skill pertinent à l'interface.
    pub fn suggest(&self, request: &str, limit: usize) -> Vec<Skill> {
        let words: Vec<String> = request
            .to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.len() > 3)
            .map(|w| w.to_string())
            .collect();
        let mut scored: Vec<(usize, Skill)> = Vec::new();
        for skill in self.list().unwrap_or_default() {
            let haystack = format!("{} {}", skill.name, skill.description).to_lowercase();
            let score = words.iter().filter(|w| haystack.contains(w.as_str())).count();
            if score > 0 {
                scored.push((score, skill));
            }
        }
        scored.sort_by(|a, b| b.0.cmp(&a.0));
        scored.into_iter().take(limit).map(|(_, s)| s).collect()
    }

    /// Crée ou réécrit un skill. Refuse d'écrire hors du dossier des skills.
    pub fn write(&self, name: &str, description: &str, body: &str) -> Result<PathBuf> {
        let dir = self.skill_dir(name);
        if !dir.starts_with(&self.root) {
            return Err(Error::PermissionDenied("chemin de skill invalide".into()));
        }
        std::fs::create_dir_all(&dir)?;
        let content = format!(
            "---\nname: {name}\ndescription: {description}\nupdated: {}\n---\n\n{body}\n",
            chrono::Utc::now().to_rfc3339()
        );
        let file = dir.join("SKILL.md");
        std::fs::write(&file, content.trim_end().to_string() + "\n")?;
        Ok(dir)
    }

    /// Améliore un skill existant en remplaçant son corps.
    pub fn update_body(&self, name: &str, body: &str) -> Result<PathBuf> {
        let existing = self.load(name)?;
        self.write(&existing.name, &existing.description, body)
    }
}

fn split_frontmatter(raw: &str) -> (Vec<(String, String)>, String) {
    let trimmed = raw.trim_start();
    if !trimmed.starts_with("---") {
        return (Vec::new(), raw.to_string());
    }
    let after = &trimmed[3..];
    let Some(end) = after.find("\n---") else {
        return (Vec::new(), raw.to_string());
    };
    let head = &after[..end];
    let body = after[end + 4..].trim_start_matches('\n').to_string();
    let front = head
        .lines()
        .filter_map(|line| {
            let (key, value) = line.split_once(':')?;
            Some((key.trim().to_string(), value.trim().to_string()))
        })
        .collect();
    (front, body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> SkillStore {
        let dir = std::env::temp_dir().join(format!("jimmy-skills-test-{}", uuid::Uuid::new_v4()));
        SkillStore::new(dir).unwrap()
    }

    #[test]
    fn slug_securise() {
        assert_eq!(SkillStore::slugify("Audit Code Rust"), "audit-code-rust");
        assert_eq!(SkillStore::slugify("../évasion/../etc"), "evasion-etc");
        assert_eq!(SkillStore::slugify("!!!"), "skill");
    }

    #[test]
    fn creation_et_relecture() {
        let store = temp_store();
        store
            .write("Audit Code Rust", "Vérifie la qualité d'un projet Rust", "1. cargo test")
            .unwrap();
        let skill = store.load("Audit Code Rust").unwrap();
        assert_eq!(skill.name, "Audit Code Rust");
        assert!(skill.body.contains("cargo test"));
        assert!(store.catalogue().contains("Audit Code Rust"));
    }

    #[test]
    fn frontmatter_incorrect_ignore() {
        let store = temp_store();
        let dir = store.skill_dir("manuel");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), "Pas de frontmatter ici.").unwrap();
        let skill = store.load("manuel").unwrap();
        assert_eq!(skill.name, "manuel");
        assert!(skill.description.is_empty());
    }
}