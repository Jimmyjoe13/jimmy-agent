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
//! * le prompt ne porte jamais le catalogue : seuls les 3 noms suggérés et
//!   une phrase d'orientation vers `list_skills` / `read_skill` y figurent.
//!   Un catalogue de dix skills comme de cinq cents coûte alors pareil :
//!   quelques lignes, pas plusieurs milliers (le catalogue injecté à chaque
//!   appel valait déjà ~1 800 jetons pour 32 skills, +900 par jour) ;
//! * un skill ne donne aucun droit nouveau : il passe par les mêmes
//!   permissions que n'importe quel outil (PLAN, risque n° 5).

pub mod capture;
pub mod demand;

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

    /// Mots-clés d'un texte : normalisés, plus de 3 lettres, triés, sans doublons.
    /// Même tokenisation que `suggest` : les deux se comparent à vocabulaire
    /// constant, sans appel au modèle.
    pub fn keywords(text: &str) -> Vec<String> {
        let normalized = crate::memory::embed::normalize(text);
        let mut words: Vec<String> = normalized
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.len() > 3)
            .map(|w| w.to_string())
            .collect();
        words.sort();
        words.dedup();
        words
    }

    /// Recouvrement entre deux ensembles de mots : taille de l'intersection
    /// sur taille du plus petit. Vaut 1 pour deux besoins identiques, 0 sans
    /// mot commun. Le dénominateur « plus petit » (et non l'union) évite de
    /// pénaliser une demande détaillée face à un nom court.
    pub fn overlap(a: &[String], b: &[String]) -> f64 {
        if a.is_empty() || b.is_empty() {
            return 0.0;
        }
        let inter = a.iter().filter(|w| b.contains(w)).count();
        inter as f64 / a.len().min(b.len()) as f64
    }

    /// Sélection grossière : les skills dont le nom, la description ou le
    /// corps partage un mot avec la demande. Le corps est inclus : sans
    /// catalogue injecté, c'est lui qui fait le rappel, sans coûter un jeton.
    /// Sert à proposer un skill pertinent à l'interface comme au prompt.
    pub fn suggest(&self, request: &str, limit: usize) -> Vec<Skill> {
        let words = Self::keywords(request);
        let mut scored: Vec<(usize, Skill)> = Vec::new();
        for skill in self.list().unwrap_or_default() {
            let haystack = crate::memory::embed::normalize(
                &format!("{} {} {}", skill.name, skill.description, skill.body),
            );
            let score = words
                .iter()
                .filter(|w| haystack.contains(w.as_str()))
                .count();
            if score > 0 {
                scored.push((score, skill));
            }
        }
        scored.sort_by(|a, b| b.0.cmp(&a.0));
        scored.into_iter().take(limit).map(|(_, s)| s).collect()
    }

    /// Skill existant quasi identique à un besoin (nom + description), ou
    /// `None`. Seuil volontairement strict (0,6) : le vocabulaire du domaine
    /// (serveur, vérifier, déployer…) sature tout, et à 0,4–0,5 on fusionne
    /// des sujets différents — mesuré sur les 32 skills réels avant codage.
    /// En dessous du seuil, c'est au modèle (ligne `proche:`) de trancher.
    pub fn find_similar(&self, name: &str, description: &str) -> Option<Skill> {
        let wanted = Self::keywords(&format!("{name} {description}"));
        let mut best: Option<(f64, Skill)> = None;
        for skill in self.list().unwrap_or_default() {
            // Jamais soi-même : même nom = aiguisage direct, pas « similaire ».
            if skill.name == name {
                continue;
            }
            let ours = Self::keywords(&format!("{} {}", skill.name, skill.description));
            let score = Self::overlap(&wanted, &ours);
            if score >= 0.6 && best.as_ref().is_none_or(|(s, _)| score > *s) {
                best = Some((score, skill));
            }
        }
        best.map(|(_, skill)| skill)
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
        // Pas de catalogue injecté : la découverte passe par suggest.
        let found = store.suggest("audit qualité projet rust", 3);
        assert!(found.iter().any(|s| s.name == "Audit Code Rust"));
    }

    #[test]
    fn rappel_passe_par_le_corps() {
        // Sans catalogue, c'est le corps qui fait le rappel : un mot présent
        // uniquement dans le corps doit suggérer le skill.
        let store = temp_store();
        store
            .write("Deploiement Exemple", "Mettre en ligne un service.", "1. Verifier le conteneur zebra.")
            .unwrap();
        let found = store.suggest("conteneur zebra en panne", 3);
        assert!(found.iter().any(|s| s.name == "Deploiement Exemple"));
    }

    #[test]
    fn similaire_strict_ou_rien() {
        // Mesuré sur les 32 skills réels : à 0,4-0,5 on fusionne des sujets
        // différents. Le seuil 0,6 ne retient que le quasi-identique.
        let store = temp_store();
        store
            .write("Diagnostiquer Demarrage Docker", "Quand les conteneurs Docker ne demarrent plus sur le serveur.", "1. docker ps. 2. journalctl.")
            .unwrap();
        store
            .write("Pousser Et Verifier Ci", "Apres un push, verifier que la CI passe.", "1. git push. 2. attendre la CI.")
            .unwrap();
        // Quasi-identique : fusion proposée.
        let twin = store.find_similar(
            "diagnostiquer-redemarrage-docker",
            "Quand les conteneurs Docker refusent de redemarrer sur le serveur.",
        );
        assert!(twin.is_some());
        assert_eq!(twin.unwrap().name, "Diagnostiquer Demarrage Docker");
        // Sujets différents malgré le vocabulaire partagé : pas de fusion.
        assert!(store.find_similar("etape-plan-de-correction", "Ecrire un plan de correction et le faire valider.").is_none());
        assert!(store.find_similar("auditer-gsc-et-ga4", "Auditer Search Console et Analytics.").is_none());
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