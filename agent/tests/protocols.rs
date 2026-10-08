//! Les trois formats d'API d'OpenCode Go, avec de vrais modèles : un tour
//! avec outil puis la réponse finale, en flux, par le client de Jimmy.
//!
//! Cas réel du 6 octobre : Muse Spark 1.3 restait muet (`400
//! ModelProtocolUnsupported`), Jimmy ne parlant que le format Chat.
//!
//! `cargo test -p jimmy-agent --test protocols -- --ignored`

use std::path::Path;

use jimmy_agent::core::types::{Message, ToolCall, ToolSpec};
use jimmy_agent::providers::LlmClient;

/// Un modèle par format : Chat, Responses, Messages.
const MODELES: [&str; 3] = ["glm-5.3-flash", "muse-spark-1.3-contributor", "qwen3.8-flash"];

fn client() -> LlmClient {
    let racine = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let _ = jimmy_agent::paths::load_dotenv(&racine.join(".env"));
    let secrets = jimmy_agent::config::Secrets::from_env();
    assert!(!secrets.opencode_api_key.is_empty(), "OPENCODE_API_KEY absente : ce test a besoin d'un vrai modèle");
    LlmClient::new("https://opencode.ai/zen/go/v1", &secrets.opencode_api_key, "jimmy-test-protocoles").unwrap()
}

fn heure() -> ToolSpec {
    ToolSpec {
        name: "heure".into(),
        description: "Donne l'heure actuelle dans une ville.".into(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": { "ville": { "type": "string" } },
            "required": ["ville"],
        }),
    }
}

#[tokio::test]
#[ignore = "consulte de vrais services ; à lancer explicitement"]
async fn chaque_format_fait_un_tour_avec_outil() {
    let llm = client();
    for model in MODELES {
        let mut messages = vec![
            Message::system("Tu es Jimmy. Pour connaître l'heure, appelle toujours l'outil heure."),
            Message::user("Quelle heure est-il à Paris ?"),
        ];
        // Tour 1 : le modèle doit demander l'outil.
        let first = llm
            .chat_stream(model, &messages, &[heure()], None, 2048, |_| {})
            .await
            .unwrap_or_else(|e| panic!("{model} : tour 1 en échec : {e}"));
        let call: ToolCall = first
            .tool_calls
            .first()
            .cloned()
            .unwrap_or_else(|| panic!("{model} : aucun appel d'outil ({:?})", first.content));
        assert_eq!(call.name, "heure", "{model}");
        assert!(!call.id.is_empty(), "{model} : appel sans identifiant");

        // Tour 2 : le résultat revient, la réponse finale arrive en flux.
        let mut assistant = Message::assistant(first.content.clone());
        assistant.tool_calls = Some(first.tool_calls.clone());
        messages.push(assistant);
        messages.push(Message::tool_result(call.id.clone(), "heure", "14:32"));
        let mut streamed = String::new();
        let second = llm
            .chat_stream(model, &messages, &[heure()], None, 2048, |text| streamed.push_str(text))
            .await
            .unwrap_or_else(|e| panic!("{model} : tour 2 en échec : {e}"));
        assert!(second.content.contains("14"), "{model} : réponse finale « {} »", second.content);
        assert_eq!(streamed, second.content, "{model} : le flux affiché diffère de la réponse");
        assert!(second.usage.prompt_tokens > 0, "{model} : usage absent");
        eprintln!("{model} : outil {} → « {} »", call.arguments, second.content.trim());
    }
}

/// Le test de la bibliothèque de modèles passe dans chaque format.
#[tokio::test]
#[ignore = "consulte de vrais services ; à lancer explicitement"]
async fn le_test_de_modele_passe_dans_chaque_format() {
    let llm = client();
    for model in MODELES {
        let test = llm.test_model(None, model).await;
        assert!(test.ok && test.tools, "{model} : {test:?}");
    }
}

/// Vision (7 octobre) : une image jointe est lue par un vrai modèle de
/// chaque format, en flux, par le client de Jimmy. Image générée (gauche
/// rouge, droite bleue) : aucune donnée personnelle.
#[tokio::test]
#[ignore = "consulte de vrais services ; à lancer explicitement"]
async fn chaque_format_lit_une_image() {
    use base64::Engine;
    let llm = client();
    let rgba = image::RgbaImage::from_fn(96, 64, |x, _| if x < 48 { image::Rgba([255, 0, 0, 255]) } else { image::Rgba([0, 0, 255, 255]) });
    let (jpeg, _, _) = jimmy_agent::screen::encode_jpeg(&rgba, 1600).unwrap();
    let image = jimmy_agent::core::types::Image {
        media_type: "image/jpeg".into(),
        base64: base64::engine::general_purpose::STANDARD.encode(&jpeg),
        label: "test".into(),
    };
    for model in MODELES {
        let messages = vec![Message::user_with_images(
            "Quelles sont les deux couleurs de cette image, de gauche à droite ? Réponds en deux mots.",
            vec![image.clone()],
        )];
        let reply = llm
            .chat_stream(model, &messages, &[], None, 2048, |_| {})
            .await
            .unwrap_or_else(|e| panic!("{model} : image refusée : {e}"));
        let text = reply.content.to_lowercase();
        println!("{model} : {}", reply.content.trim());
        assert!(text.contains("rouge") && text.contains("bleu"), "{model} n'a pas lu l'image : {text}");
    }
}
