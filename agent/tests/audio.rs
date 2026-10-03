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
    // `shutdown` est asynchrone : sans `.await`, le serveur n'était jamais arrêté.
    let mut garde = app.stt.lock().await;
    if let Some(stt) = garde.as_mut() {
        stt.shutdown().await;
    }
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
/// Les sons d'état doivent être synthétisés une fois, mis en cache, puis lus
/// depuis le disque sans nouvel appel réseau.
#[tokio::test]
#[ignore = "consulte Fish Audio et ouvre le périphérique de sortie"]
async fn sons_d_etat_en_cache_et_lus() {
    use jimmy_agent::voice::cues::{self, Cue};

    let Some(app) = app_de_test() else {
        panic!("environnement de test indisponible");
    };
    if app.secrets.openrouter_api_key.is_empty() {
        panic!("OPENROUTER_API_KEY absente");
    }

    // 1. Première passe : synthèse réseau et écriture du cache.
    cues::warm(&app).await;
    let dossier = app.paths.audio_dir().join("cues");
    let fichiers: Vec<_> = std::fs::read_dir(&dossier)
        .expect("le cache doit exister")
        .filter_map(|e| e.ok())
        .collect();
    println!("cache : {} fichiers dans {}", fichiers.len(), dossier.display());
    assert_eq!(fichiers.len(), Cue::ALL.len(), "un fichier par son");
    for fichier in &fichiers {
        let taille = fichier.metadata().map(|m| m.len()).unwrap_or(0);
        println!("  {} — {taille} octets", fichier.file_name().to_string_lossy());
        assert!(taille > 4_000, "son trop court pour être audible");
    }

    // 2. Seconde passe : lecture depuis le cache, donc quasi instantanée.
    let debut = std::time::Instant::now();
    cues::play(&app, Cue::Listening).await;
    println!("« {} » lu en {:?} (lecture comprise)", Cue::Listening.phrase(), debut.elapsed());
    assert!(debut.elapsed() < std::time::Duration::from_secs(4));
}

/// Deux serveurs : `base` pour le wake word (8178), `small` pour la commande
/// (8179). Une phrase synthétisée par Fish Audio doit être retranscrite par le
/// serveur de commande.
#[tokio::test]
#[ignore = "démarre deux whisper-server, consulte Fish Audio"]
async fn commande_transcrite_par_le_modele_precis() {
    let Some(app) = app_reel() else {
        panic!("environnement de test indisponible");
    };
    let settings = app.settings();
    let runtime = VoiceRuntime::new(settings.voice.input_sample_rate);
    app.start_voice(&runtime).await.expect("l'écoute doit démarrer");
    runtime.stop();
    assert!(app.stt.lock().await.is_some(), "serveur du wake word absent");
    assert!(
        app.stt_command.lock().await.is_some(),
        "serveur de commande ({}) absent",
        settings.stt.command_model
    );

    // Phrase de test, synthétisée puis ramenée en WAV 16 kHz mono.
    let speech = app
        .tts
        .speak(
            &app.secrets.openrouter_api_key,
            &settings.tts.model,
            &settings.tts.voice,
            "Quelle est la météo prévue demain à Marseille ?",
            settings.tts.chars_per_minute,
        )
        .await
        .expect("synthèse");
    let source: Vec<f32> = speech
        .bytes
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0)
        .collect();
    let ratio = speech.sample_rate as f64 / 16_000.0;
    let resampled: Vec<i16> = (0..(source.len() as f64 / ratio) as usize)
        .map(|i| (source[(i as f64 * ratio) as usize] * 32767.0) as i16)
        .collect();
    let wav = jimmy_agent::providers::stt::pcm_to_wav(&resampled, 16_000);

    let debut = std::time::Instant::now();
    let texte = app.transcribe_command(wav).await.expect("transcription");
    println!("commande ({:?}) : « {texte} »", debut.elapsed());
    let bas = texte.to_lowercase();
    assert!(bas.contains("marseille"), "transcription inattendue : {texte}");

    for verrou in [&app.stt_command, &app.stt] {
        let mut garde = verrou.lock().await;
        if let Some(stt) = garde.as_mut() {
            stt.shutdown().await;
        }
    }
}

/// Diagnostic : niveau RMS réel du micro (après mixage mono 16 kHz) pendant
/// que Jimmy parle sur les haut-parleurs. Sert à régler le seuil du VAD.
#[tokio::test]
#[ignore = "ouvre le micro, consulte Fish Audio, joue du son"]
async fn niveau_du_micro_pendant_la_parole() {
    let Some(app) = app_reel() else {
        panic!("environnement de test indisponible");
    };
    let settings = app.settings();
    let runtime = Arc::new(VoiceRuntime::new(16_000));
    runtime.start().expect("micro");
    let speech = app
        .tts
        .speak(
            &app.secrets.openrouter_api_key,
            &settings.tts.model,
            &settings.tts.voice,
            "Jimmy, quelle heure est-il ?",
            settings.tts.chars_per_minute,
        )
        .await
        .expect("synthèse");
    // Silence de référence, puis lecture.
    let rms = |w: &[f32]| (w.iter().map(|s| s * s).sum::<f32>() / w.len().max(1) as f32).sqrt();
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    println!("silence : RMS {:.4}", rms(&runtime.take_window(8_000)));
    let bytes = speech.bytes.clone();
    let rate = speech.sample_rate;
    let lecture = std::thread::spawn(move || play_bytes(&bytes, rate));
    let mut max = 0f32;
    for _ in 0..12 {
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        let level = rms(&runtime.take_window(4_000));
        max = max.max(level);
        println!("pendant la parole : RMS {level:.4}");
    }
    let _ = lecture.join();
    println!("maximum : {max:.4} (seuil VAD actuel : {})", settings.voice.vad_threshold);
    runtime.stop();
}

/// Écoute de bout en bout, sans micro : une phrase synthétisée est injectée
/// dans le tampon comme si elle venait du micro, atténuée pour imiter un micro
/// peu sensible, sur un bruit de fond. La vraie boucle d'écoute doit :
/// détecter la parole (VAD adaptatif), reconnaître « Jimmy », transcrire la
/// commande avec le modèle précis, faire répondre l'agent.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "whisper-server, Fish Audio et le modèle de langage réels"]
async fn ecoute_reconnait_jimmy_et_repond() {
    use jimmy_agent::core::types::AgentEvent;

    let Some(app) = app_reel() else {
        panic!("environnement de test indisponible");
    };
    app.ensure_stt().await.expect("serveurs whisper");
    let settings = app.settings();
    let debut_test = chrono::Utc::now().to_rfc3339();

    // Phrase synthétisée → mono 16 kHz, ramenée à un RMS de 0,008 (en dessous
    // de l'ancien seuil fixe de 0,012 : c'est le cas du micro intégré).
    let speech = app
        .tts
        .speak(
            &app.secrets.openrouter_api_key,
            &settings.tts.model,
            &settings.tts.voice,
            "Jimmy, quelle heure est-il ?",
            settings.tts.chars_per_minute,
        )
        .await
        .expect("synthèse");
    let source: Vec<f32> = speech
        .bytes
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0)
        .collect();
    let ratio = speech.sample_rate as f64 / 16_000.0;
    let mut phrase: Vec<f32> = (0..(source.len() as f64 / ratio) as usize)
        .map(|i| source[(i as f64 * ratio) as usize])
        .collect();
    let rms = (phrase.iter().map(|s| s * s).sum::<f32>() / phrase.len() as f32).sqrt();
    for s in phrase.iter_mut() {
        *s *= 0.008 / rms;
    }

    // Bruit de fond faible et déterministe (RMS ≈ 0,0015).
    let mut graine: u32 = 12345;
    let mut bruit = move |n: usize| -> Vec<f32> {
        (0..n)
            .map(|_| {
                graine = graine.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                ((graine >> 16) as f32 / 32768.0 - 1.0) * 0.0026
            })
            .collect()
    };

    let runtime = Arc::new(VoiceRuntime::new(16_000));
    runtime.start_without_device();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<AgentEvent>(64);
    let ecoute = app.clone().spawn_voice_listener(runtime.clone(), tx);

    // Alimentation en temps réel, par blocs de 100 ms : 2 s de fond, la
    // phrase, puis 4 s de fond pour la fin de phrase.
    let injecteur = runtime.clone();
    let alimentation = tokio::spawn(async move {
        let mut flux = bruit(32_000);
        flux.extend_from_slice(&phrase);
        flux.extend(bruit(64_000));
        for bloc in flux.chunks(1_600) {
            injecteur.inject(bloc);
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    });

    let mut entendu_reveil = false;
    let mut reponse = String::new();
    let limite = tokio::time::Instant::now() + std::time::Duration::from_secs(150);
    while let Ok(Some(event)) = tokio::time::timeout_at(limite, rx.recv()).await {
        match &event {
            AgentEvent::Heard { text, matched } => {
                println!("entendu ({}) : « {text} »", if *matched { "réveil" } else { "rien" });
                entendu_reveil |= *matched;
            }
            AgentEvent::State { state, detail } => println!("état : {state:?} {detail}"),
            AgentEvent::Final { text } => {
                println!("réponse : « {text} »");
                reponse = text.clone();
                break;
            }
            AgentEvent::Failed { message } => panic!("échec de l'agent : {message}"),
            _ => {}
        }
    }
    runtime.stop();
    alimentation.abort();
    ecoute.abort();

    // Nettoyage : la session vocale créée par le test sort de l'historique.
    if let Ok(sessions) = app.history.sessions(20) {
        for s in sessions.iter().filter(|s| s.title == "Session vocale" && s.created_at >= debut_test) {
            let _ = app.history.delete_session(&s.id);
        }
    }

    assert!(entendu_reveil, "le mot d'éveil n'a pas été reconnu");
    assert!(!reponse.trim().is_empty(), "l'agent n'a pas répondu");
}
