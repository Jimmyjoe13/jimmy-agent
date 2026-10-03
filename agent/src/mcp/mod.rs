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
use std::sync::{Arc, RwLock};
use std::time::Duration;

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
    /// Nom exposé au modèle : `mcp_<serveur>__<outil>`, normalisé.
    pub name: String,
    /// Nom d'origine côté serveur, utilisé pour l'appel (il peut contenir des
    /// caractères refusés par l'API du modèle, comme `.` ou `/`).
    pub remote_name: String,
    pub description: String,
    pub input_schema: Value,
}

/// Délais d'attente. Sans eux, un serveur muet bloquait l'agent indéfiniment.
/// L'initialisation est longue : `npx -y` télécharge le paquet au premier
/// lancement.
const INIT_TIMEOUT: Duration = Duration::from_secs(90);
const LIST_TIMEOUT: Duration = Duration::from_secs(30);
const CALL_TIMEOUT: Duration = Duration::from_secs(120);

/// Connexion active à un serveur MCP.
struct Connection {
    child: Child,
    stdin: tokio::process::ChildStdin,
    reader: BufReader<tokio::process::ChildStdout>,
    next_id: u64,
}

pub struct McpRegistry {
    /// Verrou synchrone : jamais tenu à travers un `await` (on clone la liste).
    servers: RwLock<Vec<McpServer>>,
    connections: Mutex<HashMap<String, Connection>>,
}

impl McpRegistry {
    pub fn new(servers: Vec<McpServer>) -> Arc<Self> {
        Arc::new(McpRegistry {
            servers: RwLock::new(servers),
            connections: Mutex::new(HashMap::new()),
        })
    }

    fn servers(&self) -> Vec<McpServer> {
        self.servers.read().map(|s| s.clone()).unwrap_or_default()
    }

    fn find(&self, name: &str) -> Option<McpServer> {
        // Le nom peut arriver normalisé (depuis un nom d'outil préfixé).
        self.servers()
            .into_iter()
            .find(|s| s.name == name || sanitize(&s.name) == name)
    }

    pub fn server_names(&self) -> Vec<String> {
        self.servers().iter().map(|s| s.name.clone()).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.servers().is_empty()
    }

    /// Ajoute (ou remplace) un serveur. Une connexion existante portant le
    /// même nom est fermée : la prochaine utilisation relancera le processus.
    pub async fn add_server(&self, server: McpServer) {
        if let Ok(mut servers) = self.servers.write() {
            servers.retain(|s| s.name != server.name);
            servers.push(server.clone());
        }
        let mut connections = self.connections.lock().await;
        if let Some(mut old) = connections.remove(&server.name) {
            let _ = old.child.kill().await;
        }
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

        let mut command = launcher(program, args);
        command
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

        let _result = rpc_within(
            &mut connection,
            "initialize",
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {"tools": {}},
                "clientInfo": {"name": "jimmy", "version": env!("CARGO_PKG_VERSION")},
            }),
            INIT_TIMEOUT,
        )
        .await?;
        // La spécification impose cet accusé avant tout appel ultérieur.
        notify(&mut connection, "notifications/initialized", json!({})).await?;

        connections.insert(server.name.clone(), connection);
        log::info!("[mcp] serveur prêt : {}", server.name);
        Ok(server.name.clone())
    }

    /// Outils exposés par l'ensemble des serveurs. Un serveur en panne est
    /// journalisé et ignoré : il ne doit pas priver Jimmy des autres.
    pub async fn list_tools(&self) -> Vec<McpTool> {
        let mut out = Vec::new();
        for server in self.servers() {
            match self.list_server_tools(&server.name).await {
                Ok(tools) => out.extend(tools),
                Err(error) => log::warn!("[mcp] {} indisponible : {error}", server.name),
            }
        }
        out
    }

    /// Outils d'un serveur, en le démarrant si besoin.
    pub async fn list_server_tools(&self, server_name: &str) -> Result<Vec<McpTool>> {
        let server = self
            .find(server_name)
            .ok_or_else(|| Error::Mcp(format!("serveur inconnu : {server_name}")))?;
        let name = server.name.clone();
        self.ensure(&server).await?;
        let mut connections = self.connections.lock().await;
        let connection = connections
            .get_mut(&name)
            .ok_or_else(|| Error::Mcp("connexion perdue".into()))?;
        let value = rpc_within(connection, "tools/list", json!({}), LIST_TIMEOUT).await?;
        let tools = value
            .get("tools")
            .and_then(|t| t.as_array())
            .cloned()
            .unwrap_or_default();
        let mut out = Vec::new();
        for tool in tools {
            let Some(tool_name) = tool.get("name").and_then(|n| n.as_str()) else {
                continue;
            };
            out.push(McpTool {
                server: name.clone(),
                name: format!("mcp_{}__{}", sanitize(&name), sanitize(tool_name)),
                remote_name: tool_name.to_string(),
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
        Ok(out)
    }

    /// Appelle un outil exposé par un serveur, via son nom préfixé.
    pub async fn call_tool(&self, prefixed_name: &str, arguments: &Value) -> Result<String> {
        let (server_name, tool_name) = split_prefixed(prefixed_name)?;
        self.call(&server_name, &tool_name, arguments).await
    }

    /// Appelle `tool` (nom d'origine) sur `server_name`.
    pub async fn call(&self, server_name: &str, tool: &str, arguments: &Value) -> Result<String> {
        let server = self
            .find(server_name)
            .ok_or_else(|| Error::Mcp(format!("serveur inconnu : {server_name}")))?;
        self.ensure(&server).await?;
        let mut connections = self.connections.lock().await;
        let connection = connections
            .get_mut(&server.name)
            .ok_or_else(|| Error::Mcp("connexion perdue".into()))?;
        let outcome = rpc_within(
            connection,
            "tools/call",
            json!({ "name": tool, "arguments": arguments }),
            CALL_TIMEOUT,
        )
        .await;
        if outcome.is_err() {
            // Après un délai dépassé, la réponse tardive désynchroniserait le
            // flux : on repart d'un processus neuf au prochain appel.
            if let Some(mut dead) = connections.remove(&server.name) {
                let _ = dead.child.kill().await;
            }
        }
        let value = outcome?;
        if value.get("isError").and_then(|e| e.as_bool()) == Some(true) {
            return Err(Error::Mcp(render_result(&value)));
        }
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

/// Construit la commande de lancement. Sous Windows, `npx`, `uvx` et la
/// plupart des lanceurs sont des scripts `.cmd` : `CreateProcess` ne les trouve
/// pas sans extension. On passe alors par `cmd /C`, sauf pour un `.exe`
/// explicite.
fn launcher(program: &str, args: &[String]) -> Command {
    #[cfg(windows)]
    {
        if !program.to_ascii_lowercase().ends_with(".exe") {
            let mut command = Command::new("cmd");
            command.arg("/D").arg("/C").arg(program).args(args);
            return command;
        }
    }
    let mut command = Command::new(program);
    command.args(args);
    command
}

async fn rpc_within(connection: &mut Connection, method: &str, params: Value, limit: Duration) -> Result<Value> {
    tokio::time::timeout(limit, rpc(connection, method, params))
        .await
        .map_err(|_| Error::Mcp(format!("{method} : pas de réponse en {} s", limit.as_secs())))?
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
    fn nom_d_outil_normalise_pour_l_api() {
        // L'API du modèle n'accepte que [a-zA-Z0-9_-] dans un nom d'outil.
        assert_eq!(sanitize("files.read/v2"), "files_read_v2");
    }

    #[test]
    fn rendu_du_resultat() {
        let value = json!({"content": [{"type": "text", "text": "bonjour"}]});
        assert_eq!(render_result(&value), "bonjour");
    }
}