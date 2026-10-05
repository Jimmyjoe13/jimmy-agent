//! Projet d'une conversation : l'agent doit y travailler, au Chat comme à la
//! voix (`App::session_context`). Cas réel du 4 octobre : un projet choisi
//! pendant une conversation vocale n'était ni gardé, ni appliqué à la voix.

use std::sync::Arc;

use jimmy_agent::config::Secrets;
use jimmy_agent::paths::Paths;
use jimmy_agent::App;

fn app_isolee() -> (Arc<App>, std::path::PathBuf) {
    let racine = std::env::temp_dir().join(format!("jimmy-projet-{}", std::process::id()));
    std::fs::create_dir_all(racine.join("data")).unwrap();
    let paths = Paths { data: racine.join("data"), app: racine.clone(), dev: false };
    (App::new(paths, Secrets::default()).expect("app"), racine)
}

#[test]
fn le_projet_de_la_session_devient_le_dossier_de_travail() {
    let (app, racine) = app_isolee();
    let projet = racine.join("mon-projet");
    std::fs::create_dir_all(&projet).unwrap();
    let defaut = app.settings().workspace;

    let session = app.history.create_session("Session vocale").unwrap();
    // Sans projet : le dossier par défaut.
    let (settings, ctx) = app.session_context(&session);
    assert_eq!(settings.workspace, defaut);
    assert_eq!(ctx.workspace, std::path::PathBuf::from(&defaut));

    // Avec projet : prompt et outils travaillent dedans.
    app.history.set_project(&session, Some(&projet.to_string_lossy())).unwrap();
    let (settings, ctx) = app.session_context(&session);
    assert_eq!(settings.workspace, projet.to_string_lossy());
    assert_eq!(ctx.workspace, projet);

    // Projet supprimé du disque : retour au dossier par défaut, sans erreur.
    std::fs::remove_dir_all(&projet).unwrap();
    let (settings, _) = app.session_context(&session);
    assert_eq!(settings.workspace, defaut);

    drop(app);
    let _ = std::fs::remove_dir_all(&racine);
}
