//! Application de bureau Jimmy.
//!
//! Rôle de ce binaire : assembler la fenêtre, le pont HTTP et le crate
//! `jimmy-agent`, puis exposer des commandes fines à l'interface. Toute la
//! logique reste dans le crate agent, ce qui la rend testable sans fenêtre.

mod bridge;
mod commands;

use std::sync::Arc;

use jimmy_agent::config::Secrets;
use jimmy_agent::paths::{load_dotenv, Paths};
use jimmy_agent::voice::VoiceRuntime;
use tauri::{Manager, WebviewWindow};

/// État partagé entre les commandes Tauri.
///
/// `Clone` est exigé par `axum`, qui clone l'état à chaque requête du pont.
#[derive(Clone)]
pub struct AppState {
    pub app: Arc<jimmy_agent::App>,
    pub app_handle: tauri::AppHandle,
    /// Fenêtre principale, si elle existe au moment de la commande.
    pub window: Option<WebviewWindow>,
}

/// Point d'entrée de la fenêtre principale.
///
/// La fenêtre démarre **cachée** : Jimmy doit apparaître comme un personnage
/// sur le bureau, pas comme une fenêtre qui clignote. C'est l'avatar Godot qui
/// attire l'attention ; la fenêtre complète n'apparaît que lorsque l'utilisateur
/// le demande (clic sur l'avatar, ou bouton de la bulle).
#[tauri::command]
fn show_main(state: tauri::State<'_, AppState>) {
    if let Some(window) = &state.window {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Journal écrit à la fois sur stderr et dans `data/logs/jimmy.log`.
///
/// La release est une application fenêtrée, sans console : sur stderr seul,
/// tous les logs étaient perdus, et une panne de l'écoute était impossible à
/// diagnostiquer.
struct Tee(std::fs::File);

impl std::io::Write for Tee {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let _ = std::io::stderr().write_all(buf);
        self.0.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}

fn init_logging(paths: &Paths) {
    let mut builder = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"));
    let dir = paths.data.join("logs");
    let file = std::fs::create_dir_all(&dir).ok().and_then(|_| {
        let path = dir.join("jimmy.log");
        // Rotation minimale : au-delà de 5 Mo, l'ancien journal devient .1.
        if std::fs::metadata(&path).map(|m| m.len() > 5 * 1024 * 1024).unwrap_or(false) {
            let _ = std::fs::rename(&path, dir.join("jimmy.log.1"));
        }
        std::fs::OpenOptions::new().create(true).append(true).open(path).ok()
    });
    if let Some(file) = file {
        builder.target(env_logger::Target::Pipe(Box::new(Tee(file))));
    }
    builder.init();
}

pub fn run() {
    let paths = Paths::discover().expect("chemins Jimmy introuvables");
    init_logging(&paths);

    // Le `.env` est chargé avant tout : les secrets doivent exister avant la
    // construction de l'application, sinon les clients tourneraient à vide. Les
    // valeurs déjà présentes dans l'environnement gagnent.
    let dotenv = if paths.dev {
        paths.app.join(".env")
    } else {
        paths.data.join(".env")
    };
    if let Err(error) = load_dotenv(&dotenv) {
        log::warn!("[config] .env illisible : {error}");
    } else if !dotenv.is_file() {
        log::info!("[config] aucun .env — fonctionnement limité (voir .env.example)");
    }
    let secrets = Secrets::from_env();
    let missing = jimmy_agent::config::missing_secrets(&secrets);
    if !missing.is_empty() {
        log::warn!("[config] secrets manquants : {}", missing.join(", "));
    }

    let app = match jimmy_agent::App::new(paths, secrets) {
        Ok(app) => app,
        Err(error) => {
            eprintln!("Jimmy n'a pas pu démarrer : {error}");
            std::process::exit(1);
        }
    };

    tauri::Builder::default()
            .plugin(tauri_plugin_single_instance::init(|app, _, _| {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }))
            .plugin(tauri_plugin_autostart::init(
                tauri_plugin_autostart::MacosLauncher::LaunchAgent,
                None,
            ))
            .setup(move |handle| {
                let app_handle = handle.app_handle().clone();
                let settings = app.settings();

                let state = AppState {
                    app: app.clone(),
                    app_handle: app_handle.clone(),
                    window: handle.get_webview_window("main"),
                };
                let bridge_state = state.clone();

                // Le pont HTTP démarre avant la fenêtre : Godot peut ainsi
                // appeler dès son premier clic.
                if let Err(error) = tauri::async_runtime::block_on(bridge::start(
                    bridge_state,
                    &settings.avatar.bridge_host,
                    settings.avatar.bridge_port,
                )) {
                    log::warn!("[bridge] {error}");
                }

                let window_for_voice = state.window.clone();
                handle.manage(state);
                let runtime = Arc::new(VoiceRuntime::new(settings.voice.input_sample_rate));
                handle.manage(runtime.clone());

                // Reprise de l'écoute : si l'utilisateur l'avait laissée
                // active, Jimmy l'est de nouveau au lancement. Avant, il
                // restait sourd jusqu'à un passage par la vue Voix.
                if settings.voice.listen_on_start && settings.voice.enabled && settings.stt.enabled {
                    let app_for_voice = app.clone();
                    tauri::async_runtime::spawn(async move {
                        match commands::start_listening(app_for_voice, runtime, window_for_voice).await {
                            Ok(()) => log::info!("[voice] écoute reprise au lancement"),
                            Err(error) => log::warn!("[voice] reprise de l'écoute impossible : {error}"),
                        }
                    });
                }

                // L'avatar démarre ici, et non dans la commande `bootstrap` :
                // Jimmy doit être sur le bureau même si l'interface n'a pas
                // encore fini de charger, ou si elle échoue. C'est le seul
                // élément visible de l'application — tout le reste peut
                // attendre.
                if settings.avatar.enabled && settings.avatar.autostart {
                    let app_for_avatar = app.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Err(error) = app_for_avatar.start_avatar().await {
                            log::warn!("[avatar] {error}");
                        }
                    });
                }

                // Mémoire : vecteurs sémantiques manquants (LM Studio), en
                // tâche de fond. Sans LM Studio, s'arrête au premier essai.
                let app_for_memory = app.clone();
                tauri::async_runtime::spawn(async move {
                    let count = app_for_memory.memory.reindex_semantic().await;
                    if count > 0 {
                        log::info!("[memory] {count} souvenir(s) indexé(s) sémantiquement");
                    }
                });

                // Serveurs MCP : connectés en tâche de fond, leurs outils
                // rejoignent le registre dès qu'ils répondent.
                if !app.mcp.is_empty() {
                    let app_for_mcp = app.clone();
                    tauri::async_runtime::spawn(async move {
                        let count = app_for_mcp.start_mcp().await;
                        log::info!("[mcp] {count} outil(s) disponible(s)");
                    });
                }

                if !settings.ui.first_run_done {
                    // Premier lancement : on montre l'interface pour l'onboarding.
                    if let Some(window) = handle.get_webview_window("main") {
                        let _ = window.show();
                    }
                }

                // Fermer la fenêtre ne doit pas tuer Jimmy : il reste vivant
                // sur le bureau avec son avatar.
                let hide_handle = app_handle.clone();
                if let Some(window) = handle.get_webview_window("main") {
                    window.on_window_event(move |event| {
                        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                            api.prevent_close();
                            let _ = hide_handle.get_webview_window("main").map(|w| w.hide());
                        }
                    });
                }
                Ok(())
            })
            .invoke_handler(tauri::generate_handler![
                commands::bootstrap,
                commands::status,
                commands::chat,
                commands::sessions,
                commands::session_messages,
                commands::delete_session,
                commands::get_settings,
                commands::save_settings,
                commands::list_models,
                commands::complete_onboarding,
                commands::set_startup,
                commands::get_permissions,
                commands::save_permissions,
                commands::memory_list,
                commands::memory_forget,
                commands::skills_list,
                commands::skill_read,
                commands::tts_preview,
                commands::voice_devices,
                commands::voice_start,
                commands::voice_stop,
                commands::stt_transcribe,
                commands::avatar_start,
                commands::avatar_stop,
                commands::avatar_state,
                commands::avatar_say,
                commands::avatar_quality,
                commands::avatar_skin,
                commands::voice_status,
                commands::wake_word_test,
                commands::wake_word_strip,
                commands::doctor,
                commands::paths_info,
                commands::reload_secrets,
                commands::check_update,
                show_main,
            ])
            .run(tauri::generate_context!())
            .expect("échec du démarrage de la fenêtre Jimmy");
}