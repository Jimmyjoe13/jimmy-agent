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

    // Écriture d'un souvenir : une note datée dans le dossier de Jimmy.
    let ecrit = vault
        .remember("semantic", "L'utilisateur boit son café le matin")
        .await
        .expect("écriture");
    assert!(ecrit.starts_with(racine.join("0_Inbox").join("Jimmy")), "{}", ecrit.display());
    let contenu = std::fs::read_to_string(&ecrit).unwrap();
    assert!(contenu.contains("type: semantic"));
    assert!(contenu.contains("source: jimmy"));
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
