//! Tests MCP de bout en bout, contre un vrai processus (Node) parlant stdio.
//!
//! ```text
//! .\scripts\with-msvc.ps1 cargo test -p jimmy-agent --test mcp
//! ```
//!
//! Nécessite `node` dans le PATH. Le programme est volontairement passé sans
//! extension : sous Windows, c'est le chemin `cmd /C` du lanceur qui est testé.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use jimmy_agent::mcp::{McpRegistry, McpServer};
use jimmy_agent::tools::mcp::connect_all;
use jimmy_agent::tools::ToolRegistry;
use serde_json::json;

fn serveur_echo(nom: &str) -> McpServer {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("mcp_echo.js");
    McpServer {
        name: nom.to_string(),
        transport: "stdio".into(),
        command: vec!["node".into(), fixture.to_string_lossy().to_string()],
        url: String::new(),
        env: HashMap::new(),
    }
}

#[tokio::test]
async fn outils_listes_normalises_et_appeles() {
    let mcp = McpRegistry::new(vec![serveur_echo("echo test")]);

    let outils = mcp.list_tools().await;
    let noms: Vec<_> = outils.iter().map(|t| t.name.clone()).collect();
    println!("outils : {noms:?}");
    // Nom de serveur avec espace et nom d'outil pointé : tous deux normalisés.
    assert!(noms.contains(&"mcp_echo_test__echo".to_string()));
    assert!(noms.contains(&"mcp_echo_test__files_fail".to_string()));

    // Appel par le nom préfixé normalisé (ancien bug : serveur introuvable).
    let sortie = mcp
        .call_tool("mcp_echo_test__echo", &json!({"text": "bonjour"}))
        .await
        .expect("l'appel doit aboutir");
    assert_eq!(sortie, "écho : bonjour");

    // `isError: true` doit remonter comme une erreur, pas comme un succès.
    let fail = outils.iter().find(|t| t.remote_name == "files.fail").unwrap();
    let erreur = mcp.call(&fail.server, &fail.remote_name, &json!({})).await;
    assert!(erreur.is_err(), "isError doit produire une erreur");

    mcp.shutdown().await;
}

#[tokio::test]
async fn outils_ajoutes_au_registre() {
    let mcp = McpRegistry::new(vec![serveur_echo("echo")]);
    let registre = Arc::new(ToolRegistry::new());
    let avant = registre.len();

    let ajoutes = connect_all(&mcp, &registre).await;
    assert_eq!(ajoutes, 2);
    assert_eq!(registre.len(), avant + 2);
    assert!(registre.get("mcp_echo__echo").is_some());

    // Un serveur ajouté à chaud remplace l'ancien sans doublon d'outils.
    registre.unregister_prefix("mcp_echo__");
    assert_eq!(registre.len(), avant);

    mcp.shutdown().await;
}

#[tokio::test]
async fn serveur_introuvable_sans_blocage() {
    let mut serveur = serveur_echo("absent");
    serveur.command = vec!["programme-qui-n-existe-pas-jimmy".into()];
    let mcp = McpRegistry::new(vec![serveur]);
    // Doit revenir rapidement avec une liste vide, sans paniquer ni bloquer.
    let debut = std::time::Instant::now();
    let outils = mcp.list_tools().await;
    assert!(outils.is_empty());
    assert!(debut.elapsed() < std::time::Duration::from_secs(30));
}
