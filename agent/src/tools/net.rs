//! Accès réseau : requêtes HTTP et ouverture du navigateur.

use std::time::Duration;

use super::{arg_str, arg_u64, clip, schema, BoxFuture, Tool, ToolContext, MAX_TOOL_OUTPUT};
use crate::error::{Error, Result};
use crate::permissions::Capability;

pub struct HttpRequest {
    http: reqwest::Client,
}

impl HttpRequest {
    pub fn new(http: reqwest::Client) -> Self {
        HttpRequest { http }
    }
}

impl Tool for HttpRequest {
    fn name(&self) -> &str {
        "http_request"
    }
    fn description(&self) -> &str {
        "Effectue une requête HTTP et renvoie le corps de la réponse. Utilise cet outil pour consulter une API ou une page web."
    }
    fn parameters(&self) -> serde_json::Value {
        schema(
            serde_json::json!({
                "method": {"type": "string", "description": "GET, POST, PUT, PATCH, DELETE. Défaut GET."},
                "url": {"type": "string", "description": "URL complète, ex. https://exemple.fr/api."},
                "headers": {"type": "object", "description": "En-têtes additionnels, sous forme d'objet."},
                "body": {"type": "string", "description": "Corps de la requête pour POST/PUT/PATCH."},
                "timeout_ms": {"type": "integer", "description": "Durée maximale (défaut 30000)."}
            }),
            &["url"],
        )
    }
    fn capability(&self) -> Capability {
        Capability::Network
    }
    fn call<'a>(
        &'a self,
        args: &'a serde_json::Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            let url = arg_str(args, "url").ok_or_else(|| Error::Tool("« url » manquant".into()))?;
            let host = url
                .split("://")
                .nth(1)
                .and_then(|rest| rest.split('/').next())
                .unwrap_or(url.as_str())
                .to_string();
            ctx.check(Capability::Network, &host)?;

            let method = arg_str(args, "method").unwrap_or_else(|| "GET".into()).to_uppercase();
            let method = reqwest::Method::from_bytes(method.as_bytes())
                .map_err(|_| Error::Tool(format!("méthode invalide : {method}")))?;
            let timeout_ms = arg_u64(args, "timeout_ms", 30_000).min(180_000);

            let mut request = self.http.request(
                method.clone(),
                url.clone(),
            ).timeout(Duration::from_millis(timeout_ms));
            if let Some(headers) = args.get("headers").and_then(|v| v.as_object()) {
                for (name, value) in headers {
                    if let Some(text) = value.as_str() {
                        request = request.header(name, text);
                    }
                }
            }
            if let Some(body) = arg_str(args, "body") {
                request = request.body(body);
            }

            let response = request.send().await.map_err(|e| Error::Tool(e.to_string()))?;
            let status = response.status();
            let content_type = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();
            let text = response.text().await.unwrap_or_default();
            Ok(clip(
                &format!("HTTP {status}\nContent-Type: {content_type}\n\n{text}"),
                MAX_TOOL_OUTPUT,
            ))
        })
    }
}

pub struct OpenBrowser;

impl Tool for OpenBrowser {
    fn name(&self) -> &str {
        "open_browser"
    }
    fn description(&self) -> &str {
        "Ouvre une adresse dans le navigateur par défaut de l'utilisateur."
    }
    fn parameters(&self) -> serde_json::Value {
        schema(
            serde_json::json!({
                "url": {"type": "string", "description": "Adresse à ouvrir, ex. https://exemple.fr."}
            }),
            &["url"],
        )
    }
    fn capability(&self) -> Capability {
        Capability::Execute
    }
    fn call<'a>(
        &'a self,
        args: &'a serde_json::Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            let url = arg_str(args, "url").ok_or_else(|| Error::Tool("« url » manquant".into()))?;
            let host = url
                .split("://")
                .nth(1)
                .and_then(|rest| rest.split('/').next())
                .unwrap_or(url.as_str())
                .to_string();
            ctx.check(Capability::Network, &host)?;
            if !(url.starts_with("http://") || url.starts_with("https://")) {
                return Err(Error::Tool(
                    "seules les adresses http et https sont ouvertes".to_string(),
                ));
            }
            ctx.check(Capability::Execute, &format!("start {url}"))?;

            let mut command = Command::new("cmd");
            command
                .arg("/C")
                .arg("start")
                .arg("")
                .arg(&url)
                .stdin(Stdio::null());
            #[cfg(windows)]
            {
                command.creation_flags(0x08000000);
            }
            command
                .output()
                .await
                .map_err(|e| Error::Tool(format!("ouverture impossible : {e}")))?;
            Ok(format!("navigateur ouvert sur {url}"))
        })
    }
}

use std::process::Stdio;
use tokio::process::Command;