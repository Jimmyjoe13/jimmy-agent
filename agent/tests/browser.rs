//! Navigateur (7 octobre) : le serveur MCP Playwright, lancé comme Jimmy le
//! lance (`npx`, via `cmd /C`), ouvre une page et la lit.
//!
//! Sans fenêtre et avec un profil jetable (`--headless --isolated`) : le test
//! ne touche jamais au profil de Jimmy (`data/browser-profile`).
//!
//! `cargo test -p jimmy-agent --test browser -- --ignored --nocapture`

use std::collections::HashMap;

use jimmy_agent::mcp::{McpRegistry, McpServer};
use serde_json::json;

#[tokio::test]
#[ignore = "lance Chrome et consulte un vrai site ; à lancer explicitement"]
async fn le_navigateur_ouvre_et_lit_une_page() {
    let mcp = McpRegistry::new(vec![McpServer {
        name: "navigateur".into(),
        transport: "stdio".into(),
        command: [
            "npx", "-y", "@playwright/mcp@0.0.83", "--browser", "chrome", "--headless", "--isolated",
            "--image-responses", "omit",
        ]
        .iter()
        .map(|s| s.to_string())
        // Sans `--output-dir`, Playwright écrit ses instantanés dans le
        // dossier courant (`agent/.playwright-mcp`).
        .chain(["--output-dir".to_string(), std::env::temp_dir().join("jimmy-test-navigateur").display().to_string()])
        .collect(),
        url: String::new(),
        env: HashMap::new(),
    }]);

    let outils: Vec<String> = mcp.list_tools().await.into_iter().map(|t| t.name).collect();
    println!("{} outils", outils.len());
    for attendu in ["browser_navigate", "browser_snapshot", "browser_click", "browser_type"] {
        assert!(outils.contains(&format!("mcp_navigateur__{attendu}")), "outil manquant : {attendu}");
    }

    let page = mcp
        .call_tool("mcp_navigateur__browser_navigate", &json!({ "url": "https://example.com" }))
        .await
        .expect("navigation");
    println!("{}", page.chars().take(600).collect::<String>());
    assert!(page.contains("Example Domain"), "page non lue");

    // L'instantané doit arriver DANS la réponse (le modèle ne lit pas un
    // fichier joint) : liens et textes de la page.
    let snapshot = mcp.call_tool("mcp_navigateur__browser_snapshot", &json!({})).await.expect("instantané");
    println!("--- instantané ---
{}", snapshot.chars().take(900).collect::<String>());
    assert!(snapshot.contains("Learn more") || snapshot.contains("More information"), "contenu absent de la réponse");

    mcp.shutdown().await;
}
