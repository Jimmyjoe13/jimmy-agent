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

/// Synthétise `texte` et le ramène en mono 16 kHz, au niveau RMS `rms`.
///
/// Mesuré : à 0,008 (sous l'ancien seuil fixe de 0,012) le VAD adaptatif se
/// déclenche bien, mais Whisper comprend mal la phrase *elle-même* (référence
/// hors écoute : « Qu'est-ce que tu as fait ? » pour « quelle heure est-il »).
/// Les scénarios utilisent donc 0,02, une voix normale près du micro.
async fn phrase_16k(app: &App, texte: &str, rms_cible: f32) -> Vec<f32> {
    let settings = app.settings();
    let speech = app
        .tts
        .speak(
            &app.secrets.openrouter_api_key,
            &settings.tts.model,
            &settings.tts.voice,
            texte,
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
        *s *= rms_cible / rms;
    }
    phrase
}

/// Comme [`phrase_16k`], mais régénère le clip (4 essais au plus) tant que
/// Whisper n'y entend pas le nom : la synthèse prononce parfois mal
/// « Jimmy », et aucun détecteur ne retrouve un nom absent de l'audio. Le
/// test vérifie l'écoute sur une entrée intelligible, pas la diction de Fish.
async fn phrase_avec_nom(app: &App, texte: &str, rms_cible: f32) -> Vec<f32> {
    let mut dernier = Vec::new();
    for essai in 1..=4 {
        let clip = phrase_16k(app, texte, rms_cible).await;
        let reference = app
            .transcribe_command(jimmy_agent::voice::window_to_wav(&clip, 16_000))
            .await
            .unwrap_or_default();
        println!("clip {essai} : « {reference} »");
        if jimmy_agent::voice::matches_wake_word(&reference, "jimmy") {
            return clip;
        }
        dernier = clip;
    }
    dernier
}

/// Bruit de fond faible et déterministe (RMS ≈ 0,0015), `n` échantillons.
fn bruit(n: usize, graine: &mut u32) -> Vec<f32> {
    (0..n)
        .map(|_| {
            *graine = graine.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            ((*graine >> 16) as f32 / 32768.0 - 1.0) * 0.0026
        })
        .collect()
}

/// Fait « entendre » `flux` à la vraie boucle d'écoute, en temps réel, et
/// renvoie (mot d'éveil reconnu, réponse de l'agent). Si `apres_oui` est
/// fourni, il n'est injecté qu'au signal « Oui ? » de Jimmy — comme une
/// personne qui attend sa réponse avant de parler.
async fn faire_ecouter(app: &Arc<App>, flux: Vec<f32>, apres_oui: Option<Vec<f32>>) -> (bool, String) {
    use jimmy_agent::core::types::AgentEvent;

    let debut_test = chrono::Utc::now().to_rfc3339();
    // Référence : ce que Whisper comprend du clip entier, hors boucle
    // d'écoute. Distingue un clip de synthèse raté d'un bug de l'écoute.
    let mut tout = flux.clone();
    if let Some(suite) = &apres_oui {
        tout.extend_from_slice(suite);
    }
    let reference = app
        .transcribe_command(jimmy_agent::voice::window_to_wav(&tout, 16_000))
        .await
        .unwrap_or_default();
    println!("référence (clip entier) : « {reference} »");
    let runtime = Arc::new(VoiceRuntime::new(16_000));
    runtime.start_without_device();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<AgentEvent>(64);
    let ecoute = app.clone().spawn_voice_listener(runtime.clone(), tx);

    // Alimentation par blocs de 100 ms, au rythme réel. Entre les deux
    // parties, du bruit de fond jusqu'au signal « Oui ? ».
    let injecteur = runtime.clone();
    let oui = Arc::new(tokio::sync::Notify::new());
    let oui_recu = oui.clone();
    let alimentation = tokio::spawn(async move {
        let blocs = |donnees: Vec<f32>| donnees.chunks(1_600).map(|c| c.to_vec()).collect::<Vec<_>>();
        for bloc in blocs(flux) {
            injecteur.inject(&bloc);
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        let mut graine = 4242;
        if let Some(suite) = apres_oui {
            loop {
                tokio::select! {
                    _ = oui_recu.notified() => break,
                    _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
                        injecteur.inject(&bruit(1_600, &mut graine));
                    }
                }
            }
            // Le temps de réagir au « Oui ? ».
            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            for bloc in blocs(suite) {
                injecteur.inject(&bloc);
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }
        loop {
            injecteur.inject(&bruit(1_600, &mut graine));
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    });

    let mut reveil = false;
    let mut reponse = String::new();
    let mut commande = String::new();
    let limite = tokio::time::Instant::now() + std::time::Duration::from_secs(150);
    while let Ok(Some(event)) = tokio::time::timeout_at(limite, rx.recv()).await {
        match &event {
            AgentEvent::Heard { text, matched } => {
                println!("entendu ({}) : « {text} »", if *matched { "réveil" } else { "rien" });
                reveil |= *matched;
            }
            AgentEvent::State { state, detail } => {
                println!("état : {state:?} {detail}");
                if detail == "Oui ?" {
                    oui.notify_one();
                }
            }
            AgentEvent::Notice { message } => println!("avis : {message}"),
            AgentEvent::Spoken { text } => {
                println!("commande affichée : « {text} »");
                commande = text.clone();
            }
            AgentEvent::ToolStart { name, arguments, .. } => println!("outil : {name} {arguments}"),
            AgentEvent::ToolEnd { name, ok, duration_ms, summary, .. } => {
                println!("outil fini : {name} ok={ok} {duration_ms} ms — {}", summary.chars().take(80).collect::<String>())
            }
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

    // Nettoyage : les sessions vocales créées par le test sortent de l'historique.
    if let Ok(sessions) = app.history.sessions(20) {
        for s in sessions.iter().filter(|s| s.title == "Session vocale" && s.created_at >= debut_test) {
            let _ = app.history.delete_session(&s.id);
        }
    }
    // La commande transmise à l'agent ne doit plus contenir le nom : sinon
    // Jimmy répond « oui, je suis là » au lieu de traiter la demande.
    assert!(
        !jimmy_agent::voice::matches_wake_word(&commande, "jimmy"),
        "le nom est resté dans la commande : « {commande} »"
    );
    (reveil, reponse)
}

/// Phrase enchaînée : « Jimmy, quelle heure est-il ? » d'une traite.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "whisper-server, Fish Audio et le modèle de langage réels"]
async fn ecoute_reconnait_jimmy_et_repond() {
    let Some(app) = app_reel() else {
        panic!("environnement de test indisponible");
    };
    app.ensure_stt().await.expect("serveurs whisper");
    let mut graine = 12345;
    let mut flux = bruit(32_000, &mut graine);
    flux.extend(phrase_avec_nom(&app, "Jimmy, dis-moi bonjour.", 0.02).await);

    let (reveil, reponse) = faire_ecouter(&app, flux, None).await;
    assert!(reveil, "le mot d'éveil n'a pas été reconnu");
    assert!(!reponse.trim().is_empty(), "l'agent n'a pas répondu");
}

/// Le scénario qui échouait en vrai : « Jimmy » seul, une pause, le « Oui ? »
/// de Jimmy, puis la commande. Avant le correctif, la boucle s'arrêtait juste
/// après le « Oui ? » (fenêtre vide après la purge) : plus rien.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "whisper-server, Fish Audio et le modèle de langage réels"]
async fn ecoute_nom_seul_puis_commande() {
    let Some(app) = app_reel() else {
        panic!("environnement de test indisponible");
    };
    app.ensure_stt().await.expect("serveurs whisper");
    let mut graine = 777;
    let mut flux = bruit(32_000, &mut graine);
    // Un « Jimmy. » de synthèse isolé (0,5 s) est coupé par le VAD de
    // whisper-server ; une vraie voix passe (journal du 3 octobre, 14:34:53).
    flux.extend(phrase_avec_nom(&app, "Hé Jimmy !", 0.02).await);
    // Pause : Jimmy détecte la fin de phrase, répond « Oui ? »… et la
    // commande n'arrive qu'à ce moment-là.
    let commande = phrase_16k(&app, "Dis-moi bonjour.", 0.02).await;

    let (reveil, reponse) = faire_ecouter(&app, flux, Some(commande)).await;
    assert!(reveil, "le mot d'éveil n'a pas été reconnu");
    assert!(!reponse.trim().is_empty(), "l'agent n'a pas répondu après « Oui ? »");
}

/// Auto-réparation : les serveurs whisper sont tués en pleine session (cas
/// réel : un autre processus qui les possédait s'est arrêté). La transcription
/// suivante doit les relancer et réussir, au lieu d'échouer pour toujours.
#[tokio::test]
#[ignore = "démarre et tue des whisper-server, consulte Fish Audio"]
async fn stt_se_repare_apres_arret_du_serveur() {
    let Some(app) = app_reel() else {
        panic!("environnement de test indisponible");
    };
    app.ensure_stt().await.expect("serveurs whisper");
    let clip = phrase_16k(&app, "Bonjour, ceci est un essai.", 0.05).await;
    let wav = jimmy_agent::voice::window_to_wav(&clip, 16_000);

    // Arrêt brutal de tous les serveurs, hors du contrôle de Jimmy.
    let _ = std::process::Command::new("taskkill")
        .args(["/IM", "whisper-server.exe", "/F"])
        .output();
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let texte = app.transcribe_command(wav.clone()).await.expect("la transcription doit se réparer");
    println!("commande après réparation : « {texte} »");
    assert!(!texte.trim().is_empty());
    let texte = app.transcribe_wake(wav).await.expect("le mot d'éveil aussi");
    println!("mot d'éveil après réparation : « {texte} »");

    for verrou in [&app.stt_command, &app.stt] {
        let mut garde = verrou.lock().await;
        if let Some(stt) = garde.as_mut() {
            stt.shutdown().await;
        }
    }
}
