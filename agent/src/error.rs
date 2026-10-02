//! Types d'erreur de Jimmy.
//!
//! Un seul type d'erreur pour toute la V1 : il est transporté jusqu'à
//! l'interface, qui affiche un message lisible par l'utilisateur. Les détails
//! techniques restent dans les logs.

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("configuration invalide : {0}")]
    Config(String),

    #[error("base de données : {0}")]
    Db(#[from] rusqlite::Error),

    #[error("fichier I/O : {0}")]
    Io(#[from] std::io::Error),

    #[error("sérialisation : {0}")]
    Json(#[from] serde_json::Error),

    #[error("requête HTTP : {0}")]
    Http(#[from] reqwest::Error),

    #[error("service externe « {service} » : {message}")]
    Provider { service: String, message: String },

    #[error("transcription : {0}")]
    Stt(String),

    #[error("synthèse vocale : {0}")]
    Tts(String),

    #[error("mémoire : {0}")]
    Memory(String),

    #[error("permission refusée : {0}")]
    PermissionDenied(String),

    #[error("outil « {name} » introuvable")]
    UnknownTool { name: String },

    #[error("{0}")]
    Tool(String),

    #[error("avatar (Godot) : {0}")]
    Avatar(String),

    #[error("voix : {0}")]
    Voice(String),

    #[error("mcp : {0}")]
    Mcp(String),

    #[error("timeout")]
    Timeout,

    #[error("chemin introuvable : {0}")]
    PathNotFound(PathBuf),
}

impl Error {
    pub fn provider(service: &str, message: impl Into<String>) -> Self {
        Error::Provider {
            service: service.to_string(),
            message: message.into(),
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;