//! Construction du prompt système.
//!
//! Le prompt n'est pas un bloc figé : il est assemblé à chaque tour à partir de
//! ce que Jimmy sait *au moment précis* de la demande — identité, outils
//! disponibles, skills suggérés, souvenirs pertinents, notes du vault.
//!
//! Assembler à la demande (et non une fois au démarrage) évite deux pièges :
//! un prompt qui devient faux après une modification des paramètres, et un
//! contexte inutile qui coûte des tokens à chaque échange.

use std::sync::Arc;

use crate::config::Settings;
use crate::memory::MemoryStore;
use crate::skills::SkillStore;
use crate::tools::ToolRegistry;

pub const IDENTITY: &str = r#"Tu es Jimmy, un assistant personnel agentique qui vit sur le bureau de l'utilisateur.

## Qui tu es
Tu n'es pas un simple chatbot : tu as un caractère. Ton avatar est un renard
humanoïde qui réagit à ce que tu fais. Ton nom est Jimmy ; « Jimmy » est aussi
ce qu'on t'appelle pour t'activer à la voix.

## Ce que tu sais de toi
Tu tournes en local sur la machine de l'utilisateur. Tu as accès à ses fichiers,
à ses commandes, au web, à une mémoire personnelle et à son vault Obsidian, où
tu écris et retrouves les souvenirs des tâches précédentes. Aucun serveur Jimmy
n'existe : c'est un programme personnel.

## Comment travailler
1. Une demande = un objectif. Decompose-le, puis agis : appelle des outils
   plutôt que de supposer.
2. Utilise un outil dès que la réponse dépend d'une information que tu n'as pas :
   ne devine jamais le contenu d'un fichier, d'une commande ou d'une page.
3. Après chaque appel d'outil, lis réellement le résultat avant de continuer.
   Une erreur se corrige en corrigeant, pas en expliquant.
4. Enchaîne autant d'étapes qu'il le faut. Tu disposes d'un budget d'itérations
   par demande : s'il est presque épuisé, livre le meilleur résultat partiel
   possible et dis clairement ce qui reste à faire.
5. Réponds en français, sauf indication contraire. Va droit au but : ta
   première phrase est la réponse ou l'action faite, jamais un préambule ni une
   reformulation de la question (voir « Sortie »).
6. Quand l'utilisateur valide (« oui », « go », « vas-y », « continue »),
   exécute la tâche **jusqu'au bout dans ce tour** : ne revérifie pas ce que le
   travail récent a déjà confirmé, ne redemande pas confirmation entre les
   étapes. Pour du code : lis ce qui manque, écris les fichiers, lance les
   tests s'il y en a. Ne termine par une question que si une décision de
   l'utilisateur est vraiment nécessaire.
7. Lis efficacement : un gros fichier arrive par morceaux (`read_file` indique
   le `start_line` de la suite) ; `search_files` donne le numéro de ligne de
   ce qu'il trouve, lis autour au lieu de relire tout le fichier.
8. Fichiers sensibles (`.env`, clés, secrets, identifiants), en local comme sur
   un serveur : pour en modifier un à la demande de l'utilisateur, **appelle
   directement l'outil**. C'est l'application qui lui demande son accord (une
   carte s'affiche, l'outil attend sa réponse) : ne demande pas l'accord en
   texte, n'annonce pas que tu ne peux pas. Refusé = abandonné : ne le
   contourne jamais (autre commande, script intermédiaire). Pour les lire,
   masque les valeurs (`sed 's/=.*/=***/'`).

## Mémoire et skills
- Avant d'inventer une préférence de l'utilisateur, cherche-la dans
  `search_memory`. Après avoir appris un fait stable, enregistre-le avec
  `remember`.
- Si la demande reprend un contexte antérieur, `vault_search` peut ramener une
  décision ou une leçon déjà prise : n'impose pas de refaire l'erreur.
- Le vault Obsidian de l'utilisateur est ta mémoire longue : `vault_search`
  pour y chercher, `vault_read` pour relire une note, `vault_write` pour y
  enregistrer un souvenir durable.
- Ce vault est **partagé** : c'est le second cerveau de l'utilisateur, et
  d'autres agents (Claude Code, OpenCode, sa flotte d'agents…) y écrivent
  aussi leurs journaux et leurs notes. Seules les notes de ton dossier (indiqué
  dans le contexte de la session) sont tes souvenirs. Une note marquée
  « partagée » vient de l'utilisateur ou d'un autre agent : ne dis jamais
  « j'ai fait » ou « je me souviens » pour ce qu'elle raconte ; dis d'où vient
  l'information (« d'après une note de ton vault… »).
- Si un même besoin revient, crée un skill avec `create_skill` : c'est mieux
  qu'une longue conversation.
- Un skill ne te donne aucun droit supplémentaire : les permissions de
  l'utilisateur s'appliquent à toutes les actions, y compris celles décrites
  dans un skill.
- Tes skills (procédures nommées) : `list_skills` pour voir leurs noms,
  `read_skill` pour charger celui qu'il faut avant d'agir. N'invente pas une
  procédure de tête quand un skill existe : cherche d'abord.

## Sortie : courte, nette, précise
Ce que tu écris est lu à voix haute, tel quel (voir « Voix ») : chaque phrase
coûte du temps à l'utilisateur. Par défaut, une à trois phrases suffisent.
- Commence par la réponse : l'action faite, ce que tu as trouvé, la décision.
  Pas de préambule, pas de récapitulatif de tes étapes, pas de relance de
  politesse.
- La prose est ce qui est **dit**. Elle dit l'idée, jamais l'implémentation.
- Un détail technique utile (code, chemins, extraits, sortie de commande) va
  dans un bloc de code ou une courte liste : il reste à l'écran et n'est pas lu.
- Tu détailles à l'écrit quand c'est vraiment nécessaire ; le texte affiché peut
  être plus long que ce qui est dit, l'inverse est interdit.

## Voix
Ta réponse est lue à voix haute, en entier, le Markdown et le code retirés.
Écris donc pour l'oral : une seule idée par phrase, pas de symboles, pas de
tableau, pas de parenthèses. Jamais de code, de chemin de fichier, de commande,
d'identifiant technique ni de sortie de programme dans la prose : dis l'idée
(« j'ai modifié le script d'envoi »), pas l'implémentation.
"#;

pub struct PromptContext<'a> {
    pub settings: &'a Settings,
    pub memory: &'a MemoryStore,
    pub skills: &'a SkillStore,
    pub registry: &'a ToolRegistry,
    pub request: &'a str,
    pub memory_block: Option<String>,
    pub vault_block: Option<String>,
    /// Résumé des outils appelés aux tours précédents (`History::tool_digest`).
    pub recent_tools: Option<String>,
    /// Amendements du prompt (lot « croissance ») : des instructions
    /// additionnelles actées avec l'utilisateur, lues dans
    /// `data/growth_amendments.md`. Absent ou vide : prompt inchangé.
    pub amendments: Option<String>,
    /// Tâche de fond en cours (`Tasks::prompt_block`) : Jimmy répond en
    /// parallèle en le sachant. `None` : prompt inchangé.
    pub background_task: Option<String>,
    /// Échange vocal (voir [`VOICE_MODE`]).
    pub voice: bool,
}

/// Consignes ajoutées quand la demande vient du micro.
const VOICE_MODE: &str = r#"## Échange vocal en cours
La demande vient d'être dite à voix haute et transcrite automatiquement.
- Réponds en une à trois phrases courtes : l'utilisateur t'écoute, il ne lit pas.
- La transcription peut être fausse. Si la phrase est incohérente ou ne veut
  rien dire, ne pars pas explorer : demande simplement de répéter, en une
  phrase, en disant ce que tu as compris.
- Pour une question simple ou une conversation, réponds directement, sans
  outil. N'utilise des outils que si la demande l'exige vraiment.
- C'est une conversation : tiens compte des échanges précédents de la session."#;

/// Mode d'emploi du navigateur, ajouté seulement s'il est branché.
const BROWSER_GUIDE: &str = "\n\n## Navigateur\nTon propre Chrome (profil gardé : les comptes où l'utilisateur s'est \
connecté le restent). Après chaque action, appelle `browser_snapshot` pour voir la page : les actions ne la \
renvoient pas. Ne saisis jamais de mot de passe : si un site demande une connexion, demande à l'utilisateur de \
se connecter lui-même dans la fenêtre du navigateur, puis continue. Envoyer, payer, publier ou supprimer \
demande son accord (carte dans le Chat) : décris précisément l'élément cliqué.";

/// Liste des outils pour le prompt système. Les définitions complètes sont
/// déjà envoyées à l'API : ici, seulement un repère. Les outils MCP sont
/// regroupés par serveur (« aggregate : 74 outils ») au lieu d'être recopiés
/// un par un : avec un gros serveur, la liste brute coûtait ~1 500 jetons
/// par appel au modèle, pour rien.
pub fn tools_summary(names: &[String]) -> String {
    let mut local = Vec::new();
    // Serveur → noms courts de ses outils.
    let mut mcp: Vec<(String, Vec<String>)> = Vec::new();
    for name in names {
        match name.strip_prefix("mcp_").and_then(|rest| rest.split_once("__")) {
            Some((server, tool)) => match mcp.iter_mut().find(|(s, _)| s == server) {
                Some((_, tools)) => tools.push(tool.to_string()),
                None => mcp.push((server.to_string(), vec![tool.to_string()])),
            },
            None => local.push(name.as_str()),
        }
    }
    let mut out = format!("## Outils disponibles ({})\n{}", names.len(), local.join(", "));
    if !mcp.is_empty() {
        // Leurs définitions ne sont plus envoyées (trop lourdes) : les noms
        // suffisent pour choisir ; `mcp_list_tools` donne les arguments.
        out.push_str(
            "\n\n## Serveurs MCP\nAppelle leurs outils avec `mcp_call(server, tool, arguments)` ; \
             arguments attendus : `mcp_list_tools(server)`.",
        );
        for (server, tools) in &mcp {
            out.push_str(&format!("\n- {server} ({}) : {}", tools.len(), tools.join(", ")));
        }
        // Navigateur (Playwright) : ses actions ne renvoient pas la page, et
        // l'utilisateur se connecte lui-même à ses comptes.
        if mcp.iter().any(|(_, tools)| tools.iter().any(|t| t == "browser_navigate")) {
            out.push_str(BROWSER_GUIDE);
        }
    }
    out
}

pub fn build_system(ctx: &PromptContext<'_>) -> String {
    let mut parts: Vec<String> = vec![IDENTITY.to_string()];
    if ctx.voice {
        parts.push(VOICE_MODE.to_string());
    }

    let mut session = format!(
        "## Contexte de la session\n- Dossier de travail par défaut : {}\n- Modèle : {}",
        ctx.settings.workspace, ctx.settings.llm.model
    );
    if ctx.settings.memory.vault_enabled {
        session.push_str(&format!(
            "\n- Vault partagé ; ton dossier (tes souvenirs) : {}",
            ctx.settings.memory.vault_folder
        ));
    }
    parts.push(session);

    if let Some(amendments) = &ctx.amendments {
        parts.push(format!(
            "## Amendements (leçons actées)\nCes consignes complètent tes instructions ; chacune vient d'une leçon actée \
             avec l'utilisateur. Pour en ajouter ou en retirer : garde une sauvegarde `.bak` du fichier avant de \
             l'écrire, et fais rejouer le test d'amendement après la modification.\n{amendments}"
        ));
    }

    parts.push(tools_summary(&ctx.registry.names()));

    if let Some(block) = &ctx.recent_tools {
        parts.push(block.clone());
    }

    if let Some(block) = &ctx.background_task {
        parts.push(block.clone());
    }

    // Plus de catalogue injecté : 3 noms suggérés + phrase d'orientation
    // vers list_skills (voir section Mémoire et skills). Coût constant.

    if let Some(block) = &ctx.vault_block {
        if !block.trim().is_empty() {
            parts.push(block.clone());
        }
    }

    if let Some(block) = &ctx.memory_block {
        if !block.trim().is_empty() {
            parts.push(block.clone());
        }
    }

    // Rappel des skills les plus proches de la demande : gain de temps réel,
    // le modèle n'a plus à choisir lesquels il relit.
    let suggestions = ctx.skills.suggest(ctx.request, 3);
    if !suggestions.is_empty() {
        parts.push(format!(
            "Skills probablement utiles pour cette demande : {}",
            suggestions
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    parts.join("\n\n")
}

/// Message utilisateur enrichi des souvenirs, quand le rappel doit être visible
/// de l'agent plutôt que du contexte système.
pub async fn recall_for(memory: &MemoryStore, settings: &Settings, request: &str) -> Option<String> {
    if !settings.memory.enabled {
        return None;
    }
    let block = memory
        .context_block(request, settings.memory.recall_limit as usize)
        .await
        .ok()?;
    (!block.trim().is_empty()).then_some(block)
}

/// Contexte du vault Obsidian à injecter, si la règle de déclenchement le
/// justifie. Remplace Synaptiq : même idée (ramener le contexte antérieur),
/// en local — les notes du vault sont relues en plein texte.
pub async fn vault_context(
    vault: Option<&Arc<crate::memory::vault::Vault>>,
    settings: &Settings,
    request: &str,
) -> Option<String> {
    if !settings.memory.enabled || !settings.memory.vault_enabled {
        return None;
    }
    let vault = vault?;
    // Même heuristique que Synaptiq : une demande courte, sans marqueur de
    // contexte (« comme la dernière fois », « le projet »…) n'a pas besoin du
    // vault ; la mémoire locale courte suffit.
    let markers = crate::memory::vault::has_context_markers(request);
    let decision = crate::memory::vault::should_consult(
        request,
        settings.memory.vault_min_request_chars,
        markers,
    );
    if decision == crate::memory::vault::Usefulness::No {
        log::debug!("[vault] non consulté (demande courte, sans marqueur de contexte)");
        return None;
    }
    let hits = vault.search(request, 4).await;
    if hits.is_empty() {
        return None;
    }
    // Chaque note dit d'où elle vient : le vault est partagé avec d'autres
    // agents, Jimmy ne doit pas s'attribuer leur travail.
    let lignes = hits
        .iter()
        .map(|hit| {
            let origin = if vault.is_own(&hit.path) { "ta note" } else { "partagée" };
            format!("- **{}** ({origin}) — {}", hit.title, hit.snippet)
        })
        .collect::<Vec<_>>()
        .join("\n");
    Some(format!(
        "Notes du vault Obsidian proches de cette demande (relis-en une en entier avec vault_read si nécessaire ; « partagée » = écrite par l'utilisateur ou un autre agent) :\n{lignes}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_liste_des_outils_regroupe_les_serveurs_mcp() {
        let mut names: Vec<String> = vec!["read_file".into(), "run_command".into()];
        names.extend((0..74).map(|i| format!("mcp_aggregate__tool_{i}")));
        names.push("mcp_obsidian__search".into());
        let summary = tools_summary(&names);
        assert!(summary.contains("(77)"), "{summary}");
        assert!(summary.contains("read_file, run_command"), "{summary}");
        // Noms courts par serveur (les définitions ne sont plus envoyées).
        assert!(summary.contains("- aggregate (74) : tool_0, tool_1"), "{summary}");
        assert!(summary.contains("- obsidian (1) : search"), "{summary}");
        assert!(summary.contains("mcp_call") && summary.contains("mcp_list_tools"), "{summary}");
        // Les préfixes ne sont pas répétés : la liste reste légère.
        assert!(!summary.contains("mcp_aggregate__"), "{summary}");
        assert!(summary.chars().count() < 1500, "{} caractères", summary.chars().count());
    }

    /// Le prompt ne porte jamais le catalogue : ni nom ni description d'un
    /// skill existant n'y figure, même avec une demande qui les évoque.
    /// Seuls les 3 noms suggérés et l'orientation vers list_skills y sont.
    #[test]
    fn pas_de_catalogue_dans_le_prompt() {
        let settings = Settings::default();
        let db = std::sync::Arc::new(std::sync::Mutex::new(
            crate::db::Db::open_in_memory().expect("db"),
        ));
        let memory = MemoryStore::new(db);
        let dir = std::env::temp_dir().join(format!("prompt-nocatalogue-{}", uuid::Uuid::new_v4()));
        let skills = crate::skills::SkillStore::new(dir).unwrap();
        skills.write("Atelier Vert", "Peindre des volets en vert.", "1. Poncer. 2. Peindre.").unwrap();
        skills.write("Boulon Jaune", "Serrer des boulons jaunes.", "1. Cle de 13. 2. Serrer.").unwrap();
        let registry = ToolRegistry::new();
        registry.register_defaults(&crate::tools::ToolDeps {
            http: reqwest::Client::new(),
        });
        let system = build_system(&PromptContext {
            settings: &settings,
            memory: &memory,
            skills: &skills,
            registry: &registry,
            request: "bonjour",
            memory_block: None,
            vault_block: None,
            recent_tools: None,
            amendments: None,
            background_task: None,
            voice: false,
        });
        assert!(!system.contains("Atelier"), "{system}");
        assert!(!system.contains("volets"), "{system}");
        assert!(!system.contains("Boulon"), "{system}");
        assert!(system.contains("list_skills"), "{system}");
        assert!(system.contains("read_skill"), "{system}");
    }

    /// Les amendements actés se retrouvent dans le prompt système, avec leur
    /// règle de manipulation (sauvegarde avant écriture).
    #[test]
    fn les_amendements_injectent_une_section() {
        let settings = Settings::default();
        let db = std::sync::Arc::new(std::sync::Mutex::new(
            crate::db::Db::open_in_memory().expect("db"),
        ));
        let memory = MemoryStore::new(db);
        let skills = crate::skills::SkillStore::new(std::env::temp_dir().join("prompt-test-skills")).unwrap();
        let registry = ToolRegistry::new();
        registry.register_defaults(&crate::tools::ToolDeps {
            http: reqwest::Client::new(),
        });
        let system = build_system(&PromptContext {
            settings: &settings,
            memory: &memory,
            skills: &skills,
            registry: &registry,
            request: "bonjour",
            memory_block: None,
            vault_block: None,
            recent_tools: None,
            amendments: Some("Ne jamais reformuler la question avant de répondre.".into()),
            // La tâche de fond rejoint le prompt (réponse en parallèle).
            background_task: Some("## Tâche en cours en arrière-plan
- « écris notify.py »".into()),
            voice: false,
        });
        assert!(system.contains("« écris notify.py »"), "{system}");
        assert!(system.contains("## Amendements"), "{system}");
        assert!(system.contains("Ne jamais reformuler"), "{system}");
        assert!(system.contains(".bak"), "{system}");

        // Sans amendement : pas de section.
        let system = build_system(&PromptContext {
            amendments: None,
            ..common_context(&settings, &memory, &skills, &registry)
        });
        assert!(!system.contains("## Amendements"), "{system}");
    }

    /// Gabarit de contexte réutilisé entre deux variantes d'un même test.
    fn common_context<'a>(
        settings: &'a Settings,
        memory: &'a MemoryStore,
        skills: &'a crate::skills::SkillStore,
        registry: &'a ToolRegistry,
    ) -> PromptContext<'a> {
        PromptContext {
            settings,
            memory,
            skills,
            registry,
            request: "bonjour",
            memory_block: None,
            vault_block: None,
            recent_tools: None,
            amendments: None,
            background_task: None,
            voice: false,
        }
    }
}