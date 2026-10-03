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

/// Convertit l'entrée de configuration en serveur MCP.
pub fn server_from_settings(config: &McpServerConfig) -> McpServer {
    McpServer {
        name: config.name.clone(),
        transport: config.transport.clone(),
        command: config.command.clone(),
        url: config.url.clone(),
        env: config.env.iter().map(|(k, v)| (k.clone(), v.clone())).collect::<HashMap<_, _>>(),
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
         immédiatement. Exemple de commande : [\"npx\", \"-y\", \"@modelcontextprotocol/server-filesystem\", \"C:/Users/jimmy/Documents\"]. \
         Le serveur est enregistré dans la configuration et relancé aux prochains démarrages."
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
