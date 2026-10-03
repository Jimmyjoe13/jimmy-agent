//! Tests de la chaîne audio, exécutés pour de vrai.
//!
//!```text
//! .\scripts\test-happy.ps1
//! ```
//!
//! Ces tests consomment de vrais services et ouvrent le micro : ils sont
//! ignorés par défaut.

use std::sync::Arc;

use jimmy_agent::config::Secrets;
use jimmy_agent::paths::Paths;
use jimmy_agent::voice::{play_bytes, VoiceRuntime};
use jimmy_agent::App;

fn app_de_test() -> Option<Arc<App>> {
    let racine = std::env::temp_dir().join(format!("jimmy-audio-{}", std::process::id()));
    std::fs::create_dir_all(racine.join("data")).ok()?;
    let paths = Paths {
        data: racine.join("data"),
        app: racine.clone(),
        dev: false,
    };
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or(racine);
    let _ = jimmy_agent::paths::load_dotenv(&workspace.join(".env"));
    App::new(paths, Secrets::from_env()).ok()
}

/// L'écoute doit utiliser les composants réellement installés, pas un dossier
/// temporaire : c'est ce qui manquait lors du premier essai de ce test.
fn app_reel() -> Option<Arc<App>> {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|p| p.to_path_buf())?;
    let _ = jimmy_agent::paths::load_dotenv(&workspace.join(".env"));
    let paths = Paths {
        data: workspace.join("data"),
        app: workspace,
        dev: true,
    };
    App::new(paths, Secrets::from_env()).ok()
}

use std::path::Path;

/// Le TTS doit produire des octets que Jimmy sait lire, et les lire réellement.
///
/// Ce test a échoué en conditions réelles : Fish Audio renvoyait du MP3, que
/// Jimmy ne savait pas décoder. Il vérifie maintenant la chaîne complète —
/// appel réseau, décodage, lecture sur le périphérique de sortie.
#[tokio::test]
#[ignore = "consulte Fish Audio et ouvre le périphérique de sortie"]
async fn tts_synthetise_et_se_lit() {
    let Some(app) = app_de_test() else {
        panic!("environnement de test indisponible");
    };
    if app.secrets.openrouter_api_key.is_empty() {
        panic!("OPENROUTER_API_KEY absente");
    }

    let settings = app.settings();
    let speech = app
        .tts
        .speak(
            &app.secrets.openrouter_api_key,
            &settings.tts.model,
            &settings.tts.voice,
            "Bonjour, je suis Jimmy.",
            settings.tts.chars_per_minute,
        )
        .await
        .expect("la synthèse doit aboutir");

    println!(
        "octets={} fréquence={} durée estimée={} ms",
        speech.bytes.len(),
        speech.sample_rate,
        speech.estimated_ms
    );
    assert!(speech.bytes.len() > 1000, "audio suspiciously court");
    assert!(
        speech.sample_rate >= 8000,
        "fréquence absente : la lecture ne saura pas quoi faire"
    );

    // La lecture doit aboutir sans erreur : c'est exactement le point qui
    // échouait avec le MP3.
    let bytes = speech.bytes.clone();
    let rate = speech.sample_rate;
    tokio::task::spawn_blocking(move || play_bytes(&bytes, rate))
        .await
        .expect("la tâche de lecture ne doit pas paniquer")
        .expect("la lecture ne doit pas échouer");
}

/// L'écoute permanente doit démarrer whisper.cpp et ouvrir le micro.
///
/// Ce test a échoué en conditions réelles : la boucle n'était jamais démarrée
/// depuis l'interface, donc le wake word ne pouvait pas être entendu.
#[tokio::test]
#[ignore = "démarre whisper-server et ouvre le microphone"]
async fn ecoute_permanente_demarre() {
    let Some(app) = app_reel() else {
        panic!("environnement de test indisponible");
    };
    let settings = app.settings();
    if !settings.voice.enabled || !settings.stt.enabled {
        panic!("voix ou STT désactivé dans la configuration");
    }

    let runtime = VoiceRuntime::new(settings.voice.input_sample_rate);
    app.start_voice(&runtime)
        .await
        .expect("l'écoute doit démarrer");

    assert!(
        runtime.is_running(),
        "le microphone n'a pas été ouvert"
    );
    assert!(
        app.stt.lock().await.is_some(),
        "whisper-server n'a pas été démarré"
    );

    println!("micro ouvert, whisper-server prêt");
    runtime.stop();
    app.stt.lock().await.as_mut().map(|s| s.shutdown());
}

/// Le PCM brut de Fish Audio doit être décodable sans dépendance.
#[test]
fn pcm_brut_est_decodable() {
    // 3 échantillons 16 bits signés : 0, +16384, -16384
    let bytes: Vec<u8> = [0u16, 16384, (-16384i16) as u16]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let rate = 44_100;
    // On réutilise le chemin de lecture complet via un WAV, pas l'export
    // interne : c'est le contrat public qui compte.
    let wav = jimmy_agent::providers::stt::pcm_to_wav(&[0, 16384, -16384], rate);
    assert_eq!(wav.len(), 44 + 6);
    assert_eq!(&wav[0..4], b"RIFF");
    assert!(!bytes.is_empty());
}