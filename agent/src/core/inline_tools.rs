//! Appels d'outil arrivés **en texte** au lieu d'un appel structuré.
//!
//! Cas réel du 4 octobre : MiMo a voulu écrire `notify.py`, mais sa réponse a
//! atteint la limite de longueur ; l'appel `write_file` est arrivé coupé, dans le
//! texte (`<tool_call><function=write_file><parameter=content>…`). Jimmy le
//! prenait pour la réponse finale : l'écriture était perdue et la balise
//! affichée telle quelle. Deux formats sont reconnus :
//!
//! * XML : `<tool_call><function=NOM><parameter=CLÉ>valeur</parameter>…</function></tool_call>` ;
//! * JSON : `<tool_call>{"name": "NOM", "arguments": {…}}</tool_call>`.
//!
//! Un appel incomplet (sans `</tool_call>`) n'est **pas** deviné : l'agent
//! redemande proprement au modèle.

use serde_json::{Map, Value};

use crate::core::types::ToolCall;

const OPEN: &str = "<tool_call>";
const CLOSE: &str = "</tool_call>";

/// Texte avant la première balise d'appel : ce que le modèle disait vraiment.
pub fn strip_tool_markup(content: &str) -> String {
    match content.find(OPEN) {
        Some(at) => content[..at].trim().to_string(),
        None => content.trim().to_string(),
    }
}

/// Appels complets trouvés dans `content`, avec le texte qui les précède.
/// `None` si une balise est présente mais un appel est incomplet ou illisible.
pub fn parse_inline_tool_calls(content: &str) -> Option<(String, Vec<ToolCall>)> {
    let first = content.find(OPEN)?;
    let mut calls = Vec::new();
    let mut rest = &content[first..];
    while let Some(start) = rest.find(OPEN) {
        let after = &rest[start + OPEN.len()..];
        let end = after.find(CLOSE)?;
        let body = after[..end].trim();
        let (name, arguments) = parse_json_call(body).or_else(|| parse_xml_call(body))?;
        calls.push(ToolCall { id: format!("inline_{}", calls.len()), name, arguments });
        rest = &after[end + CLOSE.len()..];
    }
    (!calls.is_empty()).then(|| (content[..first].trim().to_string(), calls))
}

fn parse_json_call(body: &str) -> Option<(String, Value)> {
    let value: Value = serde_json::from_str(body).ok()?;
    let name = value.get("name")?.as_str()?.to_string();
    let arguments = match value.get("arguments") {
        Some(Value::String(raw)) => serde_json::from_str(raw).ok()?,
        Some(other) => other.clone(),
        None => Value::Object(Map::new()),
    };
    Some((name, arguments))
}

fn parse_xml_call(body: &str) -> Option<(String, Value)> {
    let inner = body.strip_prefix("<function=")?;
    let name_end = inner.find('>')?;
    let name = inner[..name_end].trim().to_string();
    let mut params = &inner[name_end + 1..];
    // La fermeture `</function>` est obligatoire : sans elle, l'appel est coupé.
    let function_end = params.rfind("</function>")?;
    params = &params[..function_end];
    let mut arguments = Map::new();
    while let Some(start) = params.find("<parameter=") {
        let after = &params[start + "<parameter=".len()..];
        let key_end = after.find('>')?;
        let key = after[..key_end].trim().to_string();
        let value_start = &after[key_end + 1..];
        let value_end = value_start.find("</parameter>")?;
        let raw = trim_one_newline(&value_start[..value_end]);
        arguments.insert(key.clone(), typed_value(&key, raw));
        params = &value_start[value_end + "</parameter>".len()..];
    }
    Some((name, Value::Object(arguments)))
}

/// Retire un seul retour à la ligne au début et à la fin (mise en forme du
/// modèle), sans toucher au reste du contenu (indentation d'un fichier).
fn trim_one_newline(value: &str) -> &str {
    let value = value.strip_prefix("\r\n").or_else(|| value.strip_prefix('\n')).unwrap_or(value);
    value.strip_suffix("\r\n").or_else(|| value.strip_suffix('\n')).unwrap_or(value)
}

/// Les valeurs XML sont du texte ; nombres, booléens et objets JSON sont
/// convertis — sauf `content`, toujours du texte (un fichier JSON à écrire
/// doit rester une chaîne).
fn typed_value(key: &str, raw: &str) -> Value {
    if key != "content" {
        let trimmed = raw.trim();
        let looks_typed = trimmed.starts_with('{')
            || trimmed.starts_with('[')
            || trimmed == "true"
            || trimmed == "false"
            || trimmed.parse::<f64>().is_ok();
        if looks_typed {
            if let Ok(value) = serde_json::from_str(trimmed) {
                return value;
            }
        }
    }
    Value::String(raw.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_appel_xml_complet_est_recupere() {
        let content = "J'écris le module.<tool_call><function=write_file><parameter=path>\nsrc/notify.py\n</parameter>\
                       <parameter=content>\n\"\"\"Doc.\"\"\"\n\ndef f():\n    return 1\n</parameter></function></tool_call>";
        let (text, calls) = parse_inline_tool_calls(content).expect("appel");
        assert_eq!(text, "J'écris le module.");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "write_file");
        assert_eq!(calls[0].arguments["path"], "src/notify.py");
        assert_eq!(calls[0].arguments["content"], "\"\"\"Doc.\"\"\"\n\ndef f():\n    return 1");
    }

    /// Le cas réel : réponse coupée par la limite de longueur.
    #[test]
    fn un_appel_coupe_n_est_pas_devine() {
        let content = "J'écris.<tool_call><function=write_file><parameter=content>\"\"\"Notifications Telegram…\n\ndef notify_new_ideas(";
        assert!(parse_inline_tool_calls(content).is_none());
        assert_eq!(strip_tool_markup(content), "J'écris.");
    }

    #[test]
    fn le_format_json_et_les_valeurs_typees_sont_compris() {
        let content = "<tool_call>{\"name\": \"read_file\", \"arguments\": {\"path\": \"a.py\", \"start_line\": 10}}</tool_call>\
                       <tool_call><function=read_file><parameter=path>b.py</parameter><parameter=max_lines>40</parameter></function></tool_call>";
        let (text, calls) = parse_inline_tool_calls(content).expect("appels");
        assert!(text.is_empty());
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].arguments["start_line"], 10);
        assert_eq!(calls[1].arguments["max_lines"], 40);
        assert_eq!(calls[1].id, "inline_1");
        // Un contenu de fichier JSON reste du texte.
        let json_file = "<tool_call><function=write_file><parameter=path>c.json</parameter><parameter=content>{\"a\": 1}</parameter></function></tool_call>";
        let (_, calls) = parse_inline_tool_calls(json_file).unwrap();
        assert_eq!(calls[0].arguments["content"], "{\"a\": 1}");
    }

    #[test]
    fn sans_balise_rien_ne_change() {
        assert!(parse_inline_tool_calls("Bonjour, voici la réponse.").is_none());
        assert_eq!(strip_tool_markup("  Réponse simple. "), "Réponse simple.");
    }
}
