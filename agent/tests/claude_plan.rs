//! Abonnement Claude réel : la session Claude Code (jamais de clé API) doit
//! permettre à Jimmy de lister les modèles et d'appeler l'API Messages.
//!
//! Ignoré : consomme l'abonnement réel, et dépend d'une session Claude Code
//! ouverte sur la machine. A lancer pour la verification finale :
//!
//! ```text
//! .\scripts\with-msvc.ps1 cargo test -p jimmy-agent --test claude_plan '--' --ignored --nocapture --test-threads=1
//! ```
//!
//! Méthode : le modèle économique (haiku) est appelé d'abord — les relevés
//! publics indiquent qu'Anthropic peut refuser les modèles premium hors Claude
//! Code ; le verdict premium est affiché sans être exigé.

use jimmy_agent::config::{LlmSettings, Secrets, AUTH_CLAUDE_PLAN, PROVIDER_ANTHROPIC};
use jimmy_agent::core::types::{Message, ToolSpec};
use jimmy_agent::providers::claude_plan::credentials_path;
use jimmy_agent::providers::LlmClient;

const HAIKU: &str = "claude-haiku-4-5-20251001";

/// Client sur la vraie session Claude Code, en mode abonnement, avec Haiku
/// comme modèle principal (le routeur doit donc acheminer chez Anthropic).
/// `None` et affichage « SKIP » si aucune session n'existe.
fn plan_client() -> Option<LlmClient> {
    let path = credentials_path()?;
    if !path.is_file() {
        println!("SKIP : pas de session Claude Code à {}", path.display());
        return None;
    }
    let mut settings = LlmSettings::default();
    settings.provider = PROVIDER_ANTHROPIC.into();
    settings.model = HAIKU.into();
    for p in &mut settings.providers {
        if p.id == PROVIDER_ANTHROPIC {
            p.auth = AUTH_CLAUDE_PLAN.into();
        }
    }
    let client = LlmClient::new("", "", "jimmy-test-plan").expect("client");
    client.update_providers(&settings, &Secrets::default());
    Some(client)
}

#[tokio::test]
#[ignore = "utilise l'abonnement Claude réel (session Claude Code)"]
async fn l_abonnement_claude_liste_et_repond() {
    let Some(client) = plan_client() else { return };
    let models = client
        .list_models(Some(PROVIDER_ANTHROPIC), true)
        .await
        .expect("liste des modèles Claude");
    println!("{} modèle(s) chez Anthropic (abonnement)", models.len());
    for m in &models {
        println!("  {} — {}", m.id, m.name);
    }
    assert!(!models.is_empty(), "aucun modèle");

    // Le moins cher d'abord : un test ne doit pas entamer le quota premium.
    let cheap = models
        .iter()
        .find(|m| m.id.contains("haiku"))
        .or_else(|| models.first())
        .expect("au moins un modèle");
    let test = client.test_model(Some(PROVIDER_ANTHROPIC), &cheap.id).await;
    println!(
        "test « {} » : ok={} outils={} {} ms — {:?}",
        test.model, test.ok, test.tools, test.latency_ms, test.reply
    );
    assert!(test.ok, "l'abonnement ne répond pas sur « {} » : {}", cheap.id, test.error);

    // Verdicts premium affichés sans être exigés : Anthropic bride le premium
    // hors de son propre client (piège 90).
    for family in ["sonnet", "opus"] {
        if let Some(premium) = models.iter().find(|m| m.id.contains(family)) {
            let t = client.test_model(Some(PROVIDER_ANTHROPIC), &premium.id).await;
            println!(
                "verdict {family} « {} » : ok={} outils={} — {}",
                t.model,
                t.ok,
                t.tools,
                if t.error.is_empty() { "ok".to_string() } else { t.error.clone() }
            );
        }
    }
}

/// Diagnostic (9 octobre) : le « Tester » passe sur Haiku mais le chat réel
/// échoue en 400 « extra usage ». Seules différences : `max_tokens`, taille du
/// system, nombre d'outils, et le flux SSE. On bissecte, une variable à la
/// fois, pour savoir laquelle déclenche la grille d'abonnement d'Anthropic.
#[tokio::test]
#[ignore = "bissecte la grille « extra usage » sur l'abonnement réel"]
async fn labonnement_bissecte_les_declencheurs() {
    let Some(client) = plan_client() else { return };
    let ping = ToolSpec {
        name: "ping".into(),
        description: "Outil de test.".into(),
        parameters: serde_json::json!({ "type": "object", "properties": {} }),
    };
    let ping2 = ToolSpec { name: "pong".into(), description: "Second outil.".into(), parameters: serde_json::json!({"type":"object","properties":{}}) };
    let gros_system = Message::system(format!("Tu es Jimmy. {}", "réponds brièvement en français. ".repeat(400)));
    let question = Message::user("Une phrase : quelle est la capitale de l'Italie ?");
    let petit = [Message::user("Réponds par le mot : ok")];

    // Tailles réelles d'un tour de Jimmy (journal : ~5 700 jetons d'entrée).
    let enorme_system = Message::system(format!("Tu es Jimmy. {}", "réponds brièvement en français, sans jargon, une à trois phrases. ".repeat(1200)));
    let many: Vec<ToolSpec> = [
        "read_file", "write_file", "list_directory", "search_files", "run_command", "vault_search",
        "vault_read", "vault_write", "memory_recall", "list_skills", "mcp_list_tools", "mcp_call",
    ]
    .iter()
    .map(|name| ToolSpec {
        name: (*name).into(),
        description: format!("Outil « {name} » de l'agent Jimmy : lit, écrit et interroge le disque, la mémoire et les serveurs avec des bornes claires."),
        parameters: serde_json::json!({"type":"object","properties":{"path":{"type":"string","description":"Chemin cible, relatif ou absolu"},"query":{"type":"string","description":"Recherche ou commande à exécuter"},"limit":{"type":"integer","description":"Nombre maximum d'entrées renvoyées"}},"required":["path"]}),
    })
    .collect();

    // 12 outils au schéma minimal : distingue le NOMBRE d'outils du POIDS.
    let little: Vec<ToolSpec> = (1..=12)
        .map(|i| ToolSpec {
            name: format!("outil_{i}"),
            description: "Test.".into(),
            parameters: serde_json::json!({ "type": "object", "properties": {} }),
        })
        .collect();

    let cas: Vec<(&str, Vec<Message>, Vec<ToolSpec>, u32, bool)> = vec![
        ("1. petit, sans outil, max 128, sans flux (le « Tester »)", petit.to_vec(), vec![], 128, false),
        ("2. max_tokens 16384 seul", petit.to_vec(), vec![], 16_384, false),
        ("3. un outil, max 128", petit.to_vec(), vec![ping.clone()], 128, false),
        ("4. gros system + outil + 16384 (forme réelle du chat, sans flux)", vec![gros_system.clone(), question.clone()], vec![ping.clone()], 16_384, false),
        ("5. petit, max 128, EN FLUX", petit.to_vec(), vec![], 128, true),
        ("6. forme réelle DU CHAT, en flux", vec![gros_system, question], vec![ping.clone()], 16_384, true),
        ("7. énorme system (≈10k jetons), max 16384, sans outil", vec![enorme_system.clone(), Message::user("Une phrase : 2+2 ?")], vec![], 16_384, false),
        ("8. 12 outils réalistes, petit message, max 16384", petit.to_vec(), many.clone(), 16_384, false),
        ("9. énorme system + 12 outils + 16384, EN FLUX (chat réel 1:1)", vec![enorme_system.clone(), Message::user("Une phrase : 2+2 ?")], many.clone(), 16_384, true),
        ("10. énorme system + 12 outils + max 8192", vec![enorme_system, Message::user("Une phrase : 2+2 ?")], many.clone(), 8_192, false),
        ("11. DEUX outils, max 128", petit.to_vec(), vec![ping.clone(), ping2.clone()], 128, false),
        ("12. 12 outils, max 128", petit.to_vec(), many.clone(), 128, false),
        ("13. 12 outils MINUSCULES, max 128", petit.to_vec(), little.clone(), 128, false),
        ("14. un outil, max 8192", petit.to_vec(), vec![ping.clone()], 8_192, false),
    ];

    for (nom, messages, tools, max_tokens, stream) in cas {
        let outcome = if stream {
            client
                .chat_stream(HAIKU, &messages, &tools, None, max_tokens, |_| {})
                .await
        } else {
            client.chat(HAIKU, &messages, &tools, None, max_tokens).await
        };
        match outcome {
            Ok(reply) => println!(
                "{nom} : OK — entrée {} jetons, sortie {} — {:?}",
                reply.usage.prompt_tokens,
                reply.usage.completion_tokens,
                reply.content.trim().chars().take(50).collect::<String>()
            ),
            Err(error) => println!("{nom} : ÉCHEC — {error}"),
        }
    }
}

/// L'abonnement sert désormais les modèles premium : la signature Claude Code
/// (bloc de facturation en tête du systeme, `inject_billing_block`) leve la
/// grille « extra usage ». Mesure du 9 octobre : sans elle Sonnet renvoyait
/// 429 ; avec, il repond 200 — ce test le re-verifie reellement.
#[tokio::test]
#[ignore = "verifie le premium via l'abonnement (session Claude Code reelle)"]
async fn le_premium_passe_par_signature_claude_code() {
    let Some(client) = plan_client() else { return };
    let test = client.test_model(Some(PROVIDER_ANTHROPIC), "claude-sonnet-4-5-20250929").await;
    println!(
        "sonnet sur abonnement « {} » : ok={} outils={} {} ms — {:?}",
        test.model, test.ok, test.tools, test.latency_ms, test.reply
    );
    assert!(test.ok, "Sonnet doit repondre avec la signature Claude Code : {}", test.error);
    assert!(test.tools, "Sonnet doit accepter les outils : {}", test.error);
}
