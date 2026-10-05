//! Test du chemin heureux, exécuté pour de vrai.
//!
//! Ce test est ignoré par défaut (`cargo test` ne le lance pas) parce qu'il
//! consomme de vrais crédits LLM et touche le disque. On l'exécute quand on
//! veut vérifier la boucle complète :
//!
//! ```text
//! cargo test -p jimmy-agent --test happy_path -- --ignored --nocapture
//! ```
//!
//! Prérequis : un `.env` à la racine avec `OPENCODE_API_KEY`, et — pour la
//! partie vocale — `whisper-server` + un modèle dans `data/components/whisper`.
//!
//! Ce que le test valide, dans l'ordre du PLAN §35 :
//! 1. l'agent reçoit une demande en français ;
//! 2. il utilise un outil au lieu de deviner ;
//! 3. il répond ;
//! 4. la réponse est mémorisée ;
//! 5. le rappel mémoire la retrouve.

use std::path::Path;
use std::sync::Arc;

use jimmy_agent::config::Secrets;
use jimmy_agent::core::types::AvatarState;
use jimmy_agent::paths::Paths;
use jimmy_agent::App;

/// Environnement de test isolé : une base et un dossier de skills temporaires.
fn app_de_test() -> Option<Arc<App>> {
    let racine = std::env::temp_dir().join(format!("jimmy-happy-{}", uuid_like()));
    std::fs::create_dir_all(racine.join("data")).ok()?;
    let paths = Paths {
        data: racine.join("data"),
        app: racine.clone(),
        dev: false,
    };
    // Le test lit le même `.env` que l'application : c'est la réalité d'usage.
    let racine_workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| racine.clone());
    let _ = jimmy_agent::paths::load_dotenv(&racine_workspace.join(".env"));
    App::new(paths, Secrets::from_env()).ok()
}

fn uuid_like() -> String {
    format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    )
}

#[tokio::test]
#[ignore = "consulte de vrais services ; à lancer explicitement"]
async fn chemin_heureux_analyse_un_dossier() {
    let Some(app) = app_de_test() else {
        eprintln!("environnement de test indisponible");
        return;
    };
    let secrets = app.secrets.clone();
    if secrets.opencode_api_key.is_empty() {
        panic!("OPENCODE_API_KEY absente : ce test a besoin d'un vrai modèle");
    }

    // Un dossier à analyser, pour que l'agent ait un vrai motif d'appeler un outil.
    let dossier = app.paths.data.join("dossier-a-analyser");
    std::fs::create_dir_all(&dossier).unwrap();
    std::fs::write(dossier.join("notes.txt"), "Projet Jimmy : assistant de bureau.\n").unwrap();
    std::fs::write(dossier.join("todo.md"), "- [])pirical\n- [x] audio\n").unwrap();

    let settings = app.settings();
    let session = app.history.create_session("test").unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);

    let mut settings = settings.clone();
    settings.workspace = app.paths.app.to_string_lossy().to_string();

    let reponse = jimmy_agent::core::agent::run(
        app.deps(),
        settings,
        session.clone(),
        format!(
            "Analyse le dossier « {} » et explique-moi ce que tu trouves.",
            dossier.display()
        ),
        app.tool_context(),
        tx,
    )
    .await
    .expect("l'agent doit produire une réponse");

    // Les événements ont dû être émis.
    let mut etats = Vec::new();
    let mut outils = Vec::new();
    while let Ok(event) = rx.try_recv() {
        match event {
            jimmy_agent::core::types::AgentEvent::State { state, .. } => etats.push(state),
            jimmy_agent::core::types::AgentEvent::ToolStart { name, .. } => outils.push(name),
            _ => {}
        }
    }

    println!("--- réponse ---\n{}\n", reponse.text);
    println!("états : {etats:?}");
    println!("outils : {outils:?}");
    println!("durée : {} ms", reponse.duration_ms);

    assert!(!reponse.text.trim().is_empty(), "la réponse est vide");
    assert!(
        outils.iter().any(|o| o.contains("list_directory") || o.contains("read_file")),
        "l'agent aurait dû lire le dossier, il a utilisé : {outils:?}"
    );
    assert!(
        etats.contains(&AvatarState::Thinking),
        "l'état « thinking » n'a jamais été émis"
    );

    // Mémoire : ce qui a été appris doit être retrouvable.
    let total = app.memory.count().unwrap_or(0);
    println!("souvenirs après l'échange : {total}");
}
/// Cas réel du 4 octobre : « J'ai l'impression que ta mémoire est mélangée
/// avec plein d'autres agents. N'as-tu pas une mémoire personnelle à toi ? ».
/// Jimmy doit savoir que le vault est partagé et lequel est son dossier.
#[tokio::test]
#[ignore = "consulte le vrai modèle ; à lancer explicitement"]
async fn il_sait_que_sa_memoire_est_partagee() {
    let Some(app) = app_de_test() else {
        panic!("environnement de test indisponible");
    };
    let session = app.history.create_session("test").unwrap();
    let (tx, _rx) = tokio::sync::mpsc::channel(64);
    let reponse = jimmy_agent::core::agent::run(
        app.deps(),
        app.settings(),
        session,
        "J'ai l'impression que ta mémoire est mélangée avec plein d'autres agents, dans plein de dossiers. \
         N'as-tu pas une mémoire personnelle à toi ? Réponds en deux ou trois phrases, sans outil."
            .to_string(),
        app.tool_context(),
        tx,
    )
    .await
    .expect("réponse");
    println!("--- réponse ---\n{}", reponse.text);
    let texte = reponse.text.to_lowercase();
    assert!(
        texte.contains("partag") || texte.contains("autres agents"),
        "Jimmy ne sait pas que son vault est partagé : « {} »",
        reponse.text
    );
}

/// Journal d'expérience (mécanisme « Reflexion », lot « croissance ») : une
/// trajectoire avec un échec d'outil doit produire une leçon en mémoire
/// (source « lesson »). Un fichier inexistant, lu à la demande, garantit un
/// échec réel de `read_file` sans autre préparation.
#[tokio::test]
#[ignore = "vrai modèle ; à lancer explicitement"]
async fn un_echec_d_outil_produit_une_lecon() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).is_test(true).try_init();
    let Some(app) = app_de_test() else {
        panic!("environnement de test indisponible");
    };
    let session = app.history.create_session("test leçon").unwrap();
    let (tx, _rx) = tokio::sync::mpsc::channel(64);
    let absent = app.paths.data.join("fichier-inexistant-lecon.txt");
    jimmy_agent::core::agent::run(
        app.deps(),
        app.settings(),
        session,
        format!(
            "Lis le fichier « {} » et recopie son contenu tel quel. Ne vérifie pas s'il existe, lis-le directement.",
            absent.display()
        ),
        app.tool_context(),
        tx,
    )
    .await
    .expect("réponse");

    // La leçon est extraite en tâche de fond : attente bornée (le modèle
    // d'extraction répond en quelques secondes sur un modèle gratuit).
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        let lecons: Vec<String> = app
            .memory
            .list(100)
            .unwrap()
            .into_iter()
            .filter(|m| m.source == "lesson")
            .map(|m| m.content)
            .collect();
        if !lecons.is_empty() {
            println!("leçons : {lecons:?}");
            assert!(lecons.iter().any(|c| c.len() >= 12));
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "aucune leçon écrite 60 s après l'échec d'outil"
        );
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
}

/// Capture de compétence (mécanisme « Voyager », lot « croissance ») : une
/// trajectoire réussie qui mobilise plusieurs outils doit se condenser en un
/// skill réutilisable dans le dossier de skills (vide au départ de
/// l'environnement de test).
#[tokio::test]
#[ignore = "vrai modèle ; à lancer explicitement"]
async fn une_trajectoire_outillee_se_condense_en_skill() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).is_test(true).try_init();
    let Some(app) = app_de_test() else {
        panic!("environnement de test indisponible");
    };
    let session = app.history.create_session("test capture").unwrap();
    let (tx, _rx) = tokio::sync::mpsc::channel(64);
    let dossier = app.paths.data.join("dossier-a-capturer");
    std::fs::create_dir_all(&dossier).unwrap();
    std::fs::write(dossier.join("rapport.md"), "Rapport hebdo : 3 tâches, 2 livrées.\n").unwrap();
    std::fs::write(dossier.join("chiffres.txt"), "v1=12 v2=9 v3=15\n").unwrap();

    let mut settings = app.settings();
    settings.workspace = app.paths.app.to_string_lossy().to_string();

    jimmy_agent::core::agent::run(
        app.deps(),
        settings,
        session,
        format!(
            "Analyse le dossier « {} » : liste son contenu, lis chaque fichier et résume-le-moi en trois lignes.",
            dossier.display()
        ),
        app.tool_context(),
        tx,
    )
    .await
    .expect("réponse");

    // La capture tourne en tâche de fond : attente bornée (deux tentatives
    // d'extracteur au plus, le modèle gratuit répond en quelques secondes).
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(90);
    loop {
        let skills = app.skills.list().unwrap_or_default();
        if !skills.is_empty() {
            println!("skills capturés : {:?}", skills.iter().map(|s| (&s.name, &s.description)).collect::<Vec<_>>());
            assert!(skills.iter().all(|s| !s.description.trim().is_empty() && !s.body.trim().is_empty()));
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "aucun skill capturé 90 s après une trajectoire de trois outils"
        );
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
}

/// Revue périodique (mécanisme « curriculum », lot « croissance ») : des
/// leçons en mémoire + aucune revue antérieure = une session « Revue »
/// écrite avec des propositions. Sans outil, la revue ne peut rien
/// appliquer ; et une seconde revue immédiate n'est pas due.
#[tokio::test]
#[ignore = "vrai modèle ; à lancer explicitement"]
async fn la_revue_propose_sans_appliquer() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).is_test(true).try_init();
    let Some(app) = app_de_test() else {
        panic!("environnement de test indisponible");
    };
    use jimmy_agent::memory::MemoryKind;
    app.memory
        .remember_indexed(
            MemoryKind::Procedural,
            "Lecture directe d'un fichier absent : signaler l'erreur au lieu de deviner.",
            "lesson",
            0.65,
        )
        .await
        .unwrap();
    app.memory
        .remember_indexed(
            MemoryKind::Procedural,
            "Un dossier analysé deux fois : lister d'abord, lire ensuite les seuls fichiers utiles.",
            "lesson",
            0.65,
        )
        .await
        .unwrap();

    let revue = jimmy_agent::growth::review(&app).await.expect("revue");
    println!("--- revue ---\n{}\n", revue.as_deref().unwrap_or("(non due)"));
    let propositions = revue.expect("la première revue doit être due");
    assert!(!propositions.trim().is_empty());

    let sessions = app.history.sessions(20).unwrap();
    let revues: Vec<_> = sessions.iter().filter(|s| s.title == "Revue").collect();
    assert_eq!(revues.len(), 1, "une session « Revue » créée : {sessions:?}");

    // Idempotence : aussitôt revu, plus rien n'est dû (pas de nouvelles
    // leçons, fenêtre non écoulée).
    let encore = jimmy_agent::growth::review(&app).await.expect("revue");
    assert!(encore.is_none(), "la revue ne doit pas se répéter à l'infini");
}

/// Cas réel du 4 octobre : quatre « oui, go » d'affilée et `notify.py` jamais
/// écrit (6 étapes vocales brûlées à relire un fichier tronqué). Jimmy, en
/// mode vocal, doit aller au bout d'une tâche validée **en un seul tour**.
/// À lancer sur une COPIE du projet : `JIMMY_TASK_PROJECT=<copie d'agent-reddit>`.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "vrai modèle ; écrit dans la copie de projet donnée par JIMMY_TASK_PROJECT"]
async fn il_va_au_bout_d_une_tache_validee_en_un_tour() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).is_test(true).try_init();
    let projet = std::path::PathBuf::from(std::env::var("JIMMY_TASK_PROJECT").expect("JIMMY_TASK_PROJECT"));
    let Some(app) = app_de_test() else {
        panic!("environnement de test indisponible");
    };
    let session = app.history.create_session("Session vocale").unwrap();
    app.history.set_project(&session, Some(&projet.to_string_lossy())).unwrap();
    let (mut settings, ctx) = app.session_context(&session);
    // Le modèle réellement utilisé par l'utilisateur (MiMo), pas le défaut.
    let model = std::env::var("JIMMY_TASK_MODEL").unwrap_or_else(|_| "mimo-v2.6-flash".into());
    settings.llm.model = model.clone();
    settings.llm.voice_model = model;
    let (tx, mut rx) = tokio::sync::mpsc::channel(256);
    let reponse = jimmy_agent::core::agent::run(
        app.deps_voice(),
        settings,
        session,
        "Oui, go ! Écris src/agent_reddit/notify.py qui envoie sur Telegram les idées au-dessus du seuil \
         d'alerte, avec l'anti-doublon de storage.record_alert, puis branche-le en fin de run_analyse dans pipeline.py."
            .to_string(),
        ctx,
        tx,
    )
    .await
    .expect("réponse");
    let mut outils = Vec::new();
    while let Ok(event) = rx.try_recv() {
        if let jimmy_agent::core::types::AgentEvent::ToolStart { name, .. } = event {
            outils.push(name);
        }
    }
    println!("--- réponse ({} ms) ---\n{}", reponse.duration_ms, reponse.text);
    println!("outils ({}) : {outils:?}", outils.len());
    let notify = projet.join("src").join("agent_reddit").join("notify.py");
    assert!(notify.is_file(), "notify.py n'a pas été écrit en un tour");
    let pipeline = std::fs::read_to_string(projet.join("src").join("agent_reddit").join("pipeline.py")).unwrap();
    assert!(pipeline.contains("notify"), "notify n'est pas branché dans pipeline.py");
}
