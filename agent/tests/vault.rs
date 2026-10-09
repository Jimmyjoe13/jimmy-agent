//! Vault Obsidian : mémoire persistante de Jimmy, en notes Markdown.
//!
//! ```text
//! .\scripts\with-msvc.ps1 cargo test -p jimmy-agent --test vault
//! # et, contre le vrai vault de la machine :
//! .\scripts\with-msvc.ps1 cargo test -p jimmy-agent --test vault '--' --ignored --nocapture
//! ```

use jimmy_agent::memory::vault::Vault;

/// Le vault est indexé en plein texte, accents et casse ignorés ; une note
/// trouvée se lit par son chemin relatif, et un souvenir s'écrit dans le
/// dossier de Jimmy.
#[tokio::test]
async fn recherche_lecture_et_ecriture_dans_un_vault() {
    let racine = std::env::temp_dir().join(format!("vault-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&racine);
    std::fs::create_dir_all(racine.join("1_Projets")).unwrap();
    std::fs::write(
        racine.join("1_Projets").join("Café.md"),
        "# Café\n\nJimmy préfère le café sans sucre, le matin.",
    )
    .unwrap();
    std::fs::write(
        racine.join("1_Projets").join("Voiture.md"),
        "# Voiture\n\nL'utilisateur roule en électrique.",
    )
    .unwrap();

    let vault = Vault::open(racine.to_str().unwrap(), "0_Inbox/Jimmy").expect("vault");
    assert_eq!(vault.count(), 2);

    // Accent et casse : « cafe » doit retrouver « Café ».
    let hits = vault.search("cafe du matin", 5).await;
    assert_eq!(hits.len(), 1, "une seule note parle de café : {hits:?}");
    assert!(hits[0].snippet.contains("café"), "extrait : {}", hits[0].snippet);

    // Lecture par le chemin relatif renvoyé par la recherche.
    let contenu = vault.read(&hits[0].path).await.expect("lecture");
    assert!(contenu.contains("sans sucre"));

    // Écriture d'un souvenir : une puce dans le fichier du jour de l'inbox.
    let ecrit = vault
        .remember("semantic", "L'utilisateur boit son café le matin")
        .await
        .expect("écriture");
    assert!(ecrit.starts_with(racine.join("0_Inbox").join("Jimmy")), "{}", ecrit.display());
    let contenu = std::fs::read_to_string(&ecrit).unwrap();
    assert!(contenu.contains("type: inbox"));
    assert!(contenu.contains("source: jimy"));
    assert!(contenu.contains("[semantic] L'utilisateur boit son café le matin"), "{contenu}");
    let hits = vault.search("café matin", 5).await;
    assert!(hits.iter().any(|h| h.snippet.contains("boit son café")), "{hits:?}");

    let _ = std::fs::remove_dir_all(&racine);
}

/// Contre le **vrai** vault : « jimmy » doit ramener des notes réelles.
/// Ignoré par défaut (il dépend du contenu de la machine).
#[tokio::test]
#[ignore = "nécessite le vault Obsidian réel"]
async fn vault_reel_retrouve_des_notes() {
    let chemin = std::env::var("JIMMY_VAULT_PATH")
        .unwrap_or_else(|_| r"C:\Obsidian\Jimmy".to_string());
    let Some(vault) = Vault::open(&chemin, "0_Inbox/Jimmy") else {
        println!("vault absent : {chemin} — test ignoré");
        return;
    };
    println!("vault : {}", vault.root().display());
    println!("notes : {}", vault.count());
    let hits = vault.search("jimmy", 5).await;
    for hit in &hits {
        println!("- ({:.1}) {} — {}", hit.score, hit.title, hit.snippet);
    }
    assert!(!hits.is_empty(), "le vault réel doit contenir « jimmy »");
}

/// Contre la **vraie** machine : sans chemin réglé, Jimmy doit trouver le
/// vault d'Obsidian dans son registre (`%APPDATA%\obsidian\obsidian.json`).
#[test]
#[ignore = "nécessite Obsidian installé sur la machine"]
fn la_decouverte_trouve_le_vault_d_obsidian() {
    let donnees = std::env::temp_dir().join(format!("jimmy-decouverte-{}", std::process::id()));
    let vault = Vault::locate("", &donnees, "0_Inbox/Jimmy").expect("vault");
    println!("vault : {} ({})", vault.root().display(), vault.origin().as_str());
    assert_eq!(vault.origin(), jimmy_agent::memory::vault::VaultOrigin::Obsidian);
    assert!(!donnees.join("vault").exists(), "aucun vault propre ne doit être créé");
}

/// Tri réel (vrai modèle) sur une **copie** du vault, jamais l'original.
/// `JIMMY_SORT_DIR` contient `data\config.json` (copie de la config, pour le
/// modèle) et `vault\` (copie du vault).
#[tokio::test(flavor = "multi_thread")]
#[ignore = "trie une copie du vault avec le vrai modèle"]
async fn tri_reel_sur_une_copie_du_vault() {
    use jimmy_agent::config::Secrets;
    use jimmy_agent::paths::Paths;
    // Journal visible (`--nocapture`) : un lot illisible doit se diagnostiquer.
    let _ = env_logger::builder().filter_module("jimmy_agent", log::LevelFilter::Info).is_test(true).try_init();
    let dir = std::path::PathBuf::from(std::env::var("JIMMY_SORT_DIR").expect("JIMMY_SORT_DIR"));
    let workspace = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf();
    let _ = jimmy_agent::paths::load_dotenv(&workspace.join(".env"));
    std::env::set_var("JIMMY_VAULT_PATH", dir.join("vault"));
    let paths = Paths { data: dir.join("data"), app: workspace, dev: false };
    let app = jimmy_agent::App::new(paths, Secrets::from_env()).expect("app");
    let vault = app.vault.clone().expect("vault");
    let model = app.settings().memory_model().to_string();
    println!("modèle de la mémoire : {model}");
    let debut = std::time::Instant::now();
    let report = jimmy_agent::memory::sort::sort(&vault, &app.llm, &model).await.expect("tri");
    println!("tri en {} s : {report:#?}", debut.elapsed().as_secs());
    assert!(report.items > 0 && !report.notes.is_empty(), "{report:?}");
}
