//! Conversation courante partagée par le Chat et la voix. Cas réel du
//! 12 octobre : une discussion écrite (18 messages) continuée à la voix
//! partait dans une « Session vocale » neuve, historique vide — Jimmy perdait
//! le fil et repêchait un vieux sujet dans sa mémoire.

use std::sync::Arc;

use jimmy_agent::config::Secrets;
use jimmy_agent::paths::Paths;
use jimmy_agent::App;

fn app_isolee() -> (Arc<App>, std::path::PathBuf) {
    let racine = std::env::temp_dir().join(format!("jimmy-conversation-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(racine.join("data")).unwrap();
    // Vault temporaire : sans lui, la découverte automatique ouvrirait le
    // vrai vault.
    std::fs::create_dir_all(racine.join("vault")).ok();
    std::env::set_var("JIMMY_VAULT_PATH", racine.join("vault"));
    let paths = Paths { data: racine.join("data"), app: racine.clone(), dev: false };
    (App::new(paths, Secrets::default()).expect("app"), racine)
}

#[test]
fn la_voix_continue_la_conversation_du_chat() {
    let (app, racine) = app_isolee();
    // Conversation écrite en cours dans le Chat.
    let ecrite = app.history.create_session("Nouvelle session").unwrap();
    app.focus_conversation(Some(&ecrite));
    // La première commande vocale la continue, au lieu d'en ouvrir une neuve.
    assert_eq!(app.conversation_for_voice(), ecrite);
    // Et la suivante aussi.
    assert_eq!(app.conversation_for_voice(), ecrite);
    std::fs::remove_dir_all(racine).ok();
}

#[test]
fn nouvelle_conversation_du_chat_et_voix() {
    let (app, racine) = app_isolee();
    let ancienne = app.history.create_session("Nouvelle session").unwrap();
    app.focus_conversation(Some(&ancienne));
    // « Nouvelle session » dans le Chat : la voix n'hérite plus de l'ancienne.
    app.focus_conversation(None);
    let vocale = app.conversation_for_voice();
    assert_ne!(vocale, ancienne);
    // Puis le Chat ouvre une autre conversation depuis l'Historique.
    let autre = app.history.create_session("Autre").unwrap();
    app.focus_conversation(Some(&autre));
    assert_eq!(app.conversation_for_voice(), autre);
    std::fs::remove_dir_all(racine).ok();
}
