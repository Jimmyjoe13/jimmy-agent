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

    /// État de chaque serveur pour l'interface : processus vivant ou non, et
    /// outils déjà enregistrés. `tools` = (nom préfixé, description) des outils
    /// du registre. Ne bloque jamais : si une connexion ou un appel tient le
    /// verrou (jusqu'à 90 s au lancement), l'état est « occupé ».
    pub fn status(&self, tools: &[(String, String)]) -> Vec<McpServerStatus> {
        // Vivant = processus enfant qui n'a pas terminé.
        let alive: Option<HashMap<String, bool>> = self.connections.try_lock().ok().map(|mut connections| {
            connections
                .iter_mut()
                .map(|(name, c)| (name.clone(), matches!(c.child.try_wait(), Ok(None))))
                .collect()
        });
        self.servers()
            .into_iter()
            .map(|server| {
                let state = match &alive {
                    None => "busy",
                    Some(map) if map.get(&server.name) == Some(&true) => "connected",
                    Some(_) => "stopped",
                };
                let prefix = format!("mcp_{}__", sanitize(&server.name));
                let server_tools = tools
                    .iter()
                    .filter_map(|(name, description)| {
                        name.strip_prefix(&prefix).map(|short| McpToolInfo {
                            name: short.to_string(),
                            description: description.clone(),
                        })
                    })
                    .collect();
                let mut env_keys: Vec<String> = server.env.keys().cloned().collect();
                env_keys.sort();
                McpServerStatus {
                    launch: if server.is_stdio() { mask_command(&server.command) } else { mask_command(&[server.url.clone()]) },
                    name: server.name,
                    transport: server.transport,
                    env_keys,
                    state,
                    tools: server_tools,
                }
            })
            .collect()
    }

    pub async fn shutdown(&self) {
        let mut connections = self.connections.lock().await;
        for (_, connection) in connections.iter_mut() {
            let _ = connection.child.kill().await;
        }
        connections.clear();
    }
}

/// Un serveur MCP tel que l'interface l'affiche (vue Skills → Serveurs MCP).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerStatus {
    pub name: String,
    pub transport: String,
    /// Commande de lancement, secrets masqués (`mask_command`).
    pub launch: String,
    /// Noms des variables d'environnement passées au serveur ; jamais leurs
    /// valeurs, qui sont souvent des clés.
    pub env_keys: Vec<String>,
    /// `connected`, `stopped` (pas encore lancé ou processus mort), `busy`
    /// (connexion ou appel en cours) ou `disabled` (désactivé en config).
    pub state: &'static str,
    pub tools: Vec<McpToolInfo>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct McpToolInfo {
    /// Nom d'origine, sans le préfixe `mcp_<serveur>__`.
    pub name: String,
    pub description: String,
}

/// Options dont la valeur suivante est un secret.
const SECRET_FLAGS: &[&str] = &[
    "--header", "-h", "--token", "--api-key", "--apikey", "--key", "--password", "--secret", "--auth",
    "--authorization", "--bearer",
];

/// Mots qui signalent un secret dans un argument (`X-API-Key: …`, `token=…`).
const SECRET_HINTS: &[&str] = &["key", "token", "secret", "password", "passwd", "authorization", "bearer", "auth"];

/// Commande lisible, secrets masqués. Une commande MCP porte souvent une clé
/// (en-tête `--header "X-API-Key: …"` de `mcp-remote`, `--token=…`, jeton
/// dans l'URL) : elle ne doit jamais apparaître en clair dans l'interface.
/// Le nom de l'en-tête ou de l'option reste visible, seule la valeur est cachée.
pub fn mask_command(args: &[String]) -> String {
    let mut out = Vec::with_capacity(args.len());
    let mut hide_next = false;
    for arg in args {
        if hide_next {
            hide_next = false;
            out.push(mask_value(arg, true));
            continue;
        }
        let lower = arg.to_lowercase();
        if SECRET_FLAGS.contains(&lower.as_str()) {
            hide_next = true;
            out.push(arg.clone());
            continue;
        }
        out.push(mask_value(arg, false));
    }
    out.join(" ")
}

/// Masque la valeur d'un argument : après `:` ou `=` si ce qui précède parle
/// d'un secret (ou si `always`), en entier sinon quand `always`.
fn mask_value(arg: &str, always: bool) -> String {
    // Identifiants dans une URL (`postgres://user:motdepasse@hôte`) : masqués,
    // puis le reste de l'argument est traité normalement.
    let masked_userinfo = arg.split_once("://").and_then(|(scheme, rest)| {
        let (userinfo, host) = rest.split_once('@')?;
        (!userinfo.contains('/')).then(|| {
            let user = userinfo.split(':').next().unwrap_or("");
            format!("{scheme}://{user}:••••@{host}")
        })
    });
    let arg = masked_userinfo.as_deref().unwrap_or(arg);
    // Jeton dans une URL : chaque paramètre sensible de la requête.
    if let Some((base, query)) = arg.split_once('?') {
        let params: Vec<String> = query
            .split('&')
            .map(|p| match p.split_once('=') {
                Some((k, _)) if always || is_secret_name(k) => format!("{k}=••••"),
                _ => p.to_string(),
            })
            .collect();
        return format!("{base}?{}", params.join("&"));
    }
    // `X-API-Key: valeur`, `--token=valeur`, `Authorization: Bearer valeur`.
    for sep in [':', '='] {
        if let Some((name, _)) = arg.split_once(sep) {
            // `http://…` ou `C:\…` : un `:` qui n'introduit pas de valeur.
            let is_path_or_url = sep == ':' && (name.len() == 1 || arg[name.len()..].starts_with("://"));
            if !is_path_or_url && (always || is_secret_name(name)) {
                return if sep == '=' { format!("{name}=••••") } else { format!("{name}: ••••") };
            }
        }
    }
    if always {
        "••••".to_string()
    } else {
        arg.to_string()
    }
}

fn is_secret_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    SECRET_HINTS.iter().any(|hint| lower.contains(hint))
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

/// Préfixe des outils d'un serveur dans le registre : `mcp_<serveur>__`.
pub fn tool_prefix(server: &str) -> String {
    format!("mcp_{}__", sanitize(server))
}

/// Outil MCP « proxy » (`mcp_<serveur>__<outil>`), par opposition aux outils
/// de Jimmy dont le nom commence aussi par `mcp_` (`mcp_add_server`, `mcp_call`…).
pub fn is_proxy_tool(name: &str) -> bool {
    name.starts_with("mcp_") && name.contains("__")
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

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn la_commande_affichee_ne_montre_aucun_secret() {
        // Forme réelle d'un serveur ajouté par Jimmy (mcp-remote + en-tête).
        let shown = mask_command(&args(&[
            "npx", "-y", "mcp-remote", "http://127.0.0.1:8010/mcp", "--header", "X-API-Key: abc123secret",
        ]));
        assert!(!shown.contains("abc123secret"), "{shown}");
        assert!(shown.contains("X-API-Key: ••••"), "{shown}");
        assert!(shown.contains("mcp-remote http://127.0.0.1:8010/mcp"), "{shown}");
        // Options « --token=… », « --api-key … », jeton dans l'URL, Bearer.
        for (input, secret) in [
            (args(&["srv", "--token=tok42"]), "tok42"),
            (args(&["srv", "--api-key", "k-99"]), "k-99"),
            (args(&["srv", "https://x.io/mcp?api_key=zz77"]), "zz77"),
            (args(&["srv", "-H", "Authorization: Bearer bb55"]), "bb55"),
            (args(&["srv", "postgres://jim:pw66@db:5432/x"]), "pw66"),
        ] {
            let shown = mask_command(&input);
            assert!(!shown.contains(secret), "{shown}");
        }
        // Rien de sensible : la commande reste lisible telle quelle.
        assert_eq!(
            mask_command(&args(&["npx", "-y", "mcp-obsidian", "C:\\Obsidian\\Jimmy"])),
            "npx -y mcp-obsidian C:\\Obsidian\\Jimmy"
        );
    }

    #[tokio::test]
    async fn l_etat_rattache_chaque_outil_a_son_serveur() {
        let server = |name: &str| McpServer {
            name: name.into(),
            transport: "stdio".into(),
            command: args(&["npx", "x"]),
            url: String::new(),
            env: HashMap::from([("API_TOKEN".to_string(), "secret".to_string())]),
        };
        let registry = McpRegistry::new(vec![server("obsidian"), server("my server")]);
        let tools = vec![
            ("mcp_obsidian__search".to_string(), "Cherche".to_string()),
            ("mcp_my_server__run".to_string(), "Lance".to_string()),
            ("read_file".to_string(), "Local".to_string()),
        ];
        let status = registry.status(&tools);
        assert_eq!(status.len(), 2);
        assert_eq!(status[0].name, "obsidian");
        // Jamais lancé dans ce test : arrêté, pas connecté.
        assert_eq!(status[0].state, "stopped");
        assert_eq!(status[0].tools.len(), 1);
        assert_eq!(status[0].tools[0].name, "search");
        assert_eq!(status[1].tools[0].name, "run");
        // Les variables d'environnement : leurs noms seulement, jamais les valeurs.
        assert_eq!(status[0].env_keys, vec!["API_TOKEN".to_string()]);
    }

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