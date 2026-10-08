//! L'avatar Godot doit démarrer sur une installation fraîche, **sans** le
//! cache de l'éditeur (`godot/.godot/`, non versionné) : `main.gd` utilisait
//! les `class_name` globaux (`Jimmy`, `JimmyHttpServer`), inconnus sans ce
//! cache — la scène mourait à l'analyse, aucun serveur HTTP, avatar invisible
//! et fenêtre morte (cas réel du 8 octobre 2026, voir `HANDOFF.md` piège 86).
//!
//! Test ignoré : lance le vrai Godot en `headless` (aucune fenêtre), avec le
//! cache mis de côté s'il existe. `JIMMY_GODOT_EXE` reste prioritaire.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Racine du dépôt (`agent/tests/` → `agent/` → racine).
fn racine() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("agent/ a un parent")
        .to_path_buf()
}

/// Remet le cache de l'éditeur en place même si le test échoue.
struct RestaureCache {
    cache: PathBuf,
    repli: PathBuf,
    deplace: bool,
}

impl Drop for RestaureCache {
    fn drop(&mut self) {
        if self.deplace {
            let _ = std::fs::rename(&self.repli, &self.cache);
        }
    }
}

fn attendre_le_serveur(port: u16, delai: Duration) -> bool {
    let debut = Instant::now();
    while debut.elapsed() < delai {
        if std::net::TcpStream::connect_timeout(
            &format!("127.0.0.1:{port}").parse().unwrap(),
            Duration::from_millis(300),
        )
        .is_ok()
        {
            return true;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    false
}

#[test]
#[ignore]
fn l_avatar_demarre_sans_cache_editeur() {
    let racine = racine();
    let projet = racine.join("godot");
    assert!(
        projet.join("project.godot").is_file(),
        "projet Godot introuvable : {}",
        projet.display()
    );

    // Sans cache d'éditeur, comme sur une installation fraîche.
    let cache = projet.join(".godot");
    let repli = projet.join(".godot.test-bak");
    let deplace = cache.exists();
    if deplace {
        if repli.exists() {
            std::fs::remove_dir_all(&repli).unwrap();
        }
        std::fs::rename(&cache, &repli).unwrap();
    }
    let _restaure = RestaureCache {
        cache: cache.clone(),
        repli: repli.clone(),
        deplace,
    };

    let exe = jimmy_agent::paths::find_godot_exe("").expect("Godot introuvable (JIMMY_GODOT_EXE)");
    let mut sortie = std::env::temp_dir();
    sortie.push(format!("jimmy-avatar-test-{}.log", std::process::id()));
    let journal = std::fs::File::create(&sortie).unwrap();
    let erreurs = journal.try_clone().unwrap();

    let mut enfant = Command::new(&exe)
        .arg("--headless")
        .arg("--path")
        .arg(&projet)
        .arg("--")
        .arg("--port=8799")
        .arg("--bridge-port=8798")
        .arg("--skin=renard")
        .arg("--quality=low")
        .arg("--dodge=0")
        .stdin(Stdio::null())
        .stdout(Stdio::from(journal))
        .stderr(Stdio::from(erreurs))
        .spawn()
        .expect("lancement de Godot impossible");

    // La scène tourne si son serveur HTTP répond ; `is_up` du vrai code fait
    // pareil. Premier lancement (import du projet) : jusqu'à 4 minutes.
    let debout = attendre_le_serveur(8799, Duration::from_secs(240));
    let _ = enfant.kill();
    let _ = enfant.wait();

    let mut journal_lu = String::new();
    std::fs::File::open(&sortie)
        .unwrap()
        .read_to_string(&mut journal_lu)
        .unwrap();
    let _ = std::fs::remove_file(&sortie);
    // Le cache doit être revenu (ou ne jamais avoir bougé).
    assert!(
        !deplace || cache.exists(),
        "cache de l'éditeur non restauré après le test"
    );
    assert!(
        !journal_lu.contains("SCRIPT ERROR"),
        "les scripts Godot ne se parsent pas sans le cache de l'éditeur :\n{}",
        journal_lu
            .lines()
            .filter(|l| l.contains("ERROR"))
            .take(10)
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert!(
        debout,
        "le serveur HTTP de l'avatar ne répond pas sans le cache de l'éditeur"
    );
    assert!(
        journal_lu.contains("[godot/main] prêt"),
        "l'avatar n'a pas affiché son « prêt » sans le cache de l'éditeur"
    );
}
