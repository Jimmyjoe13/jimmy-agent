//! Pont HTTP : Godot → Tauri.
//!
//! L'avatar Godot n'a aucun accès au backend, et n'en a pas besoin. Pour une
//! seule chose — « l'utilisateur a cliqué sur Jimmy, ouvre l'interface » — il
//! émet une requête HTTP vers un petit serveur local.
//!
//! Pourquoi HTTP plutôt qu'un canal bidirectionnel ? Parce que c'est déjà le
//! protocole du reste de la communication V1 : un seul mécanisme, une seule
//! chose à déboguer. Le serveur n'écoute que sur `127.0.0.1`, ce qui le rend
//! injoignable depuis le réseau.

use std::net::SocketAddr;

use axum::{routing::post, Json, Router};
use tauri::Manager;

use crate::AppState;

/// Routeur du pont. Volontairement minimal : une seule route en V1.
pub fn router(state: AppState) -> Router {
    Router::new().route("/ui/open", post(open_ui)).with_state(state)
}

async fn open_ui(state: axum::extract::State<AppState>) -> Json<serde_json::Value> {
    log::info!("[bridge] ouverture de l'interface demandée par l'avatar");
    // Le résultat est rapporté : avant, la route répondait `ok` même quand la
    // fenêtre n'avait pas pu s'afficher (piège 15).
    let Some(window) = state.app_handle.get_webview_window("main") else {
        log::warn!("[bridge] fenêtre principale introuvable");
        return Json(serde_json::json!({ "ok": false, "error": "fenêtre principale introuvable" }));
    };
    if let Err(error) = window.show() {
        log::warn!("[bridge] affichage impossible : {error}");
        return Json(serde_json::json!({ "ok": false, "error": error.to_string() }));
    }
    let _ = window.unminimize();
    let _ = window.set_focus();
    Json(serde_json::json!({ "ok": true }))
}

/// Démarre le serveur du pont. Retourne l'adresse d'écoute.
pub async fn start(state: AppState, host: &str, port: u16) -> Result<SocketAddr, String> {
    let address: SocketAddr = format!("{host}:{port}")
        .parse()
        .map_err(|e| format!("adresse de pont invalide : {e}"))?;
    let router = router(state);
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .map_err(|e| format!("pont indisponible sur {address} : {e}"))?;
    let local = listener
        .local_addr()
        .map_err(|e| format!("adresse du pont illisible : {e}"))?;
    tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, router).await {
            log::warn!("[bridge] serveur arrêté : {error}");
        }
    });
    log::info!("[bridge] serveur HTTP local sur http://{local}");
    Ok(local)
}