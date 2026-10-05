//! Arrêt d'urgence (« STOP », bouton « Arrêter ») : une tâche en cours est
//! abandonnée tout de suite, ses commandes tuées, et Jimmy n'est plus occupé.

use std::sync::Arc;
use std::time::{Duration, Instant};

use jimmy_agent::config::Secrets;
use jimmy_agent::error::Error;
use jimmy_agent::paths::Paths;
use jimmy_agent::App;

fn app_isolee() -> (Arc<App>, std::path::PathBuf) {
    let racine = std::env::temp_dir().join(format!("jimmy-stop-{}", std::process::id()));
    std::fs::create_dir_all(racine.join("data")).unwrap();
    let paths = Paths { data: racine.join("data"), app: racine.clone(), dev: false };
    (App::new(paths, Secrets::default()).expect("app"), racine)
}

#[tokio::test]
async fn une_tache_en_cours_s_arrete_tout_de_suite() {
    let (app, racine) = app_isolee();
    assert!(!app.is_busy());

    // Une « tâche » de 30 s, arrêtée au bout de 200 ms.
    let stopper = app.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(stopper.is_busy(), "la tâche doit être comptée pendant qu'elle tourne");
        assert!(stopper.request_stop(), "il y avait quelque chose à arrêter");
    });
    let started = Instant::now();
    let outcome: jimmy_agent::error::Result<()> = app
        .cancellable(async {
            tokio::time::sleep(Duration::from_secs(30)).await;
            Ok(())
        })
        .await;
    assert!(matches!(outcome, Err(Error::Cancelled)), "{outcome:?}");
    assert!(started.elapsed() < Duration::from_secs(2), "arrêt trop lent : {:?}", started.elapsed());
    assert!(!app.is_busy(), "plus rien ne doit tourner après l'arrêt");

    // Un arrêt demandé AVANT une tâche ne l'empêche pas de démarrer.
    let next: jimmy_agent::error::Result<u8> = app.cancellable(async { Ok(7) }).await;
    assert_eq!(next.unwrap(), 7);
    // Rien en cours : l'arrêt n'a rien à faire.
    assert!(!app.request_stop());

    drop(app);
    let _ = std::fs::remove_dir_all(&racine);
}
