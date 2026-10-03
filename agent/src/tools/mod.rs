//! Outils : ce que Jimmy est capable de faire sur la machine.
//!
//! Chaque outil déclare trois choses : son nom, sa description et son schéma
//! JSON. Le modèle ne voit que cela. L'implémentation ne reçoit que les
//! arguments — jamais de contexte arbitraire.
//!
//! Avant toute action, l'outil passe par [`crate::permissions::Permissions`]
//! avec la capacité correspondante. Un refus produit une erreur lisible
//! renvoyée au modèle : il peut alors demander une permission ou proposer
//! autre chose, au lieu d'échouer silencieusement.

pub mod cli;
pub mod fs;
pub mod knowledge;
pub mod mcp;
pub mod net;
pub mod skills;

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, RwLock};

use crate::core::types::ToolSpec;
use crate::error::{Error, Result};
use crate::memory::MemoryStore;
use crate::permissions::{Capability, Permissions};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Contexte transmis aux outils. Volontairement restreint : ce qu'un outil
/// peut faire est decided par ce qu'il reçoit ici plus par sa vérification de
/// permission.
pub struct ToolContext {
    pub workspace: PathBuf,
    pub permissions: Arc<RwLock<Permissions>>,
    pub memory: Arc<MemoryStore>,
    pub skills: Arc<crate::skills::SkillStore>,
    pub synaptiq: Option<Arc<crate::synaptiq::SynaptiqClient>>,
}

impl ToolContext {
    pub fn check(&self, capability: Capability, target: &str) -> Result<()> {
        let permissions = self.permissions.read().map_err(|_| Error::Tool("permissions illisibles".into()))?;
        permissions.check(capability, target)
    }

    /// Résout un chemin fourni par le modèle : relatif = relatif au workspace,
    /// absolu = accepté tel quel puis vérifié par les permissions.
    pub fn resolve(&self, path: &str) -> PathBuf {
        let candidate = PathBuf::from(path);
        if candidate.is_absolute() {
            candidate
        } else {
            self.workspace.join(candidate)
        }
    }
}

pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> serde_json::Value;
    fn capability(&self) -> Capability;
    fn call<'a>(
        &'a self,
        args: &'a serde_json::Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<String>>;

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters(),
        }
    }
}

/// Catalogue d'outils. L'agent interroge le registre, jamais l'inverse.
///
/// Le catalogue est modifiable après le démarrage : les outils MCP arrivent
/// quand leurs serveurs ont répondu, ou quand Jimmy en ajoute un. Verrou
/// synchrone, jamais tenu à travers un `await` (on clone les `Arc`).
#[derive(Default)]
pub struct ToolRegistry {
    tools: RwLock<Vec<Arc<dyn Tool>>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        ToolRegistry::default()
    }

    fn snapshot(&self) -> Vec<Arc<dyn Tool>> {
        self.tools.read().map(|t| t.clone()).unwrap_or_default()
    }

    pub fn register(&self, tool: Arc<dyn Tool>) {
        let name = tool.name().to_string();
        if let Ok(mut tools) = self.tools.write() {
            tools.retain(|t| t.name() != name);
            tools.push(tool);
        }
    }

    /// Retire les outils dont le nom commence par `prefix` (ex. les outils
    /// d'un serveur MCP remplacé).
    pub fn unregister_prefix(&self, prefix: &str) {
        if let Ok(mut tools) = self.tools.write() {
            tools.retain(|t| !t.name().starts_with(prefix));
        }
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.snapshot().into_iter().find(|t| t.name() == name)
    }

    pub fn specs(&self) -> Vec<ToolSpec> {
        self.snapshot().iter().map(|t| t.spec()).collect()
    }

    pub fn names(&self) -> Vec<String> {
        self.snapshot().iter().map(|t| t.name().to_string()).collect()
    }

    pub fn len(&self) -> usize {
        self.snapshot().len()
    }

    pub fn is_empty(&self) -> bool {
        self.snapshot().is_empty()
    }

    /// Enregistre les outils de base. Appelé au démarrage ; les outils MCP
    /// viennent s'y ajouter ensuite.
    pub fn register_defaults(&self, deps: &ToolDeps) {
        self.register(Arc::new(fs::ListDirectory));
        self.register(Arc::new(fs::ReadFile));
        self.register(Arc::new(fs::WriteFile));
        self.register(Arc::new(fs::SearchFiles));
        self.register(Arc::new(cli::RunCommand));
        self.register(Arc::new(net::HttpRequest::new(deps.http.clone())));
        self.register(Arc::new(net::OpenBrowser));
        self.register(Arc::new(knowledge::SearchMemory));
        self.register(Arc::new(knowledge::Remember));
        self.register(Arc::new(knowledge::SynaptiqSearch));
        self.register(Arc::new(knowledge::SynaptiqRemember));
        self.register(Arc::new(skills::ListSkills));
        self.register(Arc::new(skills::ReadSkill));
        self.register(Arc::new(skills::CreateSkill));
        self.register(Arc::new(skills::UpdateSkill));
    }
}

/// Dépendances externes des outils.
#[derive(Clone)]
pub struct ToolDeps {
    pub http: reqwest::Client,
}

// ── Aides partagées ──────────────────────────────────────────────────────────

/// Lit un champ chaîne d'un objet d'arguments JSON.
pub fn arg_str(args: &serde_json::Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.trim().is_empty())
}

pub fn arg_u64(args: &serde_json::Value, key: &str, default: u64) -> u64 {
    args.get(key)
        .and_then(|v| v.as_u64())
        .unwrap_or(default)
}

/// Tronque une sortie d'outil : les modèles ont une fenêtre limitée et une
/// sortie de 100 Ko de `dir` n'aide personne.
pub fn clip(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let kept: String = text.chars().take(max_chars).collect();
    format!("{kept}\n… (sortie tronquée, {max_chars} caractères conservés)")
}

/// Volume de texte dupliqué en sortie d'un tool avant l'appliquer au modèle.
pub const MAX_TOOL_OUTPUT: usize = 12_000;

pub fn schema(properties: serde_json::Value, required: &[&str]) -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": properties,
        "required": required,
    })
}