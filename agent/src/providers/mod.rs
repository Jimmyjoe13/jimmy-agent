//! Fournisseurs de Jimmy.
//!
//! Chaque capacité externe passe par un type dédié. Le PLAN demande que ces
//! interfaces soient interchangeables pour ne pas dépendre d'un fournisseur ;
//! c'est respecté par construction : l'agent ne manipule que des types locaux
//! ([`llm::LlmClient`], [`tts::Tts`], [`stt::Stt`], [`avatar::AvatarClient`]),
//! et le remplacement se fait dans [`crate::App`] sans toucher à la logique.

pub mod avatar;
pub mod llm;
pub mod protocol;
pub mod stt;
pub mod tts;

pub use avatar::AvatarClient;
pub use llm::{LlmClient, ModelInfo};
pub use stt::Stt;
pub use tts::{Tts, TtsVoice};