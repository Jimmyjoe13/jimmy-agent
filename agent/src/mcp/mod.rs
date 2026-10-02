//! Client MCP (Model Context Protocol).
//!
//! Périmètre V1, volontairement restreint : **stdio**. C'est le transport que
//! presque tous les serveurs MCP du catalogue utilisent, il ne demande ni port
//! ni authentification, et il fonctionne derrière un pare-feu sans configuration.
//!
//! Le cycle de vie suit la spécification : `initialize`, puis `notifications/initialized`,
//! puis `tools/list` et `tools/call`. Chaque serveur est un processus annexe,
//! démarré à la demande et arrêté avec Jimmy.
//!
//! Ce module ne contient **pas** de marketplace : la liste des serveurs vient de
//! la configuration, et Jimmy peut en ajouter par son outil `mcp_add_server`
//! puis l'utiliser normalement. C'est exactement le découpage demandé : « pas
//! de marketplace en V1, mais une intégration technique propre ».

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use serde_json::{json, Value};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

use crate::error::{Error, Result};

/// Serveur MCP connu de Jimmy.
#[derive(Debug, Clone)]
pub struct McpServer {
    pub name: String,
    pub transport: String,
    pub command: Vec<String>,
    pub url: String,
    pub env: HashMap<String, String>,
}

impl McpServer {
    pub fn is_stdio(&self) -> bool {
        self.transport.eq_ignore_ascii_case("stdio")
    }
}

#[derive(Debug, Clone)]
pub struct McpTool {
    pub server: String,
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

/// Connexion active à un serveur MCP.
struct Connection {
    child: Child,
    stdin: tokio::process::ChildStdin,
    reader: BufReader<tokio::process::ChildStdout>,
    next_id: u64,
}

pub struct McpRegistry {
    servers: Vec<McpServer>,
    connections: Mutex<HashMap<String, Connection>>,
}

impl McpRegistry {
    pub fn new(servers: Vec<McpServer>) -> Arc<Self> {
        Arc::new(McpRegistry {
            servers,
            connections: Mutex::new(HashMap::new()),
        })
    }

    pub fn server_names(&self) -> Vec<String> {
        self.servers.iter().map(|s| s.name.clone()).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.servers.is_empty()
    }

    /// Démarre le serveur si nécessaire et renvoie l'identifiant de session.
    async fn ensure(&self, server: &McpServer) -> Result<String> {
        let mut connections = self.connections.lock().await;
        if connections.contains_key(&server.name) {
            return Ok(server.name.clone());
        }
        if !server.is_stdio() {
            return Err(Error::Mcp(format!(
                "transport non pris en charge en V1 : {}",
                server.transport
            )));
        }
        let (program, args) = server
            .command
            .split_first()
            .ok_or_else(|| Error::Mcp(format!("{} : commande vide", server.name)))?;

        let mut command = Command::new(program);
        command
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .envs(&server.env);
        #[cfg(windows)]
        {
            command.creation_flags(0x08000000);
        }

        let mut child = command
            .spawn()
            .map_err(|e| Error::Mcp(format!("{} : démarrage impossible ({e})", server.name)))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| Error::Mcp("stdin indisponible".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| Error::Mcp("stdout indisponible".into()))?;

        let mut connection = Connection {
            child,
            stdin,
            reader: BufReader::new(stdout),
            next_id: 1,
        };

        let _result = rpc(
            &mut connection,
            "initialize",
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {"tools": {}},
                "clientInfo": {"name": "jimmy", "version": env!("CARGO_PKG_VERSION")},
            }),
        )
        .await?;
        // La spécification impose cet accusé avant tout appel ultérieur.
        notify(&mut connection, "notifications/initialized", json!({})).await?;

        connections.insert(server.name.clone(), connection);
        log::info!("[mcp] serveur prêt : {}", server.name);
        Ok(server.name.clone())
    }

    /// Outils exposés par l'ensemble des serveurs activés.
    pub async fn list_tools(&self) -> Vec<McpTool> {
        let mut out = Vec::new();
        for server in &self.servers {
            let name = server.name.clone();
            let mut connections = self.connections.lock().await;
            if !connections.contains_key(&name) {
                drop(connections);
                if self.ensure(server).await.is_err() {
                    continue;
                }
                connections = self.connections.lock().await;
            }
            let Some(connection) = connections.get_mut(&name) else {
                continue;
            };
            let Ok(value) = rpc(connection, "tools/list", json!({})).await else {
                continue;
            };
            let tools = value
                .get("tools")
                .and_then(|t| t.as_array())
                .cloned()
                .unwrap_or_default();
            for tool in tools {
                let Some(tool_name) = tool.get("name").and_then(|n| n.as_str()) else {
                    continue;
                };
                out.push(McpTool {
                    server: name.clone(),
                    name: format!("mcp_{}__{}", sanitize(&name), tool_name),
                    description: tool
                        .get("description")
                        .and_then(|d| d.as_str())
                        .unwrap_or("Outil MCP")
                        .to_string(),
                    input_schema: tool
                        .get("inputSchema")
                        .cloned()
                        .unwrap_or_else(|| json!({"type": "object", "properties": {}})),
                });
            }
        }
        out
    }

    /// Appelle un outil exposé par un serveur, via son nom préfixé.
    pub async fn call_tool(&self, prefixed_name: &str, arguments: &Value) -> Result<String> {
        let (server_name, tool_name) = split_prefixed(prefixed_name)?;
        let server = self
            .servers
            .iter()
            .find(|s| s.name == server_name)
            .ok_or_else(|| Error::Mcp(format!("serveur inconnu : {server_name}")))?;
        self.ensure(server).await?;
        let mut connections = self.connections.lock().await;
        let connection = connections
            .get_mut(&server_name)
            .ok_or_else(|| Error::Mcp("connexion perdue".into()))?;
        let value = rpc(
            connection,
            "tools/call",
            json!({ "name": tool_name, "arguments": arguments }),
        )
        .await?;
        Ok(render_result(&value))
    }

    pub async fn shutdown(&self) {
        let mut connections = self.connections.lock().await;
        for (_, connection) in connections.iter_mut() {
            let _ = connection.child.kill().await;
        }
        connections.clear();
    }
}

/// Le nom d'outil exposé au modèle combine serveur et outil, avec un
/// séparateur impossible dans un nom de serveur.
fn split_prefixed(name: &str) -> Result<(String, String)> {
    let rest = name
        .strip_prefix("mcp_")
        .ok_or_else(|| Error::Mcp(format!("{name} n'est pas un outil MCP")))?;
    rest.split_once("__")
        .map(|(server, tool)| (server.to_string(), tool.to_string()))
        .ok_or_else(|| Error::Mcp(format!("nom d'outil MCP invalide : {name}")))
}

fn sanitize(name: &str) -> String {
    let mut out = String::new();
    let mut previous_underscore = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            previous_underscore = false;
        } else if !previous_underscore {
            out.push('_');
            previous_underscore = true;
        }
    }
    out
}

/// Extrait le texte utile d'une réponse `tools/call`.
fn render_result(value: &Value) -> String {
    let content = value.get("content").and_then(|c| c.as_array());
    let Some(items) = content else {
        return value.to_string();
    };
    let mut parts = Vec::new();
    for item in items {
        if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
            parts.push(text.to_string());
        }
    }
    if parts.is_empty() {
        value.to_string()
    } else {
        parts.join("\n")
    }
}

async fn notify(connection: &mut Connection, method: &str, params: Value) -> Result<()> {
    let payload = json!({"jsonrpc": "2.0", "method": method, "params": params});
    let mut line = payload.to_string();
    line.push('\n');
    connection.stdin.write_all(line.as_bytes()).await?;
    connection.stdin.flush().await?;
    Ok(())
}

async fn rpc(connection: &mut Connection, method: &str, params: Value) -> Result<Value> {
    let id = connection.next_id;
    connection.next_id += 1;
    let payload = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
    let mut line = payload.to_string();
    line.push('\n');
    connection
        .stdin
        .write_all(line.as_bytes())
        .await
        .map_err(|e| Error::Mcp(format!("envoi impossible : {e}")))?;
    connection.stdin.flush().await?;

    // Les serveurs peuvent envoyer des notifications avant la réponse :
    // on les ignore jusqu'à trouver le bon identifiant.
    loop {
        let mut buffer = String::new();
        let read = connection
            .reader
            .read_line(&mut buffer)
            .await
            .map_err(|e| Error::Mcp(format!("lecture impossible : {e}")))?;
        if read == 0 {
            return Err(Error::Mcp("serveur MCP fermé la connexion".into()));
        }
        let Ok(value) = serde_json::from_str::<Value>(buffer.trim()) else {
            continue;
        };
        if value.get("id").and_then(|i| i.as_u64()) != Some(id) {
            continue;
        }
        if let Some(error) = value.get("error") {
            let message = error
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("erreur inconnue");
            return Err(Error::Mcp(message.to_string()));
        }
        return Ok(value.get("result").cloned().unwrap_or(Value::Null));
    }
}

/// Exécutable déclaré par l'utilisateur pour un serveur MCP.
pub fn server_from_config(
    name: &str,
    transport: &str,
    command: Vec<PathBuf>,
    url: String,
    env: HashMap<String, String>,
) -> McpServer {
    McpServer {
        name: name.to_string(),
        transport: transport.to_string(),
        command: command
            .into_iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect(),
        url,
        env,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nom_compose_separe() {
        assert_eq!(
            split_prefixed("mcp_docker__list_containers").unwrap(),
            ("docker".to_string(), "list_containers".to_string())
        );
        assert!(split_prefixed("run_command").is_err());
        assert!(split_prefixed("mcp_sans_separateur").is_err());
    }

    #[test]
    fn nom_serveur_normalise_sans_double_tiret() {
        assert_eq!(sanitize("docker gateway"), "docker_gateway");
        assert_eq!(sanitize("a--b"), "a_b");
    }

    #[test]
    fn rendu_du_resultat() {
        let value = json!({"content": [{"type": "text", "text": "bonjour"}]});
        assert_eq!(render_result(&value), "bonjour");
    }
}