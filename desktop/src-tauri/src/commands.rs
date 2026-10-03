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
        "wake_words": ["jimmy"],
        "startup_modes": ["manual", "with_windows", "with_windows_hidden"],
    }))
}

#[tauri::command]
pub async fn status(state: State<'_, AppState>) -> std::result::Result<serde_json::Value, String> {
    Ok(state.app.status())
}

// ── Conversation ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ChatRequest {
    pub session_id: Option<String>,
    pub message: String,
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
        _ => app.history.create_session("Nouvelle session").map_err(err)?,
    };

    let settings = app.settings();
    let deps = app.deps();
    let tool_context = app.tool_context();
    let window_label = state.window.clone();
    let avatar = app.avatar.clone();
    let cue_app = app.clone();
    let answer_session = session_id.clone();

    tauri::async_runtime::spawn(async move {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<AgentEvent>(64);
        let forward_window: Option<WebviewWindow> = window_label.clone();
        let relay_avatar = avatar.clone();

        // Relais des événements vers le frontend et vers l'avatar Godot.
        let relay = tauri::async_runtime::spawn(async move {
            while let Some(event) = rx.recv().await {
                if let Some(window) = &forward_window {
                    let _ = window.emit("agent-event", &event);
                }
                // L'avatar Godot n'a besoin que de l'état et de la phrase.
                match &event {
                    AgentEvent::State { state, detail } => {
                        let _ = relay_avatar.set_state(*state, detail).await;
                    }
                    AgentEvent::Final { text } => {
                        relay_avatar.say(text, 0).await;
                    }
                    _ => {}
                }
            }
        });

        let outcome = jimmy_agent::core::agent::run(
            deps,
            settings,
            answer_session,
            message,
            tool_context,
            tx,
        )
        .await;
        // Le relais doit être vidé avant de rendre la main, sinon la fenêtre
        // peut fermer avant d'avoir reçu les derniers événements.
        let _ = relay.await;

        // Son d'état : le chat texte ne parle pas, un « C'est prêt. » signale
        // la réponse quand la fenêtre n'est pas sous les yeux.
        let cue = if outcome.is_ok() { Cue::Answer } else { Cue::Error };
        cues::play(&cue_app, cue).await;

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

#[tauri::command]
pub async fn sessions(state: State<'_, AppState>) -> std::result::Result<Vec<serde_json::Value>, String> {
    let list = state.app.history.sessions(50).map_err(err)?;
    Ok(list
        .into_iter()
        .map(|s| {
            serde_json::json!({
                "id": s.id, "title": s.title,
                "createdAt": s.created_at, "updatedAt": s.updated_at,
                "messageCount": s.message_count
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
    Ok(state.app.settings())
}

#[tauri::command]
pub async fn save_settings(state: State<'_, AppState>, settings: Settings) -> std::result::Result<(), String> {
    let app = state.app.clone();
    let before = app.settings();
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

#[tauri::command]
pub async fn list_models(state: State<'_, AppState>) -> std::result::Result<Vec<serde_json::Value>, String> {
    let models = state.app.llm.list_models("opencode-go").await.map_err(err)?;
    Ok(models
        .into_iter()
        .map(|m| {
            serde_json::json!({
                "id": m.full_id,
                "model": m.id,
                "name": m.name,
                "context": m.context,
                "free": m.free
            })
        })
        .collect())
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

#[tauri::command]
pub async fn skill_read(state: State<'_, AppState>, name: String) -> std::result::Result<String, String> {
    state.app.skills.load(&name).map(|s| s.body).map_err(err)
}

// ── Voix ─────────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn tts_preview(
    state: State<'_, AppState>,
    text: String,
) -> std::result::Result<serde_json::Value, String> {
    let app = state.app.clone();
    let settings = app.settings();
    if !settings.tts.enabled {
        return Err("la synthèse vocale est désactivée".into());
    }
    let speech = app
        .tts
        .speak(
            &app.secrets.openrouter_api_key,
            &settings.tts.model,
            &settings.tts.voice,
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
    let app = state.app.clone();
    app.start_voice(&runtime).await.map_err(err)?;

    // Pré-génère les sons d'état en tâche de fond : le premier « Oui ? » doit
    // être immédiat, pas attendre un aller-retour réseau.
    let warm_app = app.clone();
    tauri::async_runtime::spawn(async move { cues::warm(&warm_app).await });

    let handle = state.window.clone();
    let avatar = app.avatar.clone();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<AgentEvent>(64);

    let relay = tauri::async_runtime::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let Some(window) = &handle {
                let _ = window.emit("agent-event", &event);
            }
            match &event {
                AgentEvent::State { state, detail } => {
                    let _ = avatar.set_state(*state, detail).await;
                }
                AgentEvent::Final { text } => {
                    avatar.say(text, 0).await;
                }
                _ => {}
            }
        }
    });

    let app_for_loop = app.clone();
    let runtime = runtime.inner().clone();
    tauri::async_runtime::spawn(async move {
        let _ = app_for_loop.spawn_voice_listener(runtime, tx).await;
        relay.abort();
    });
    Ok(())
}

/// Coupe l'écoute et libère le relais d'événements.
#[tauri::command]
pub async fn voice_stop(
    runtime: State<'_, Arc<VoiceRuntime>>,
) -> std::result::Result<(), String> {
    runtime.stop();
    Ok(())
}

/// Transcrit un WAV 16 kHz envoyé par l'interface (onboarding, test du micro).
#[tauri::command]
pub async fn stt_transcribe(state: State<'_, AppState>, wav: Vec<u8>) -> std::result::Result<String, String> {
    let app = state.app.clone();
    let guard = app.stt.lock().await;
    let stt = guard.as_ref().ok_or("la reconnaissance vocale n'est pas démarrée")?;
    stt.transcribe_wav(wav).await.map_err(err)
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
    match &app.synaptiq {
        Some(client) => checks.push(check(
            "Synaptiq",
            client.health().await.unwrap_or(false),
            "instance locale Synaptiq injoignable",
        )),
        None => checks.push(check(
            "Synaptiq",
            false,
            "SYNAPTIQ_API_KEY absente de .env",
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