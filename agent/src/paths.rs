//! Chemins et répertoire de travail de Jimmy.
//!
//! Deux modes de fonctionnement, détectés automatiquement :
//!
//! * **développement** — le binaire est lancé depuis les sources ; les données
//!   restent dans le dépôt (`<racine>/data`) ;
//! * **installé** — le binaire est dans `Program Files` ; les données vont dans
//!   `%APPDATA%\Jimmy`.
//!
//! `JIMMY_HOME` force un répertoire, ce qui permet de tester plusieurs profils
//! côte à côte.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

#[derive(Debug, Clone)]
pub struct Paths {
    /// Racine des données : contient `jimmy.db`, `audio/`, `logs/`, `skills/`.
    pub data: PathBuf,
    /// Racine de l'application : contient `godot/`, `agent/skills/`.
    pub app: PathBuf,
    /// `true` si les données restent dans le dépôt (mode développement).
    pub dev: bool,
}

impl Paths {
    pub fn discover() -> Result<Self> {
        if let Some(home) = std::env::var_os("JIMMY_HOME") {
            let data = PathBuf::from(home);
            return Ok(Paths {
                app: data.clone(),
                data,
                dev: false,
            });
        }

        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));

        // Mode développement : on remonte jusqu'à trouver la racine du dépôt.
        let mut cursor: Option<&Path> = Some(exe_dir.as_path());
        while let Some(dir) = cursor {
            if dir.join("Cargo.toml").is_file() && dir.join("agent").is_dir() {
                return Ok(Paths {
                    data: dir.join("data"),
                    app: dir.to_path_buf(),
                    dev: true,
                });
            }
            cursor = dir.parent();
        }

        // Mode installé.
        let base = dirs_data_dir();
        Ok(Paths {
            data: base.clone(),
            app: base,
            dev: false,
        })
    }

    pub fn db_file(&self) -> PathBuf {
        self.data.join("jimmy.db")
    }

    pub fn config_file(&self) -> PathBuf {
        self.data.join("config.json")
    }

    pub fn audio_dir(&self) -> PathBuf {
        self.data.join("audio")
    }

    pub fn components_dir(&self) -> PathBuf {
        self.data.join("components")
    }

    pub fn whisper_dir(&self) -> PathBuf {
        self.components_dir().join("whisper")
    }

    /// Racine des skills écrits ou améliorés par Jimmy.
    pub fn skills_dir(&self) -> PathBuf {
        if self.dev {
            self.app.join("agent").join("skills")
        } else {
            self.data.join("skills")
        }
    }

    pub fn godot_project(&self) -> PathBuf {
        self.app.join("godot")
    }

    /// Crée les répertoires indispensables. Idempotent.
    pub fn ensure(&self) -> Result<()> {
        for dir in [
            &self.data,
            &self.audio_dir(),
            &self.components_dir(),
            &self.skills_dir(),
        ] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }
}

fn dirs_data_dir() -> PathBuf {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        PathBuf::from(appdata).join("Jimmy")
    } else if let Some(home) = std::env::var_os("USERPROFILE") {
        PathBuf::from(home).join("AppData").join("Roaming").join("Jimmy")
    } else {
        PathBuf::from("jimmy-data")
    }
}

/// Charge un fichier `.env` simple (`CLE=valeur`) sans dépendance externe.
/// Les variables déjà présentes dans l'environnement ont priorité.
pub fn load_dotenv(path: &Path) -> Result<()> {
    if !path.is_file() {
        return Ok(());
    }
    let raw = std::fs::read_to_string(path)?;
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let mut value = value.trim().to_string();
        // Retire les guillemets englobants éventuels.
        if value.len() >= 2
            && ((value.starts_with('"') && value.ends_with('"'))
                || (value.starts_with('\'') && value.ends_with('\'')))
        {
            value = value[1..value.len() - 1].to_string();
        }
        if std::env::var_os(key).is_none() {
            std::env::set_var(key, value);
        }
    }
    Ok(())
}

/// Résout l'exécutable Godot : variable d'environnement, dossier connu,
/// puis installation utilisateur classique.
pub fn find_godot_exe(configured: &str) -> Result<PathBuf> {
    if !configured.is_empty() {
        let p = PathBuf::from(configured);
        if p.is_file() {
            return Ok(p);
        }
        return Err(Error::PathNotFound(p));
    }
    if let Some(env) = std::env::var_os("JIMMY_GODOT_EXE") {
        let p = PathBuf::from(env);
        if p.is_file() {
            return Ok(p);
        }
    }
    let mut candidates: Vec<PathBuf> = vec![
        PathBuf::from(r"C:\Program Files\Godot\Godot_v4.5.1-stable_win64.exe"),
        PathBuf::from(r"C:\Dev\Godot\Godot_v4.5.1-stable_win64.exe"),
    ];
    // Sur cette machine, l'installation estrangère peut être rangée dans un
    // dossier du même nom que l'exécutable : on explore donc les dossiers
    // versionnés aussi, et on privilégie la build standard (sans C#).
    let mut roots: Vec<PathBuf> = vec![PathBuf::from(r"C:\Dev\Godot")];
    roots.push(PathBuf::from(r"C:\Program Files\Godot"));
    // Installation dans le dossier de l'utilisateur (`%USERPROFILE%\Godot`).
    if let Some(home) = std::env::var_os("USERPROFILE") {
        roots.push(PathBuf::from(home).join("Godot"));
    }
    for root in roots {
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let is_dir = path.is_dir();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !name.starts_with("Godot_v") || !name.ends_with("_win64.exe") {
                continue;
            }
            if is_dir {
                if let Ok(inner) = std::fs::read_dir(&path) {
                    for file in inner.flatten() {
                        let p = file.path();
                        let Some(n) = p.file_name().and_then(|n| n.to_str()) else {
                            continue;
                        };
                        if n.starts_with("Godot_v") && n.ends_with("_win64.exe") && !n.contains("mono") {
                            candidates.insert(0, p);
                        }
                    }
                }
            } else if !name.contains("mono") {
                candidates.insert(0, path);
            }
        }
    }
    candidates
        .into_iter()
        .find(|p| p.is_file())
        .ok_or_else(|| Error::Config("exécutable Godot introuvable (JIMMY_GODOT_EXE)".into()))
}