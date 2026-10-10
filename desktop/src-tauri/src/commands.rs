//! Commandes exposées à l'interface.
//!
//! Une seule règle : une commande ne fait qu'un travail et renvoie du JSON que
//! le frontend sait afficher. Toute la logique métier reste dans
//! `jimmy-agent`, ce crate ne fait que l'adaptation Tauri ↔ agent.
//!
//! Le canal d'événements (`agent-event`) porte le flux de l'agent : changement
//! d'état de l'avatar, appel d'outil, écriture mémoire, réponse finale. Le
//! frontend n'a donc jamais à *sonder* l'état : il écoute.


use std::sync::Arc;

use jimmy_agent::config::{Secrets, Settings, StartupMode};
use jimmy_agent::core::types::{AgentEvent, AvatarState};
use jimmy_agent::paths::load_dotenv;
use jimmy_agent::providers::tts::VoiceInfo;
use jimmy_agent::providers::tts::TtsVoice;
use jimmy_agent::voice::cues::{self, Cue};
use jimmy_agent::voice::{matches_wake_word, strip_wake_word, VoiceRuntime};
use serde::{Deserialize, Serialize};
use tauri::{Emitter, State, WebviewWindow};

use crate::AppState;

fn err(message: impl std::fmt::Display) -> String {
    message.to_string()
}

// ── Cycle de vie ─────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn bootstrap(state: State<'_, AppState>) -> std::result::Result<serde_json::Value, String> {
    let app = state.app.clone();
    let status = app.status();

    Ok(serde_json::json!({
        "status": status,
        "voices": TtsVoice::PRESETS.iter().map(|v| serde_json::json!({
            "id": v.id, "label": v.label, "description": v.description
        })).collect::<Vec<_>>(),
        "wake_words": ["jimy"],
        "startup_modes": ["manual", "with_windows", "with_windows_hidden"],
    }))
}

#[tauri::command]
pub async fn status(state: State<'_, AppState>) -> std::result::Result<serde_json::Value, String> {
    Ok(state.app.status())
}

// ── Conversation ─────────────────────────────────────────────────────────────

/// Tauri ne renomme que les arguments de premier niveau : les champs d'une
/// structure imbriquée arrivent tels que l'interface les écrit (`sessionId`).
/// Sans `rename_all`, `sessionId` était ignoré et chaque message ouvrait une
/// session neuve : Jimmy perdait le fil (piège 59).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatRequest {
    #[serde(alias = "session_id")]
    pub session_id: Option<String>,
    pub message: String,
    /// Projet choisi avant le premier message d'une conversation neuve.
    #[serde(default)]
    pub project: Option<String>,
    /// Capture du bouton « Joindre ma fenêtre » (`screen_capture`).
    #[serde(default)]
    pub capture_id: Option<String>,
}

#[tauri::command]
pub async fn chat(state: State<'_, AppState>, request: ChatRequest) -> std::result::Result<String, String> {
    let app = state.app.clone();
    let message = request.message.trim().to_string();
    if message.is_empty() {
        return Err("message vide".into());
    }

    let session_id = match request.session_id {
        Some(id) if !id.is_empty() => id,
        _ => {
            let id = app.history.create_session("Nouvelle session").map_err(err)?;
            if let Some(project) = request.project.as_deref().filter(|p| std::path::Path::new(p).is_dir()) {
                app.history.set_project(&id, Some(project)).map_err(err)?;
            }
            id
        }
    };

    // `/update` : mise à jour déterministe, pas un tour d'agent (le modèle
    // n'a rien à y décider : récupérer, recompiler, redémarrer).
    if jimmy_agent::update::is_update_command(&message) {
        return update_chat(state, session_id, message).await;
    }

    // Projet de la conversation : l'agent y travaille ; sans projet, le
    // dossier par défaut (même règle que la voix, `App::session_context`).
    let (settings, tool_context) = app.session_context(&session_id);
    let deps = app.deps();
    let window_label = state.window.clone();
    // Vision : capture du bouton, ou phrase explicite (« regarde mon écran »).
    let (images, note) = app.images_for(&message, request.capture_id.as_deref(), false).await;
    if let (Some(image), Some(window)) = (images.first(), &window_label) {
        let _ = window.emit(
            "agent-event",
            AgentEvent::Notice { message: format!("Fenêtre « {} » jointe à la demande.", image.label) },
        );
    }
    let avatar = app.avatar.clone();
    let cue_app = app.clone();
    let answer_session = session_id.clone();

    tauri::async_runtime::spawn(async move {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<AgentEvent>(64);
        let forward_window: Option<WebviewWindow> = window_label.clone();
        let relay_avatar = avatar.clone();

        // Relais des événements vers le frontend et vers l'avatar Godot.
        let relay = tauri::async_runtime::spawn(async move {
            // `success` ne se fête que pour une tâche outillée qui aboutit :
            // une simple réponse sans outil reste en `speaking`, sans saut.
            let mut tools_seen = false;
            let mut failed = false;
            while let Some(event) = rx.recv().await {
                if let Some(window) = &forward_window {
                    let _ = window.emit("agent-event", &event);
                }
                // L'avatar Godot n'a besoin que de l'état et de la phrase.
                match &event {
                    AgentEvent::State { state, detail } => {
                        let _ = relay_avatar.set_state(*state, detail).await;
                    }
                    AgentEvent::ToolStart { .. } => {
                        tools_seen = true;
                    }
                    AgentEvent::Failed { .. } => {
                        failed = true;
                        let _ = relay_avatar.set_state(AvatarState::Error, "").await;
                    }
                    AgentEvent::Final { text } => {
                        // Le geste `cheer` de Godot est additif : il joue
                        // par-dessus la parole qui suit, sans la retarder.
                        if tools_seen && !failed {
                            let _ = relay_avatar.set_state(AvatarState::Success, "").await;
                        }
                        relay_avatar.say(text, 0).await;
                    }
                    AgentEvent::Detached { .. } | AgentEvent::Background { .. } => {
                        background_avatar(&relay_avatar, &event).await;
                    }
                    _ => {}
                }
            }
        });

        // Tâche suivie : interruptible par « STOP » à la voix ou le bouton
        // « Arrêter » du Chat tant qu'elle est au premier plan ; si elle dure,
        // elle passe en arrière-plan et sa fin est traitée par l'agent
        // (`App::start_task`) — ce relais continue de la transmettre.
        let stop_session = answer_session.clone();
        let run_session = answer_session.clone();
        let request = match note {
            Some(note) => format!("{message}\n\n{note}"),
            None => message.clone(),
        };
        let ticket = cue_app.start_task(&answer_session, &message, tx, move |tx, background| {
            jimmy_agent::core::agent::run_with_images(deps.in_task(background), settings, run_session, request, images, tool_context, tx)
        });
        let outcome = match ticket.wait().await {
            jimmy_agent::TaskOutcome::Done(outcome) => outcome,
            jimmy_agent::TaskOutcome::Detached => return,
        };
        // Le relais doit être vidé avant de rendre la main, sinon la fenêtre
        // peut fermer avant d'avoir reçu les derniers événements.
        let _ = relay.await;

        // Jimmy lit sa réponse à voix haute, comme en vocal. (Avant : un son
        // « C'est prêt. » joué deux fois trop vite, et aucune voix.)
        // Arrêt d'urgence : ni erreur ni son d'échec ; la conversation note que
        // la tâche n'est pas finie, et le Chat affiche l'arrêt.
        if let Err(jimmy_agent::error::Error::Cancelled) = &outcome {
            log::info!("[agent] tâche du Chat arrêtée à la demande de l'utilisateur");
            let _ = cue_app.history.append(
                &stop_session,
                &jimmy_agent::core::types::Message::assistant("(Tâche arrêtée à la demande de l'utilisateur, avant la fin.)"),
            );
            if let Some(window) = &window_label {
                let _ = window.emit(
                    "agent-event",
                    AgentEvent::Final { text: "Arrêté à ta demande. Dis-moi quoi faire.".into() },
                );
            }
            let _ = avatar.set_state(AvatarState::Idle, "").await;
            return;
        }

        match &outcome {
            Ok(answer) => cue_app.speak(&answer.text).await,
            Err(_) => cues::play(&cue_app, Cue::Error).await,
        }

        if let Err(error) = outcome {
            log::error!("[agent] {error}");
            if let Some(window) = &window_label {
                let _ = window.emit(
                    "agent-event",
                    AgentEvent::Failed {
                        message: error.to_string(),
                    },
                );
            }
            let _ = avatar.set_state(AvatarState::Error, "").await;
        }
    });

    Ok(session_id)
}

/// `/update` tapé dans le Chat : applique la mise à jour disponible
/// (`pull` fast-forward, recompilation, redémarrage — voir
/// `jimmy_agent::update::apply`). Tourne en tâche suivie comme un tour
/// normal : passage en fond, bouton « Arrêter » et annonces compris.
async fn update_chat(
    state: State<'_, AppState>,
    session_id: String,
    message: String,
) -> std::result::Result<String, String> {
    let app = state.app.clone();
    let window_label = state.window.clone();
    let avatar = app.avatar.clone();
    let cue_app = app.clone();
    let answer_session = session_id.clone();
    let _ = app.history.append(
        &session_id,
        &jimmy_agent::core::types::Message::user(message.clone()),
    );

    tauri::async_runtime::spawn(async move {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<AgentEvent>(64);
        let forward_window: Option<WebviewWindow> = window_label.clone();
        let relay_avatar = avatar.clone();

        // Relais allégé : tout vers la fenêtre, la réponse finale dans la
        // bulle de l'avatar.
        let relay = tauri::async_runtime::spawn(async move {
            while let Some(event) = rx.recv().await {
                if let Some(window) = &forward_window {
                    let _ = window.emit("agent-event", &event);
                }
                match &event {
                    AgentEvent::ToolStart { .. } => {
                        let _ = relay_avatar.set_state(AvatarState::Thinking, "").await;
                    }
                    AgentEvent::Final { text } => {
                        relay_avatar.say(text, 0).await;
                    }
                    AgentEvent::Failed { .. } => {
                        let _ = relay_avatar.set_state(AvatarState::Error, "").await;
                    }
                    AgentEvent::Detached { .. } | AgentEvent::Background { .. } => {
                        background_avatar(&relay_avatar, &event).await;
                    }
                    _ => {}
                }
            }
        });

        let stop_session = answer_session.clone();
        let run_session = answer_session.clone();
        let update_app = cue_app.clone();
        let ticket = cue_app.start_task(&answer_session, &message, tx, move |tx, _background| {
            jimmy_agent::update::apply(update_app, run_session, tx)
        });
        let outcome = match ticket.wait().await {
            jimmy_agent::TaskOutcome::Done(outcome) => outcome,
            jimmy_agent::TaskOutcome::Detached => return,
        };
        let _ = relay.await;

        if let Err(jimmy_agent::error::Error::Cancelled) = &outcome {
            log::info!("[agent] mise à jour arrêtée à la demande de l'utilisateur");
            let _ = cue_app.history.append(
                &stop_session,
                &jimmy_agent::core::types::Message::assistant("(Mise à jour arrêtée à la demande de l'utilisateur, avant la fin.)"),
            );
            if let Some(window) = &window_label {
                let _ = window.emit(
                    "agent-event",
                    AgentEvent::Final { text: "Arrêté à ta demande. Le Jimy actuel tourne toujours.".into() },
                );
            }
            let _ = avatar.set_state(AvatarState::Idle, "").await;
            return;
        }

        match &outcome {
            Ok(answer) => {
                let _ = cue_app.history.append(
                    &stop_session,
                    &jimmy_agent::core::types::Message::assistant(answer.text.clone()),
                );
                cue_app.speak(&answer.text).await
            }
            Err(error) => {
                let _ = cue_app.history.append(
                    &stop_session,
                    &jimmy_agent::core::types::Message::assistant(format!("(Mise à jour impossible : {error})")),
                );
                cues::play(&cue_app, Cue::Error).await
            }
        }

        if let Err(error) = outcome {
            log::error!("[agent] {error}");
            if let Some(window) = &window_label {
                let _ = window.emit(
                    "agent-event",
                    AgentEvent::Failed {
                        message: error.to_string(),
                    },
                );
            }
            let _ = avatar.set_state(AvatarState::Error, "").await;
        }
    });

    Ok(session_id)
}

/// Réponse de l'utilisateur à une demande d'autorisation (modification d'un
/// fichier sensible). `false` si la demande n'existe plus (délai dépassé,
/// tâche arrêtée) : l'interface le signale au lieu de croire l'accord passé.
#[tauri::command]
pub async fn approval_respond(
    state: State<'_, AppState>,
    id: String,
    approved: bool,
    always: Option<bool>,
) -> std::result::Result<bool, String> {
    // `always` : « Toujours autoriser », la portée de la demande est retenue.
    Ok(state.app.approvals.respond(&id, approved, always.unwrap_or(false)))
}

/// Accords « Toujours autoriser » en vigueur (Paramètres → Sécurité).
#[tauri::command]
pub async fn approvals_always(state: State<'_, AppState>) -> std::result::Result<Vec<String>, String> {
    Ok(state.app.approvals.always_list())
}

/// Retire un accord permanent : la carte d'autorisation reviendra.
#[tauri::command]
pub async fn approval_revoke(state: State<'_, AppState>, key: String) -> std::result::Result<bool, String> {
    Ok(state.app.approvals.revoke(&key))
}

#[tauri::command]
pub async fn sessions(state: State<'_, AppState>) -> std::result::Result<Vec<serde_json::Value>, String> {
    let list = state.app.history.sessions(50).map_err(err)?;
    Ok(list
        .into_iter()
        .map(|s| {
            serde_json::json!({
                "id": s.id, "title": s.title,
                "createdAt": s.created_at, "updatedAt": s.updated_at,
                "messageCount": s.message_count, "project": s.project
            })
        })
        .collect())
}

#[tauri::command]
pub async fn session_messages(
    state: State<'_, AppState>,
    session_id: String,
) -> std::result::Result<Vec<serde_json::Value>, String> {
    let messages = state.app.history.messages(&session_id, 500).map_err(err)?;
    Ok(messages
        .into_iter()
        .filter(|m| {
            // Les résultats d'outils intermédiaires ne polluent pas
            // l'historique affiché : seuls les échanges counting.
            m.role != jimmy_agent::core::types::Role::Tool && !m.content.trim().is_empty()
        })
        .map(|m| serde_json::json!({ "role": m.role.as_str(), "content": m.content }))
        .collect())
}

#[tauri::command]
pub async fn delete_session(state: State<'_, AppState>, session_id: String) -> std::result::Result<(), String> {
    state.app.history.delete_session(&session_id).map_err(err)
}

// ── Paramètres ───────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> std::result::Result<Settings, String> {
    let mut settings = state.app.settings();
    // Les clés d'API ne quittent jamais le Rust : l'interface les voit via
    // `status` (has_key, key_from, key_hint) et les saisit par la commande
    // dédiée `llm_set_provider_key`.
    for provider in &mut settings.llm.providers {
        provider.api_key = String::new();
    }
    Ok(settings)
}

#[tauri::command]
pub async fn save_settings(state: State<'_, AppState>, settings: Settings) -> std::result::Result<(), String> {
    let app = state.app.clone();
    let before = app.settings();
    // Une clé vide venant de l'interface veut dire « inchangée » (`get_settings`
    // masque les clés) : seul `llm_set_provider_key` pose ou retire une clé.
    let mut settings = settings;
    for provider in &mut settings.llm.providers {
        if provider.api_key.trim().is_empty() {
            if let Some(old) = before.llm.providers.iter().find(|p| p.id == provider.id) {
                provider.api_key = old.api_key.clone();
            }
        }
    }
    app.save_settings(settings.clone()).map_err(err)?;
    // Les réglages d'avatar s'appliquent à chaud : sans cela, changer la
    // qualité ou le skin dans Paramètres n'avait d'effet qu'au relancement.
    if before.avatar.quality != settings.avatar.quality {
        app.avatar.set_quality(&settings.avatar.quality).await;
    }
    if before.avatar.skin != settings.avatar.skin {
        app.avatar.set_skin(&settings.avatar.skin).await;
    }
    Ok(())
}

/// Modèles d'un fournisseur (principal par défaut), enrichis par le catalogue
/// public pour OpenCode Go. `refresh` : ignore le cache d'une heure.
#[tauri::command]
pub async fn list_models(
    state: State<'_, AppState>,
    provider: Option<String>,
    refresh: Option<bool>,
) -> std::result::Result<Vec<serde_json::Value>, String> {
    let models = state
        .app
        .llm
        .list_models(provider.as_deref().filter(|p| !p.is_empty()), refresh.unwrap_or(false))
        .await
        .map_err(err)?;
    Ok(models
        .into_iter()
        .map(|m| {
            let mut value = serde_json::to_value(&m).unwrap_or_default();
            // `id` est l'identifiant **court**, celui qu'on enregistre et qu'on
            // envoie au fournisseur. (Une première version le remplaçait par
            // `fournisseur/modèle` : choisir un modèle aurait écrit
            // « opencode-go/xxx » dans la configuration et cassé toutes les
            // requêtes.) `model` reste un alias pour l'onboarding.
            value["model"] = serde_json::json!(m.id);
            value
        })
        .collect())
}

/// Teste un modèle dans les conditions de Jimmy (requête simple, puis avec outils).
#[tauri::command]
pub async fn llm_test_model(
    state: State<'_, AppState>,
    model: String,
    provider: Option<String>,
) -> std::result::Result<jimmy_agent::providers::llm::ModelTest, String> {
    let model = model.trim().to_string();
    if model.is_empty() {
        return Err("modèle vide".into());
    }
    Ok(state.app.llm.test_model(provider.as_deref().filter(|p| !p.is_empty()), &model).await)
}

/// Choisit le modèle principal (`main`) ou le modèle vocal (`voice`, vide = le
/// même que le principal), et le fournisseur qui le sert. S'applique
/// immédiatement, sans redémarrage.
#[tauri::command]
pub async fn set_llm_model(
    state: State<'_, AppState>,
    role: String,
    model: String,
    provider: Option<String>,
) -> std::result::Result<(), String> {
    let model = model.trim().to_string();
    let provider = provider.map(|p| p.trim().to_string()).filter(|p| !p.is_empty());
    let app = state.app.clone();
    let mut settings = app.settings();
    match role.as_str() {
        "main" => {
            if model.is_empty() {
                return Err("le modèle principal ne peut pas être vide".into());
            }
            settings.llm.model = model;
            if let Some(p) = provider {
                settings.llm.provider = p;
            }
        }
        "voice" => {
            settings.llm.voice_model = model.clone();
            // Plus de modèle vocal : le fournisseur vocal suit le principal.
            settings.llm.voice_provider = if model.is_empty() { String::new() } else { provider.unwrap_or(settings.llm.provider.clone()) };
        }
        other => return Err(format!("rôle inconnu : {other}")),
    }
    app.save_settings(settings).map_err(err)
}

// ── Fournisseurs LLM (Paramètres → LLM) ──────────────────────────────────────

/// Pose (`key` non vide) ou retire (`key` vide) la clé saisie d'un fournisseur.
#[tauri::command]
pub async fn llm_set_provider_key(
    state: State<'_, AppState>,
    provider: String,
    key: String,
) -> std::result::Result<(), String> {
    let app = state.app.clone();
    let mut settings = app.settings();
    let Some(p) = settings.llm.providers.iter_mut().find(|p| p.id == provider) else {
        return Err(format!("fournisseur inconnu : {provider}"));
    };
    p.api_key = key.trim().to_string();
    app.save_settings(settings).map_err(err)
}

/// Modifie un fournisseur (URL, format, méthode d'accès, activé, libellé). Les
/// clés ne passent pas par ici (voir `llm_set_provider_key`).
#[tauri::command]
pub async fn llm_update_provider(
    state: State<'_, AppState>,
    provider: String,
    label: String,
    base_url: String,
    protocol: Option<String>,
    auth: Option<String>,
    enabled: bool,
) -> std::result::Result<(), String> {
    let app = state.app.clone();
    let mut settings = app.settings();
    let Some(p) = settings.llm.providers.iter_mut().find(|p| p.id == provider) else {
        return Err(format!("fournisseur inconnu : {provider}"));
    };
    if !label.trim().is_empty() {
        p.label = label.trim().to_string();
    }
    if !base_url.trim().is_empty() {
        p.base_url = base_url.trim().to_string();
    }
    p.protocol = protocol.map(|s| s.trim().to_string()).filter(|s| !s.is_empty() && s != "auto");
    if let Some(auth) = auth.as_deref().filter(|a| !a.trim().is_empty()) {
        match auth {
            jimmy_agent::config::AUTH_API_KEY => p.auth = auth.into(),
            // L'abonnement Claude est une session Claude Code, pas un secret
            // générique : seul le fournisseur Anthropic sait s'en servir.
            jimmy_agent::config::AUTH_CLAUDE_PLAN if p.id == jimmy_agent::config::PROVIDER_ANTHROPIC => p.auth = auth.into(),
            jimmy_agent::config::AUTH_CLAUDE_PLAN => {
                return Err("l'abonnement Claude ne s'applique qu'au fournisseur Anthropic".into())
            }
            other => return Err(format!("méthode d'accès inconnue : {other}")),
        }
    }
    p.enabled = enabled;
    // Un fournisseur sur lequel un modèle est choisi ne peut pas être désactivé :
    // le routing retomberait sur un autre fournisseur et enverrait des modèles
    // qui n'existent pas chez lui.
    let still_referenced = (settings.llm.provider == p.id && !p.enabled)
        || (settings.llm.voice_provider == p.id && !p.enabled);
    if still_referenced {
        p.enabled = true;
        return Err(format!("« {} » sert un modèle choisi : retire le modèle avant de le désactiver", p.label));
    }
    app.save_settings(settings).map_err(err)
}

/// Ajoute un fournisseur personnalisé (LM Studio local, API d'entreprise…).
#[tauri::command]
pub async fn llm_add_provider(
    state: State<'_, AppState>,
    label: String,
    base_url: String,
    protocol: Option<String>,
    api_key: Option<String>,
) -> std::result::Result<String, String> {
    let label = label.trim().to_string();
    let base_url = base_url.trim().to_string();
    if label.is_empty() || base_url.is_empty() {
        return Err("nom et URL de base sont requis".into());
    }
    if !(base_url.starts_with("http://") || base_url.starts_with("https://")) {
        return Err("l'URL doit commencer par http:// ou https://".into());
    }
    let protocol = protocol.map(|s| s.trim().to_string()).filter(|s| !s.is_empty() && s != "auto");
    if let Some(p) = protocol.as_deref() {
        if !["chat", "responses", "messages"].contains(&p) {
            return Err(format!("format inconnu : {p}"));
        }
    }
    let app = state.app.clone();
    let mut settings = app.settings();
    if settings.llm.providers.iter().any(|p| p.label.to_lowercase() == label.to_lowercase()) {
        return Err(format!("un fournisseur « {label} » existe déjà"));
    }
    // Identifiant stable : custom-N après le plus grand existant.
    let n = settings
        .llm
        .providers
        .iter()
        .filter_map(|p| p.id.strip_prefix("custom-").and_then(|s| s.parse::<u32>().ok()))
        .max()
        .map_or(1, |m| m + 1);
    let id = format!("custom-{n}");
    settings.llm.providers.push(jimmy_agent::config::ProviderConfig {
        id: id.clone(),
        label,
        base_url,
        api_key: api_key.unwrap_or_default().trim().to_string(),
        protocol,
        x_api_key: false,
        auth: jimmy_agent::config::AUTH_API_KEY.into(),
        session_header: false,
        enabled: true,
        builtin: false,
    });
    app.save_settings(settings).map_err(err)?;
    Ok(id)
}

/// Retire un fournisseur personnalisé. Les intégrés ne se suppriment pas : on
/// les désactive.
#[tauri::command]
pub async fn llm_remove_provider(
    state: State<'_, AppState>,
    provider: String,
) -> std::result::Result<(), String> {
    let app = state.app.clone();
    let mut settings = app.settings();
    let Some(found) = settings.llm.providers.iter().find(|p| p.id == provider) else {
        return Err(format!("fournisseur inconnu : {provider}"));
    };
    if found.builtin {
        return Err("un fournisseur intégré se désactive, ne se supprime pas".into());
    }
    if settings.llm.provider == provider || settings.llm.voice_provider == provider {
        return Err("ce fournisseur sert un modèle choisi : choisis-en un autre d'abord".into());
    }
    settings.llm.providers.retain(|p| p.id != provider);
    app.save_settings(settings).map_err(err)
}

/// Vérifie la connexion d'un fournisseur : liste ses modèles accessibles.
/// Retourne le nombre (et la liste est de nouveau fraîche pour l'interface).
#[tauri::command]
pub async fn llm_check_provider(
    state: State<'_, AppState>,
    provider: String,
) -> std::result::Result<usize, String> {
    let models = state.app.llm.list_models(Some(&provider), true).await.map_err(err)?;
    Ok(models.len())
}

#[tauri::command]
pub async fn complete_onboarding(
    state: State<'_, AppState>,
    user_name: String,
    model: String,
    voice: String,
    quality: String,
    language: String,
) -> std::result::Result<serde_json::Value, String> {
    let app = state.app.clone();
    let mut settings = app.settings();
    if !user_name.trim().is_empty() {
        settings.ui.user_name = user_name.trim().to_string();
    }
    if !model.trim().is_empty() {
        settings.llm.model = model.trim().to_string();
    }
    if !voice.trim().is_empty() {
        settings.tts.voice = voice.trim().to_string();
    }
    settings.avatar.quality = quality;
    settings.stt.language = language;
    settings.ui.first_run_done = true;
    app.save_settings(settings).map_err(err)?;
    Ok(app.status())
}

#[tauri::command]
pub async fn set_startup(
    state: State<'_, AppState>,
    mode: StartupMode,
) -> std::result::Result<(), String> {
    let app = state.app.clone();
    let mut settings = app.settings();
    settings.startup = mode;
    app.save_settings(settings).map_err(err)?;

    use tauri_plugin_autostart::ManagerExt;
    let manager = state.app_handle.clone();
    let autostart = manager.autolaunch();
    let enabled = autostart.is_enabled().unwrap_or(false);
    let should_enable = matches!(mode, StartupMode::WithWindows | StartupMode::WithWindowsHidden);
    if should_enable && !enabled {
        let _ = autostart.enable();
    } else if !should_enable && enabled {
        let _ = autostart.disable();
    }
    Ok(())
}

// ── Permissions ──────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn get_permissions(state: State<'_, AppState>) -> std::result::Result<serde_json::Value, String> {
    let permissions = state.app.load_permissions();
    Ok(serde_json::to_value(permissions).map_err(err)?)
}

#[tauri::command]
pub async fn save_permissions(
    state: State<'_, AppState>,
    permissions: jimmy_agent::permissions::Permissions,
) -> std::result::Result<(), String> {
    state.app.save_permissions(&permissions).map_err(err)
}

// ── Mémoire ──────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn memory_list(state: State<'_, AppState>) -> std::result::Result<Vec<serde_json::Value>, String> {
    let memories = state.app.memory.list(200).map_err(err)?;
    Ok(memories
        .into_iter()
        .map(|m| {
            serde_json::json!({
                "id": m.id,
                "kind": m.kind.as_str(),
                "content": m.content,
                "importance": m.importance,
                "createdAt": m.created_at,
                "useCount": m.use_count
            })
        })
        .collect())
}

#[tauri::command]
pub async fn memory_forget(state: State<'_, AppState>, id: String) -> std::result::Result<(), String> {
    state.app.memory.forget(&id).map_err(err)
}

// ── Skills ───────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn skills_list(state: State<'_, AppState>) -> std::result::Result<Vec<serde_json::Value>, String> {
    let skills = state.app.skills.list().map_err(err)?;
    Ok(skills
        .into_iter()
        .map(|s| serde_json::json!({ "name": s.name, "description": s.description, "body": s.body }))
        .collect())
}

// ── Bibliothèque de voix ─────────────────────────────────────────────────────

/// Un identifiant de voix Fish Audio : 32 caractères hexadécimaux. Rien
/// d'autre n'est envoyé au service ni écrit dans la configuration.
fn is_voice_id(id: &str) -> bool {
    id.len() == 32 && id.chars().all(|c| c.is_ascii_hexdigit())
}

/// Voix actuelle, voix prédéfinies et voix ajoutées par l'utilisateur.
#[tauri::command]
pub async fn tts_voices(state: State<'_, AppState>) -> std::result::Result<serde_json::Value, String> {
    let settings = state.app.settings();
    let presets: Vec<VoiceInfo> = TtsVoice::PRESETS.iter().map(|v| v.info()).collect();
    Ok(serde_json::json!({
        "current": settings.tts.voice,
        "presets": presets,
        "library": settings.tts.library,
    }))
}

/// Recherche dans le catalogue public de Fish Audio, dans la langue d'écoute.
#[tauri::command]
pub async fn tts_search_voices(
    state: State<'_, AppState>,
    query: String,
) -> std::result::Result<Vec<VoiceInfo>, String> {
    let language = state.app.settings().stt.language;
    // « auto » ou vide : toutes les langues.
    let language = if language.len() == 2 { language } else { String::new() };
    state.app.tts.search_voices(&query, &language).await.map_err(err)
}

/// Choisit la voix de Jimmy ; une voix du catalogue rejoint la bibliothèque.
#[tauri::command]
pub async fn tts_set_voice(state: State<'_, AppState>, voice: VoiceInfo) -> std::result::Result<(), String> {
    if !is_voice_id(&voice.id) {
        return Err("identifiant de voix invalide".into());
    }
    let app = state.app.clone();
    let mut settings = app.settings();
    let known = TtsVoice::from_id(&voice.id).is_some() || settings.tts.library.iter().any(|v| v.id == voice.id);
    if !known {
        settings.tts.library.push(voice.clone());
    }
    settings.tts.voice = voice.id;
    app.save_settings(settings).map_err(err)
}

/// Retire une voix de la bibliothèque. Si c'était la voix de Jimmy, il
/// revient à la voix par défaut. Renvoie la voix désormais utilisée.
#[tauri::command]
pub async fn tts_remove_voice(state: State<'_, AppState>, id: String) -> std::result::Result<String, String> {
    let app = state.app.clone();
    let mut settings = app.settings();
    settings.tts.library.retain(|v| v.id != id);
    if settings.tts.voice == id {
        settings.tts.voice = TtsVoice::NARRATEUR.id.to_string();
    }
    let current = settings.tts.voice.clone();
    app.save_settings(settings).map_err(err)?;
    Ok(current)
}

/// Arrêt d'urgence (bouton « Arrêter » du Chat) : la tâche au premier plan
/// est abandonnée et Jimmy se tait ; une tâche de fond continue (`task_stop`).
/// Renvoie `true` s'il y avait quelque chose à arrêter.
#[tauri::command]
pub async fn agent_stop(state: State<'_, AppState>) -> std::result::Result<bool, String> {
    Ok(state.app.request_stop())
}

/// Capture de la fenêtre active pour le prochain message (bouton « Joindre
/// ma fenêtre »). Renvoie de quoi l'afficher ; l'image reste en mémoire.
#[tauri::command]
pub async fn screen_capture(state: State<'_, AppState>) -> std::result::Result<serde_json::Value, String> {
    let capture = state.app.capture_screen().await.map_err(err)?;
    let preview = capture.image.data_url();
    let label = capture.image.label.clone();
    let id = state.app.stash_capture(capture.image);
    Ok(serde_json::json!({
        "id": id, "label": label, "app": capture.app,
        "width": capture.width, "height": capture.height, "preview": preview,
    }))
}

/// Retire une capture en attente (croix de la vignette).
#[tauri::command]
pub async fn screen_discard(state: State<'_, AppState>, id: String) -> std::result::Result<bool, String> {
    Ok(state.app.take_capture(&id).is_some())
}

/// Tâches en cours (bandeau « en arrière-plan » du Chat).
#[tauri::command]
pub async fn tasks_list(state: State<'_, AppState>) -> std::result::Result<Vec<jimmy_agent::tasks::TaskInfo>, String> {
    Ok(state.app.tasks.list())
}

/// Arrête une tâche précise (bouton de la tâche de fond). `false` si elle
/// est déjà finie.
#[tauri::command]
pub async fn task_stop(state: State<'_, AppState>, id: String) -> std::result::Result<bool, String> {
    Ok(state.app.stop_task(&id))
}

/// Avatar et tâche de fond : elle ne prend pas le renard (l'utilisateur fait
/// autre chose pendant ce temps) ; c'est le **lapin** qui travaille à côté de
/// lui (`/helper`). Le passage en fond rend le renard au repos et fait
/// apparaître le lapin ; la fin réussie fait sauter le lapin et joue la joie
/// du renard (une tâche de fond est toujours outillée) ; l'échec ou l'arrêt
/// baisse les oreilles du lapin. La phrase est dite par l'annonce.
async fn background_avatar(avatar: &jimmy_agent::providers::AvatarClient, event: &AgentEvent) {
    match event {
        AgentEvent::Detached { .. } => {
            let _ = avatar.set_state(AvatarState::Idle, "").await;
            let _ = avatar.helper("working").await;
        }
        AgentEvent::Background { event, .. } => match event.as_ref() {
            AgentEvent::Final { text } if text == jimmy_agent::tasks::BACKGROUND_STOPPED => {
                let _ = avatar.helper("error").await;
            }
            AgentEvent::Final { .. } => {
                let _ = avatar.set_state(AvatarState::Success, "").await;
                let _ = avatar.helper("success").await;
            }
            AgentEvent::Failed { .. } => {
                let _ = avatar.set_state(AvatarState::Error, "").await;
                let _ = avatar.helper("error").await;
            }
            _ => {}
        },
        _ => {}
    }
}

/// Serveurs MCP connectés ou configurés, secrets masqués.
#[tauri::command]
pub async fn mcp_servers(state: State<'_, AppState>) -> std::result::Result<Vec<jimmy_agent::mcp::McpServerStatus>, String> {
    Ok(state.app.mcp_overview())
}

#[tauri::command]
pub async fn skill_read(state: State<'_, AppState>, name: String) -> std::result::Result<String, String> {
    state.app.skills.load(&name).map(|s| s.body).map_err(err)
}

// ── Voix ─────────────────────────────────────────────────────────────────────

/// Essai de synthèse. `voice` : une autre voix que celle de Jimmy, pour
/// l'écouter avant de la choisir (bibliothèque de voix).
#[tauri::command]
pub async fn tts_preview(
    state: State<'_, AppState>,
    text: String,
    voice: Option<String>,
) -> std::result::Result<serde_json::Value, String> {
    let app = state.app.clone();
    let settings = app.settings();
    if !settings.tts.enabled {
        return Err("la synthèse vocale est désactivée".into());
    }
    let voice = voice.filter(|v| !v.trim().is_empty()).unwrap_or_else(|| settings.tts.voice.clone());
    if !is_voice_id(&voice) {
        return Err("identifiant de voix invalide".into());
    }
    let speech = app
        .tts
        .speak(
            &app.secrets.openrouter_api_key,
            &settings.tts.model,
            &voice,
            &text,
            settings.tts.chars_per_minute,
        )
        .await
        .map_err(err)?;
    let bytes = speech.bytes.clone();
    let rate = speech.sample_rate;
    jimmy_agent::voice::play_bytes(&bytes, rate).map_err(err)?;
    Ok(serde_json::json!({ "bytes": speech.bytes.len(), "durationMs": speech.estimated_ms }))
}

#[tauri::command]
pub async fn voice_devices() -> std::result::Result<Vec<String>, String> {
    Ok(VoiceRuntime::list_devices())
}

/// Démarre l'écoute permanente et la boucle de reconnaissance.
///
/// La boucle vit dans le crate agent ; elle émet les mêmes événements que le
/// chat, ce qui évite deux canaux à maintenir côté interface.
#[tauri::command]
pub async fn voice_start(
    state: State<'_, AppState>,
    runtime: State<'_, Arc<VoiceRuntime>>,
) -> std::result::Result<(), String> {
    start_listening(state.app.clone(), runtime.inner().clone(), state.window.clone()).await?;
    remember_listening(&state.app, true);
    Ok(())
}

/// Mémorise le choix de l'utilisateur pour le prochain lancement.
fn remember_listening(app: &jimmy_agent::App, on: bool) {
    let mut settings = app.settings();
    if settings.voice.listen_on_start != on {
        settings.voice.listen_on_start = on;
        if let Err(error) = app.save_settings(settings) {
            log::warn!("[voice] préférence d'écoute non enregistrée : {error}");
        }
    }
}

/// Démarre micro + boucle d'écoute + relais d'événements. Idempotent : si
/// l'écoute tourne déjà, ne lance **pas** une seconde boucle (avant, chaque
/// clic en ajoutait une sur le même micro). Utilisé par la commande et par la
/// reprise automatique au lancement.
pub async fn start_listening(
    app: Arc<jimmy_agent::App>,
    runtime: Arc<VoiceRuntime>,
    window: Option<WebviewWindow>,
) -> std::result::Result<(), String> {
    if runtime.is_running() {
        return Ok(());
    }
    app.start_voice(&runtime).await.map_err(err)?;

    // Pré-génère les sons d'état en tâche de fond : le premier « Oui ? » doit
    // être immédiat, pas attendre un aller-retour réseau.
    let warm_app = app.clone();
    tauri::async_runtime::spawn(async move { cues::warm(&warm_app).await });

    let handle = window;
    let avatar = app.avatar.clone();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<AgentEvent>(64);

    let relay = tauri::async_runtime::spawn(async move {
        // Même règle que le relais du Chat : joie seulement pour une tâche
        // outillée qui aboutit, jamais après un « STOP » (voir STOPPED_REPLY).
        let mut tools_seen = false;
        let mut failed = false;
        while let Some(event) = rx.recv().await {
            if let Some(window) = &handle {
                let _ = window.emit("agent-event", &event);
            }
            match &event {
                AgentEvent::State { state, detail } => {
                    let _ = avatar.set_state(*state, detail).await;
                }
                AgentEvent::ToolStart { .. } => {
                    tools_seen = true;
                }
                AgentEvent::Failed { .. } => {
                    failed = true;
                    let _ = avatar.set_state(AvatarState::Error, "").await;
                }
                AgentEvent::Final { text } => {
                    if tools_seen
                        && !failed
                        && text != jimmy_agent::voice::listener::STOPPED_REPLY
                    {
                        let _ = avatar.set_state(AvatarState::Success, "").await;
                    }
                    avatar.say(text, 0).await;
                }
                AgentEvent::Detached { .. } | AgentEvent::Background { .. } => {
                    background_avatar(&avatar, &event).await;
                }
                _ => {}
            }
        }
    });

    let app_for_loop = app.clone();
    tauri::async_runtime::spawn(async move {
        let _ = app_for_loop.spawn_voice_listener(runtime, tx).await;
        relay.abort();
    });
    Ok(())
}

/// Coupe l'écoute et libère le relais d'événements.
#[tauri::command]
pub async fn voice_stop(
    state: State<'_, AppState>,
    runtime: State<'_, Arc<VoiceRuntime>>,
) -> std::result::Result<(), String> {
    runtime.stop();
    remember_listening(&state.app, false);
    let _ = state.app.avatar.set_state(AvatarState::Idle, "").await;
    Ok(())
}

/// État réel de l'écoute. L'interface s'y fie au lieu de garder son propre
/// drapeau, qui retombait à « arrêtée » à chaque changement de vue.
#[tauri::command]
pub async fn voice_status(
    state: State<'_, AppState>,
    runtime: State<'_, Arc<VoiceRuntime>>,
) -> std::result::Result<serde_json::Value, String> {
    let app = state.app.clone();
    let settings = app.settings();
    let wake_ready = match app.stt.lock().await.as_ref() {
        Some(stt) => stt.health().await,
        None => false,
    };
    let command_ready = match app.stt_command.lock().await.as_ref() {
        Some(stt) => stt.health().await,
        None => false,
    };
    Ok(serde_json::json!({
        "running": runtime.is_running(),
        "deviceRate": runtime.device_rate(),
        "wakeWord": settings.stt.wake_word,
        "wakeModel": settings.stt.model,
        "commandModel": settings.stt.command_model,
        "wakeReady": wake_ready,
        "commandReady": command_ready,
        "listenOnStart": settings.voice.listen_on_start,
        // Niveau du micro sur les 300 dernières ms (vumètre de la vue Voix).
        "level": runtime.level(4_800),
    }))
}

/// Transcrit un WAV 16 kHz envoyé par l'interface (onboarding, test du micro).
#[tauri::command]
pub async fn stt_transcribe(state: State<'_, AppState>, wav: Vec<u8>) -> std::result::Result<String, String> {
    // Les serveurs démarrent à la demande : tester la transcription ne doit
    // pas exiger d'avoir activé l'écoute permanente.
    state.app.ensure_stt().await.map_err(err)?;
    // Une phrase libre, comme une commande : modèle précis si disponible.
    state.app.transcribe_command(wav).await.map_err(err)
}

// ── Avatar ───────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn avatar_start(state: State<'_, AppState>) -> std::result::Result<(), String> {
    state.app.start_avatar().await.map_err(err)
}

#[tauri::command]
pub async fn avatar_stop(state: State<'_, AppState>) -> std::result::Result<(), String> {
    state.app.stop_avatar().await;
    Ok(())
}

#[tauri::command]
pub async fn avatar_state(
    state: State<'_, AppState>,
    avatar_state: AvatarState,
    detail: String,
) -> std::result::Result<(), String> {
    state.app.avatar.set_state(avatar_state, &detail).await;
    Ok(())
}

#[tauri::command]
pub async fn avatar_say(state: State<'_, AppState>, text: String) -> std::result::Result<(), String> {
    state.app.avatar.say(&text, 0).await;
    Ok(())
}

#[tauri::command]
pub async fn avatar_quality(state: State<'_, AppState>, level: String) -> std::result::Result<(), String> {
    let app = state.app.clone();
    let mut settings = app.settings();
    settings.avatar.quality = level.clone();
    app.save_settings(settings).map_err(err)?;
    app.avatar.set_quality(&level).await;
    Ok(())
}

#[tauri::command]
pub async fn avatar_dodge(state: State<'_, AppState>, enabled: bool) -> std::result::Result<(), String> {
    let app = state.app.clone();
    let mut settings = app.settings();
    settings.avatar.dodge = enabled;
    app.save_settings(settings).map_err(err)?;
    app.avatar.set_dodge(enabled).await;
    Ok(())
}

#[tauri::command]
pub async fn avatar_skin(state: State<'_, AppState>, skin: String) -> std::result::Result<(), String> {
    if !jimmy_agent::providers::avatar::SKINS.iter().any(|(id, _)| *id == skin) {
        return Err(format!("skin inconnu : {skin}"));
    }
    let app = state.app.clone();
    let mut settings = app.settings();
    settings.avatar.skin = skin.clone();
    app.save_settings(settings).map_err(err)?;
    app.avatar.set_skin(&skin).await;
    Ok(())
}

// ── Outils de diagnostic ─────────────────────────────────────────────────────

/// Test hors ligne de la détection du wake word, utilisé par l'onboarding :
/// on Injecte une transcription et on vérifie le filtre.
#[tauri::command]
pub fn wake_word_test(transcript: String, wake_word: String) -> std::result::Result<bool, String> {
    Ok(matches_wake_word(&transcript, &wake_word))
}

#[derive(Serialize)]
pub struct WokeCommand {
    pub command: String,
    pub matched: bool,
}

#[tauri::command]
pub fn wake_word_strip(transcript: String, wake_word: String) -> std::result::Result<WokeCommand, String> {
    let matched = matches_wake_word(&transcript, &wake_word);
    Ok(WokeCommand {
        command: if matched {
            strip_wake_word(&transcript, &wake_word)
        } else {
            transcript.clone()
        },
        matched,
    })
}

/// Vérifie que tous les composants locaux sont en place. Utilisé par
/// l'installateur et par la page « Diagnostic ».
#[tauri::command]
pub async fn doctor(state: State<'_, AppState>) -> std::result::Result<serde_json::Value, String> {
    let app = state.app.clone();
    let settings = app.settings();
    let whisper_dir = app.paths.whisper_dir();
    let model = whisper_dir.join("models").join(&settings.stt.model);
    let server = whisper_dir.join("Release").join(&settings.stt.server_exe);

    let mut checks: Vec<serde_json::Value> = Vec::new();
    checks.push(check(
        "Clé OpenCode Go",
        !app.secrets.opencode_api_key.is_empty(),
        "OPENCODE_API_KEY absent de .env",
    ));
    checks.push(check(
        "Clé OpenRouter (voix)",
        !app.secrets.openrouter_api_key.is_empty(),
        "OPENROUTER_API_KEY absent de .env",
    ));
    checks.push(check(
        "whisper.cpp",
        server.is_file(),
        &format!("{} introuvable", server.display()),
    ));
    checks.push(check(
        "Modèle de reconnaissance vocale",
        model.is_file(),
        &format!("{} introuvable", model.display()),
    ));
    if !settings.stt.command_model.trim().is_empty() {
        let command_model = whisper_dir.join("models").join(&settings.stt.command_model);
        checks.push(check(
            "Modèle de commande (précis)",
            command_model.is_file(),
            &format!("{} introuvable — la commande utilisera le modèle du wake word", command_model.display()),
        ));
    }
    let godot = jimmy_agent::paths::find_godot_exe(&settings.avatar.godot_exe);
    checks.push(check(
        "Godot",
        godot.is_ok(),
        "exécutable Godot introuvable (JIMMY_GODOT_EXE)",
    ));
    checks.push(check(
        "Avatar en cours d'exécution",
        app.avatar.is_up().await,
        "l'avatar ne répond pas",
    ));
    match &app.vault {
        Some(vault) => {
            let existant = vault.root().is_dir();
            let hint = if existant {
                format!("vault ouvert : {}", vault.folder_display())
            } else {
                "dossier du vault introuvable".to_string()
            };
            checks.push(check(
                "Vault Obsidian",
                existant,
                &hint,
            ));
        }
        None => checks.push(check(
            "Vault Obsidian",
            false,
            "chemin du vault absent des réglages mémoire",
        )),
    }

    let ok = checks.iter().filter(|c| c["ok"].as_bool() == Some(true)).count();
    let total = checks.len();
    Ok(serde_json::json!({ "ok": ok == total, "passed": ok, "total": total, "checks": checks }))
}

fn check(name: &str, ok: bool, hint: &str) -> serde_json::Value {
    serde_json::json!({ "name": name, "ok": ok, "hint": hint })
}

/// Recherche une nouvelle version de Jimmy.
///
/// La V1 n'a pas de source de distribution publique : la vérification est donc
/// branchée sur une source *optionnelle*, déclarée par `JIMMY_UPDATE_ENDPOINT`
/// dans le `.env`. Sans elle, la commande le dit clairement plutôt que de
/// laisser croire qu'une vérification a eu lieu.
#[tauri::command]
pub async fn check_update() -> std::result::Result<serde_json::Value, String> {
    let endpoint = std::env::var("JIMMY_UPDATE_ENDPOINT").unwrap_or_default();
    if endpoint.trim().is_empty() {
        return Ok(serde_json::json!({
            "checked": false,
            "reason": "aucune source de mise à jour configurée (JIMMY_UPDATE_ENDPOINT absent du .env)",
            "current": env!("CARGO_PKG_VERSION"),
        }));
    }
    let response = reqwest::get(&endpoint)
        .await
        .map_err(|e| format!("source de mise à jour injoignable : {e}"))?;
    let payload: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("réponse de mise à jour illisible : {e}"))?;
    let latest = payload
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("inconnue");
    Ok(serde_json::json!({
        "checked": true,
        "current": env!("CARGO_PKG_VERSION"),
        "latest": latest,
        "update_available": latest != env!("CARGO_PKG_VERSION"),
        "note": "l'installation de la mise à jour reste une action manuelle en V1",
    }))
}
#[tauri::command]
pub fn paths_info(state: State<'_, AppState>) -> std::result::Result<serde_json::Value, String> {
    let app = state.app.clone();
    Ok(serde_json::json!({
        "data": app.paths.data,
        "app": app.paths.app,
        "dev": app.paths.dev,
        "skills": app.paths.skills_dir(),
        "components": app.paths.components_dir(),
        "config": app.paths.config_file(),
    }))
}

/// Recharge les secrets depuis `.env` sans redémarrer (utile après avoir
/// rempli le fichier pendant l'onboarding).
#[tauri::command]
pub async fn reload_secrets(state: State<'_, AppState>) -> std::result::Result<serde_json::Value, String> {
    let app = state.app.clone();
    let env_path = app.paths.app.join(".env");
    load_dotenv(&env_path).map_err(err)?;
    let secrets = Secrets::from_env();
    let missing = jimmy_agent::config::missing_secrets(&secrets);
    Ok(serde_json::json!({ "missing": missing }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cas réel du 4 octobre : chaque message du Chat ouvrait une session
    /// neuve, Jimmy perdait le fil. L'interface envoie `sessionId` (camelCase)
    /// dans une structure imbriquée, que Tauri ne renomme pas.
    #[test]
    fn la_requete_de_chat_garde_la_session_envoyee_par_l_interface() {
        let json = serde_json::json!({ "sessionId": "abc-123", "message": "installe-le" });
        let request: ChatRequest = serde_json::from_value(json).expect("requête valide");
        assert_eq!(request.session_id.as_deref(), Some("abc-123"));
        // Premier message : pas encore de session.
        let first: ChatRequest = serde_json::from_value(serde_json::json!({ "sessionId": null, "message": "salut" })).unwrap();
        assert!(first.session_id.is_none());
    }
}