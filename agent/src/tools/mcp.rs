//! Outils MCP : pont entre les serveurs MCP et le registre d'outils.
//!
//! Chaque outil d'un serveur devient un [`McpProxy`] : même nom, même
//! description, même schéma, vus par le modèle comme n'importe quel outil
//! natif. L'outil [`McpAddServer`] permet à Jimmy de brancher un nouveau
//! serveur en cours de conversation ; la configuration est persistée.
//!
//! Sécurité : un serveur MCP est un processus arbitraire qui peut tout faire.
//! Ajouter un serveur **et** appeler l'un de ses outils exigent la capacité
//! EXÉCUTION, avec la commande ou `mcp:<serveur>` comme cible — ce qui permet
//! d'interdire un serveur précis par la liste `deny_commands`.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, RwLock, Weak};

use serde_json::{json, Value};

use super::{arg_str, clip, schema, BoxFuture, Tool, ToolContext, ToolRegistry, MAX_TOOL_OUTPUT};
use crate::config::{McpServerConfig, Settings};
use crate::error::{Error, Result};
use crate::mcp::{McpRegistry, McpServer, McpTool};
use crate::paths::Paths;
use crate::permissions::Capability;

// ── Outils MCP à la demande ─────────────────────────────────────────────────
//
// Les 79 définitions d'outils MCP pesaient ~9 500 jetons à CHAQUE appel au
// modèle (mesuré le 4 octobre ; 1 900 pour les 16 outils de Jimmy) : des appels
// lents et chers, même quand aucun outil MCP ne sert. Le modèle ne reçoit plus
// que leurs noms (dans le prompt) et ces deux outils : `mcp_list_tools` pour le
// détail d'un serveur, `mcp_call` pour appeler un outil. L'exécution passe par
// le même proxy, donc les mêmes permissions.

/// Détail (description, schéma) des outils d'un serveur MCP.
pub struct McpListTools {
    registry: Weak<ToolRegistry>,
}

impl McpListTools {
    pub fn new(registry: Weak<ToolRegistry>) -> Self {
        McpListTools { registry }
    }
}

impl Tool for McpListTools {
    fn name(&self) -> &str {
        "mcp_list_tools"
    }
    fn description(&self) -> &str {
        "Détail des outils d'un serveur MCP connecté : nom, description et schéma des arguments. À appeler avant mcp_call si tu ne connais pas les arguments attendus."
    }
    fn parameters(&self) -> Value {
        schema(
            json!({
                "server": {"type": "string", "description": "Nom du serveur MCP (voir la liste des serveurs dans le contexte)."},
                "filter": {"type": "string", "description": "Ne garder que les outils dont le nom contient ce texte (facultatif)."}
            }),
            &["server"],
        )
    }
    fn capability(&self) -> Capability {
        Capability::Read
    }
    fn call<'a>(&'a self, args: &'a Value, _ctx: &'a ToolContext) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            let server = arg_str(args, "server").ok_or_else(|| Error::Tool("« server » manquant".into()))?;
            let filter = arg_str(args, "filter").unwrap_or_default().to_lowercase();
            let registry = self.registry.upgrade().ok_or_else(|| Error::Tool("registre indisponible".into()))?;
            let prefix = crate::mcp::tool_prefix(&server);
            let tools: Vec<Value> = registry
                .specs()
                .into_iter()
                .filter_map(|s| {
                    let short = s.name.strip_prefix(&prefix)?.to_string();
                    (filter.is_empty() || short.to_lowercase().contains(&filter))
                        .then(|| json!({ "tool": short, "description": s.description, "arguments": s.parameters }))
                })
                .collect();
            if tools.is_empty() {
                return Err(Error::Tool(format!("aucun outil pour le serveur « {server} » (filtre « {filter} »)")));
            }
            Ok(clip(&serde_json::to_string(&tools).unwrap_or_default(), MAX_TOOL_OUTPUT))
        })
    }
}

/// Appelle un outil d'un serveur MCP.
pub struct McpCall {
    registry: Weak<ToolRegistry>,
}

impl McpCall {
    pub fn new(registry: Weak<ToolRegistry>) -> Self {
        McpCall { registry }
    }
}

impl Tool for McpCall {
    fn name(&self) -> &str {
        "mcp_call"
    }
    fn description(&self) -> &str {
        "Appelle un outil d'un serveur MCP connecté. Les outils disponibles sont listés par serveur dans le contexte ; leurs arguments s'obtiennent avec mcp_list_tools."
    }
    fn parameters(&self) -> Value {
        schema(
            json!({
                "server": {"type": "string", "description": "Nom du serveur MCP."},
                "tool": {"type": "string", "description": "Nom de l'outil, sans préfixe (ex. « search_notes »)."},
                "arguments": {"type": "object", "description": "Arguments de l'outil, selon son schéma."}
            }),
            &["server", "tool"],
        )
    }
    fn capability(&self) -> Capability {
        Capability::Execute
    }
    fn call<'a>(&'a self, args: &'a Value, ctx: &'a ToolContext) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            let server = arg_str(args, "server").ok_or_else(|| Error::Tool("« server » manquant".into()))?;
            let tool = arg_str(args, "tool").ok_or_else(|| Error::Tool("« tool » manquant".into()))?;
            let arguments = args.get("arguments").cloned().unwrap_or_else(|| json!({}));
            let registry = self.registry.upgrade().ok_or_else(|| Error::Tool("registre indisponible".into()))?;
            let name = format!("{}{}", crate::mcp::tool_prefix(&server), tool);
            let proxy = registry.get(&name).ok_or_else(|| {
                Error::Tool(format!("outil inconnu : « {tool} » sur « {server} » (voir mcp_list_tools)"))
            })?;
            // Le proxy vérifie la permission EXÉCUTION `mcp:<serveur>`.
            match proxy.call(&arguments, ctx).await {
                Ok(text) => Ok(text),
                // Arguments refusés : le schéma attendu aide le modèle à se corriger.
                Err(error) => Err(Error::Tool(format!(
                    "{error} — arguments attendus : {}",
                    clip(&proxy.parameters().to_string(), 1500)
                ))),
            }
        })
    }
}

/// Motifs d'arguments qui portent une clé (`--header`, `--token`…).
fn is_secret_flag(arg: &str) -> bool {
    let low = arg.to_lowercase();
    low == "--header"
        || low == "-h"
        || low == "--token"
        || low == "--api-key"
        || low == "--auth"
        || low.starts_with("--header=")
        || low.starts_with("--token=")
        || low.starts_with("--api-key=")
        || low.starts_with("--auth=")
}

/// Un nom de paramètre parle-t-il d'un secret (`Authorization`, `X-API-Key`) ?
fn is_secret_param(name: &str) -> bool {
    let low = name.to_lowercase();
    low.contains("key") || low.contains("token") || low.contains("auth") || low.contains("secret") || low.contains("bearer")
}

/// Une valeur ressemble-t-elle à une clé en clair (longue, sans blanc, pas
/// une référence `${VAR}` ni `secret:nom`) ?
fn looks_like_clear_secret(value: &str) -> bool {
    let v = value.trim().trim_matches(|c| c == '"' || c == '\'');
    v.chars().count() >= 12
        && !v.chars().any(char::is_whitespace)
        && !v.contains("${")
        && !v.starts_with("secret:")
}

/// Repère une clé d'API écrite en clair dans la commande ou l'environnement
/// d'un serveur MCP à brancher (cas réel du 13 octobre : `emile` recréé avec
/// `Authorization: Bearer <clé>` en clair, piège 108 contourné).
/// Convention attendue : `${VAR}` dans la commande, `secret:nom` dans `env`
/// (coffre `%USERPROFILE%\.secrets`, comme `aggregate`).
/// `None` = rien de secret, rien à signaler.
pub fn secret_in_server_args(command: &[String], env: &BTreeMap<String, String>) -> Option<String> {
    let mut i = 0;
    while i < command.len() {
        let arg = command[i].trim();
        // Forme `--flag=valeur` : la valeur est dans le même argument.
        let inline = arg.split_once('=').filter(|(flag, _)| is_secret_flag(flag));
        let value = if let Some((_, v)) = inline {
            Some(v.to_string())
        } else if is_secret_flag(arg) {
            command.get(i + 1).cloned()
        } else {
            None
        };
        if let Some(v) = value {
            // `Nom: valeur` ou `Nom=valeur` : seul un nom qui parle d'un
            // secret compte (`Content-Type: application/json` reste libre).
            let param = v.split([':', '=']).next().unwrap_or("").trim();
            let token = v.rsplit([':', '=', ' ']).next().unwrap_or("").trim();
            if (is_secret_param(param) || is_secret_flag(arg)) && looks_like_clear_secret(token) {
                return Some(format!(
                    "clé en clair dans la commande (« …{} ») : jamais de secret dans `config.json` ni sur la ligne de commande (piège 108). \
                     Demande à l'utilisateur de ranger la clé dans le coffre `%USERPROFILE%\\.secrets\\<nom>.json` (champ `api_key`), \
                     puis déclare `--header \"Authorization: Bearer ${{VAR}}\"` avec `\"env\": {{\"VAR\": \"secret:<nom>\"}}` comme le serveur `aggregate`",
                    &token.chars().take(4).collect::<String>()
                ));
            }
        }
        // Jeton connu, n'importe où dans la ligne (clé déjà exposée ailleurs).
        let low = arg.to_lowercase();
        if ["sk-or-", "xoxb-", "ghp_", "gsk_"].iter().any(|p| low.contains(p)) {
            return Some("jeton connu dans la commande : même règle, coffre + `${VAR}`, jamais en clair".to_string());
        }
        i += 1;
    }
    for (key, value) in env {
        if crate::mcp::is_secret_name(key) && looks_like_clear_secret(value) {
            return Some(format!(
                "clé en clair dans `env` ({key}) : utilise `\"{key}\": \"secret:<nom>\"` avec la clé dans le coffre, comme `aggregate`"
            ));
        }
    }
    None
}

/// Convertit l'entrée de configuration en serveur MCP.
pub fn server_from_settings(config: &McpServerConfig) -> McpServer {
    // `secret:nom` → coffre `.secrets` (la clé n'est ni dans `config.json`
    // ni dans la ligne de commande : `mcp-remote` lit `${VAR}` lui-même).
    let env: HashMap<String, String> = config
        .env
        .iter()
        .map(|(k, v)| (k.clone(), crate::config::resolve_secret_ref(v)))
        .collect();
    // Les valeurs secrètes sont masquées si un outil les fait remonter.
    crate::sensitive::register_secrets(
        env.iter().filter(|(k, _)| crate::mcp::is_secret_name(k)).map(|(_, v)| v.clone()),
    );
    McpServer {
        name: config.name.clone(),
        transport: config.transport.clone(),
        command: config.command.clone(),
        url: config.url.clone(),
        env,
    }
}

/// Connecte tous les serveurs et enregistre leurs outils. Renvoie le nombre
/// d'outils ajoutés.
pub async fn connect_all(mcp: &Arc<McpRegistry>, registry: &ToolRegistry) -> usize {
    let tools = mcp.list_tools().await;
    let count = tools.len();
    for tool in tools {
        registry.register(Arc::new(McpProxy {
            mcp: mcp.clone(),
            tool,
        }));
    }
    count
}

/// Un outil distant, exposé comme un outil local.
pub struct McpProxy {
    mcp: Arc<McpRegistry>,
    tool: McpTool,
}

impl Tool for McpProxy {
    fn name(&self) -> &str {
        &self.tool.name
    }

    fn description(&self) -> &str {
        &self.tool.description
    }

    fn parameters(&self) -> Value {
        self.tool.input_schema.clone()
    }

    fn capability(&self) -> Capability {
        Capability::Execute
    }

    fn call<'a>(&'a self, args: &'a Value, ctx: &'a ToolContext) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            ctx.check(Capability::Execute, &format!("mcp:{}", self.tool.server))?;
            let output = self.mcp.call(&self.tool.server, &self.tool.remote_name, args).await?;
            Ok(clip(&output, MAX_TOOL_OUTPUT))
        })
    }
}

/// `mcp_add_server` : branche un serveur MCP stdio, liste ses outils, les
/// rend disponibles immédiatement et enregistre le serveur dans la
/// configuration.
pub struct McpAddServer {
    mcp: Arc<McpRegistry>,
    /// Référence faible : le registre possède cet outil, une référence forte
    /// créerait un cycle.
    registry: Weak<ToolRegistry>,
    settings: Arc<RwLock<Settings>>,
    paths: Paths,
}

impl McpAddServer {
    pub fn new(
        mcp: Arc<McpRegistry>,
        registry: Weak<ToolRegistry>,
        settings: Arc<RwLock<Settings>>,
        paths: Paths,
    ) -> Self {
        McpAddServer {
            mcp,
            registry,
            settings,
            paths,
        }
    }

    fn persist(&self, config: McpServerConfig) -> Result<()> {
        let mut settings = self
            .settings
            .write()
            .map_err(|_| Error::Config("verrou des paramètres empoisonné".into()))?;
        settings.mcp_servers.retain(|s| s.name != config.name);
        settings.mcp_servers.push(config);
        settings.save(&self.paths)
    }
}

impl Tool for McpAddServer {
    fn name(&self) -> &str {
        "mcp_add_server"
    }

    fn description(&self) -> &str {
        "Branche un serveur MCP (transport stdio) et rend ses outils disponibles \
         immédiatement. Exemple de commande : [\"npx\", \"-y\", \"@modelcontextprotocol/server-filesystem\", \"C:/Users/user/Documents\"]. \
         Le serveur est enregistré dans la configuration et relancé aux prochains démarrages. \
         Clé d'API : jamais en clair dans la commande ni dans `env` (piège 108) — \
         `--header \"Authorization: Bearer ${NOM_VAR}\"` avec `\"env\": {\"NOM_VAR\": \"secret:nom\"}`, \
         la clé rangée par l'utilisateur dans `%USERPROFILE%\\.secrets\\nom.json` (champ `api_key`), comme le serveur `aggregate`."
    }

    fn parameters(&self) -> Value {
        schema(
            json!({
                "name": {"type": "string", "description": "Nom court du serveur (ex. filesystem, github)."},
                "command": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Programme puis arguments, un élément par argument."
                },
                "env": {
                    "type": "object",
                    "additionalProperties": {"type": "string"},
                    "description": "Variables d'environnement facultatives."
                }
            }),
            &["name", "command"],
        )
    }

    fn capability(&self) -> Capability {
        Capability::Execute
    }

    fn call<'a>(&'a self, args: &'a Value, ctx: &'a ToolContext) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            let name = arg_str(args, "name").ok_or_else(|| Error::Tool("« name » manquant".into()))?;
            let command: Vec<String> = args
                .get("command")
                .and_then(|c| c.as_array())
                .map(|items| items.iter().filter_map(|i| i.as_str().map(String::from)).collect())
                .unwrap_or_default();
            if command.is_empty() {
                return Err(Error::Tool("« command » doit contenir au moins le programme".into()));
            }
            let env: BTreeMap<String, String> = args
                .get("env")
                .and_then(|e| e.as_object())
                .map(|map| {
                    map.iter()
                        .filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_string())))
                        .collect()
                })
                .unwrap_or_default();

            // Lancer un serveur = exécuter une commande : même permission.
            ctx.check(Capability::Execute, &command.join(" "))?;

            // Piège 108 : une clé passée en argument ou écrite dans
            // `config.json` est lisible par Jimmy lui-même et part au modèle.
            // On refuse avant de persister, avec la convention à suivre.
            if let Some(problem) = secret_in_server_args(&command, &env) {
                return Err(Error::Tool(format!("REFUSÉ : {problem}")));
            }

            let config = McpServerConfig {
                name: name.clone(),
                transport: "stdio".into(),
                command,
                url: String::new(),
                env,
                enabled: true,
            };
            self.mcp.add_server(server_from_settings(&config)).await;

            // On ne persiste qu'un serveur qui répond : une faute de frappe ne
            // doit pas s'installer dans la configuration.
            let tools = self.mcp.list_server_tools(&name).await?;
            if let Some(registry) = self.registry.upgrade() {
                let prefix = tools
                    .first()
                    .map(|t| t.name.split("__").next().unwrap_or("").to_string() + "__")
                    .unwrap_or_default();
                if !prefix.is_empty() {
                    registry.unregister_prefix(&prefix);
                }
                for tool in &tools {
                    registry.register(Arc::new(McpProxy {
                        mcp: self.mcp.clone(),
                        tool: tool.clone(),
                    }));
                }
            }
            self.persist(config)?;

            let listing: Vec<String> = tools
                .iter()
                .map(|t| format!("- {} : {}", t.name, t.description.lines().next().unwrap_or("")))
                .collect();
            Ok(format!(
                "Serveur MCP « {name} » connecté, {} outil(s) disponible(s) dès le prochain message :\n{}",
                tools.len(),
                listing.join("\n")
            ))
        })
    }
}

#[cfg(test)]
mod server_secret_tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    /// Cas réel du 13 octobre : `emile` recréé avec sa clé en clair dans
    /// `--header` (piège 108 contourné par `mcp_add_server` lui-même).
    /// Une clé en clair dans la commande est refusée ; la convention
    /// `${VAR}` + `secret:nom` passe.
    #[test]
    fn une_cle_en_clair_dans_la_commande_est_signalee() {
        let clair = vec![
            "npx".to_string(),
            "-y".to_string(),
            "mcp-remote".to_string(),
            "https://exemple.fr/mcp".to_string(),
            "--header".to_string(),
            "Authorization: Bearer cle-emile-factice-0123456789".to_string(),
        ];
        assert!(secret_in_server_args(&clair, &env(&[])).is_some());
        // Convention coffre : `${VAR}` dans la commande, `secret:nom` dans l'env.
        let propre = vec![
            "npx".to_string(),
            "-y".to_string(),
            "mcp-remote".to_string(),
            "https://exemple.fr/mcp".to_string(),
            "--header".to_string(),
            "Authorization: Bearer ${EMILE_API_KEY}".to_string(),
        ];
        assert_eq!(
            secret_in_server_args(&propre, &env(&[("EMILE_API_KEY", "secret:emile")])),
            None
        );
        // Ni clé ni en-tête : rien à signaler.
        let simple = vec!["npx".to_string(), "-y".to_string(), "server-fichiers".to_string()];
        assert_eq!(secret_in_server_args(&simple, &env(&[])), None);
    }
}
