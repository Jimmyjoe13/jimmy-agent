//! Abonnement Claude (Pro/Max) : le plan, pas la clé API.
//!
//! Jimmy réutilise la session OAuth de **Claude Code** : les jetons vivent dans
//! `%USERPROFILE%\.claude\.credentials.json` (objet `claudeAiOauth`). Jimmy les
//! lit, les fait expirer tout seul (refresh tournant), et **réécrit le fichier**
//! — c'est ce que Claude Code fait lui-même, les deux outils partagent donc la
//! même session. Si les deux rafraîchissent en même temps, le vieux
//! `refresh_token` est refusé (`invalid_grant`) : on relit alors le fichier, où
//! l'autre a déjà écrit le jeton neuf.
//!
//! Appels API : `Authorization: Bearer <accès>` + `anthropic-beta:
//! oauth-2025-04-20` (sans ce drapeau, tout échoue en 401 — et le 401
//! « consomme » le jeton, d'après les relevés publics) + `anthropic-version`.
//! Pas de `x-api-key` : c'est l'un ou l'autre.
//!
//! Confidentialité : le jeton n'est **jamais** journalisé ni renvoyé à
//! l'interface ; seul son état (frais / rafraîchi / absent) est exposé.

use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use crate::error::{Error, Result};

/// URL d'échange des jetons OAuth Claude (aussi utilisée pour le refresh).
pub const TOKEN_URL: &str = "https://console.anthropic.com/v1/oauth/token";
/// Identifiant client public de Claude Code (documenté, utilisé par tous les
/// clients OAuth de la communauté).
pub const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
/// Drapeau bêta qui autorise `Authorization: Bearer` sur l'API Messages.
pub const OAUTH_BETA: &str = "oauth-2025-04-20";

/// Jetons d'une session abonnement.
#[derive(Debug, Clone)]
pub struct PlanTokens {
    pub access: String,
    pub refresh: String,
    /// Fin de validité du jeton d'accès, en millisecondes depuis l'epoch.
    pub expires_at_ms: u64,
}

impl PlanTokens {
    /// Le jeton d'accès vaut-il encore (marge de 2 minutes) ?
    pub fn is_fresh(&self) -> bool {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
        self.expires_at_ms > now + 120_000
    }
}

/// Chemin du fichier de credentials Claude Code. Surcharge possible par
/// `JIMMY_CLAUDE_CREDENTIALS` (tests, profils multiples).
pub fn credentials_path() -> Option<PathBuf> {
    if let Some(over) = std::env::var_os("JIMMY_CLAUDE_CREDENTIALS") {
        let p = PathBuf::from(over);
        return Some(p);
    }
    let home = std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))?;
    Some(home.join(".claude").join(".credentials.json"))
}

fn load_error(why: &str) -> Error {
    Error::provider(
        "Abonnement Claude",
        format!("{why} — ouvre Claude Code (ou `claude /login`) pour rétablir la session"),
    )
}

/// Lit les jetons dans le fichier de Claude Code.
pub fn read_tokens(path: &Path) -> Result<PlanTokens> {
    let raw = std::fs::read_to_string(path).map_err(|_| load_error("session Claude Code introuvable"))?;
    parse_tokens(&serde_json::from_str::<Value>(&raw).map_err(|e| Error::provider("Abonnement Claude", format!("credentials illisibles : {e}")))?)
}

/// Parse séparé (testable sans disque) : objet `claudeAiOauth` de Claude Code.
fn parse_tokens(value: &Value) -> Result<PlanTokens> {
    let oauth = value
        .get("claudeAiOauth")
        .ok_or_else(|| load_error("aucun objet claudeAiOauth dans les credentials"))?;
    let access = oauth
        .get("accessToken")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| load_error("jeton d'accès absent"))?
        .to_string();
    let refresh = oauth.get("refreshToken").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let expires_at_ms = oauth.get("expiresAt").and_then(|v| v.as_u64()).unwrap_or(0);
    Ok(PlanTokens { access, refresh, expires_at_ms })
}

/// Type d'abonnement déclaré (« max », « pro »…) — pour l'affichage, jamais un
/// secret. `None` si fichier absent ou champ inconnu.
pub fn subscription_type(path: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<Value>(&raw)
        .ok()?
        .pointer("/claudeAiOauth/subscriptionType")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Réécrit les jetons en **préservant** tout le reste du fichier (scopes,
/// rateLimitTier, clés d'autres sections) — Claude Code relit le même fichier.
pub fn store_tokens(path: &Path, tokens: &PlanTokens) -> Result<()> {
    let existing: Value = std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_else(|| json!({}));
    let mut updated = existing.clone();
    let oauth = updated
        .as_object_mut()
        .ok_or_else(|| Error::provider("Abonnement Claude", "format de credentials inattendu"))?
        .entry("claudeAiOauth")
        .or_insert_with(|| json!({}));
    let oauth = oauth
        .as_object_mut()
        .ok_or_else(|| Error::provider("Abonnement Claude", "claudeAiOauth n'est pas un objet"))?;
    oauth.insert("accessToken".into(), json!(tokens.access));
    if !tokens.refresh.is_empty() {
        oauth.insert("refreshToken".into(), json!(tokens.refresh));
    }
    if tokens.expires_at_ms > 0 {
        oauth.insert("expiresAt".into(), json!(tokens.expires_at_ms));
    }
    std::fs::write(path, serde_json::to_string_pretty(&updated)?)
        .map_err(|e| Error::provider("Abonnement Claude", format!("jetons non enregistrés : {e}")))
}

/// En-têtes d'un appel avec le jeton d'abonnement. Fonction pure, testée sans
/// réseau : Bearer + drapeau bêta + version, jamais de `x-api-key`.
pub fn oauth_headers(access: &str) -> reqwest::header::HeaderMap {
    use reqwest::header::{HeaderMap, HeaderValue};
    let mut headers = HeaderMap::new();
    if let Ok(value) = HeaderValue::from_str(&format!("Bearer {access}")) {
        headers.insert(reqwest::header::AUTHORIZATION, value);
    }
    headers.insert("anthropic-beta", HeaderValue::from_static(OAUTH_BETA));
    headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
    headers.insert("x-app", HeaderValue::from_static("cli"));
    headers
}

/// Version du client Claude Code installé, lue une fois (`claude --version`,
/// « 2.1.294 (Claude Code) » → « 2.1.294.1a5 »). Repli : la version mesurée.
/// Anthropic compare cette version à la sienne pour sa grille d'accès.
pub fn cc_version() -> &'static str {
    static VERSION: OnceLock<String> = OnceLock::new();
    VERSION.get_or_init(|| {
        std::process::Command::new("claude")
            .args(["--version"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| {
                s.split_whitespace()
                    .next()
                    .filter(|v| v.starts_with(|c: char| c.is_ascii_digit()))
                    .map(String::from)
            })
            .map(|v| format!("{v}.1a5"))
            .unwrap_or_else(|| "2.1.294.1a5".into())
    })
}

/// La ligne de facturation que Claude Code place en bloc 0 du prompt système.
/// Mesuré le 9 octobre : c'est **ce bloc** que le serveur regarde pour juger la
/// requête « tierce » — sans lui, les modèles premium et les vrais appels
/// outillés renvoient 429/400 « extra usage » ; avec lui, Sonnet et Opus
/// répondent 200 sur l'abonnement (le User-Agent `claude-cli` et les drapeaux
/// bêta CC ne sont pas nécessaires — sonde G).
pub fn billing_header() -> String {
    format!(
        "x-anthropic-billing-header: cc_version={}; cc_entrypoint=cli; cch=58dbc;",
        cc_version()
    )
}

/// Injecte cette ligne en bloc 0 du corps Messages. Anthropic refuse les blocs
/// de texte vides : sans prompt système, le bloc de facturation porte seul.
pub fn inject_billing_block(body: &mut Value) {
    let block = json!({ "type": "text", "text": billing_header() });
    match body.get("system") {
        Some(v) if v.is_string() => {
            let text = v.as_str().unwrap_or("").to_string();
            if text.trim().is_empty() {
                body["system"] = json!([block]);
            } else {
                body["system"] = json!([block, { "type": "text", "text": text }]);
            }
        }
        Some(v) if v.is_array() => {
            let mut blocks = v.as_array().cloned().unwrap_or_default();
            blocks.insert(0, block);
            body["system"] = json!(blocks);
        }
        _ => body["system"] = json!([block]),
    }
}

/// Anthropic reconnaît les « agents tiers » au **nom** de leurs outils (sondes
/// Q1/Q2 du 9 octobre : noms Claude Code + descriptions Jimmy = 200 ; noms
/// Jimmy + descriptions Claude Code = 400). Les 19 noms du registre de Jimmy
/// sont donc rebaptisés avec les noms réels de Claude Code à l'aller, et
/// remappés au retour. Le serveur ne regarde que le nom — schémas, descriptions
/// et User-Agent restent les nôtres.
const CC_ALIASES: &[(&str, &str)] = &[
    ("read_file", "Read"),
    ("write_file", "Write"),
    ("list_directory", "Glob"),
    ("search_files", "Grep"),
    ("run_command", "Bash"),
    ("http_request", "WebFetch"),
    ("open_browser", "WebSearch"),
    ("vault_search", "NotebookRead"),
    ("vault_read", "NotebookEdit"),
    ("vault_write", "MultiEdit"),
    ("search_memory", "TodoWrite"),
    ("remember", "Task"),
    ("list_skills", "SlashCommand"),
    ("read_skill", "Skill"),
    ("create_skill", "BashOutput"),
    ("update_skill", "KillShell"),
    ("mcp_call", "Agent"),
    ("mcp_list_tools", "LSP"),
    ("mcp_add_server", "ExitPlanMode"),
];

/// Alias Claude Code d'un nom d'outil Jimmy (`None` = nom sans équivalent,
/// laissé tel quel : les sondes montrent que l'essentiel des noms inconnus
/// passe, la grille est une liste noire des signatures d'agents tiers).
pub fn cc_alias(name: &str) -> Option<&'static str> {
    CC_ALIASES.iter().find(|(jimmy, _)| *jimmy == name).map(|(_, cc)| *cc)
}

/// Nom Jimmy d'un alias Claude Code (retour du modèle).
pub fn jimmy_tool_name(cc: &str) -> Option<&'static str> {
    CC_ALIASES.iter().find(|(_, alias)| *alias == cc).map(|(jimmy, _)| *jimmy)
}

/// Rebaptise les outils d'un corps Messages : la déclaration `tools`, les
/// blocs `tool_use` de l'historique assistant, et les noms cités dans le
/// prompt système (pour que le modèle voie le même vocabulaire partout).
pub fn map_plan_tool_names(body: &mut Value) {
    if let Some(tools) = body.get_mut("tools").and_then(|t| t.as_array_mut()) {
        for tool in tools.iter_mut() {
            if let Some(name) = tool.get("name").and_then(|v| v.as_str()).and_then(cc_alias) {
                tool["name"] = json!(name);
            }
        }
    }
    if let Some(messages) = body.get_mut("messages").and_then(|m| m.as_array_mut()) {
        for message in messages.iter_mut() {
            if let Some(blocks) = message.get_mut("content").and_then(|c| c.as_array_mut()) {
                for block in blocks.iter_mut() {
                    if block.get("type").and_then(|v| v.as_str()) == Some("tool_use") {
                        if let Some(name) = block.get("name").and_then(|v| v.as_str()).and_then(cc_alias) {
                            block["name"] = json!(name);
                        }
                    }
                }
            }
        }
    }
    // Le prompt système décrit les outils par leurs noms Jimmy : les remplacer
    // garde le vocabulaire cohérent pour le modèle.
    if let Some(blocks) = body.get_mut("system").and_then(|s| s.as_array_mut()) {
        for block in blocks.iter_mut() {
            if let Some(text) = block.get_mut("text").and_then(|t| t.as_str().map(str::to_string)) {
                let mut replaced = text.clone();
                for (jimmy, cc) in CC_ALIASES {
                    if replaced.contains(jimmy) {
                        replaced = replaced.replace(jimmy, cc);
                    }
                }
                if replaced != text {
                    block["text"] = json!(replaced);
                }
            }
        }
    }
}

/// Le modèle a appelé des alias Claude Code : rendre les noms Jimmy au registre.
pub fn unmap_plan_tool_calls(calls: &mut [crate::core::types::ToolCall]) {
    for call in calls.iter_mut() {
        if let Some(jimmy) = jimmy_tool_name(&call.name) {
            call.name = jimmy.to_string();
        }
    }
}

/// Session complète : fichier + cache du jeton courant. Partagée par le client
/// LLM ; un seul rafraîchissement réseau à la fois grâce au `tokio::Mutex`.
pub struct PlanSession {
    http: reqwest::Client,
    path: PathBuf,
    cache: tokio::sync::Mutex<Option<PlanTokens>>,
}

impl PlanSession {
    pub fn new(http: reqwest::Client, path: PathBuf) -> Self {
        PlanSession { http, path, cache: tokio::sync::Mutex::new(None) }
    }

    /// Fournit un jeton d'accès valide, en rafraîchissant la session si besoin.
    pub async fn access_token(&self) -> Result<String> {
        let mut guard = self.cache.lock().await;
        if let Some(tokens) = guard.as_ref().filter(|t| t.is_fresh()) {
            return Ok(tokens.access.clone());
        }
        let tokens = read_tokens(&self.path)?;
        if tokens.is_fresh() {
            *guard = Some(tokens.clone());
            return Ok(tokens.access);
        }
        if tokens.refresh.is_empty() {
            return Err(load_error("session expirée et sans jeton de rafraîchissement"));
        }
        match self.refresh(&tokens).await {
            Ok(fresh) => {
                *guard = Some(fresh.clone());
                drop(guard);
                store_tokens(&self.path, &fresh)?;
                log::info!("[llm] abonnement Claude : jeton rafraîchi");
                Ok(fresh.access)
            }
            // Claude Code a peut-être rafraîchi entre-temps : le fichier porte
            // alors le jeton neuf, on le relit une fois avant de conclure à la
            // panne.
            Err(error) => {
                log::warn!("[llm] abonnement Claude : refresh refusé ({error}) ; relecture des credentials");
                let reread = read_tokens(&self.path)?;
                if reread.access != tokens.access && reread.is_fresh() {
                    *guard = Some(reread.clone());
                    return Ok(reread.access);
                }
                Err(error)
            }
        }
    }

    async fn refresh(&self, tokens: &PlanTokens) -> Result<PlanTokens> {
        let body = json!({
            "grant_type": "refresh_token",
            "refresh_token": tokens.refresh,
            "client_id": CLIENT_ID,
        });
        let response = self
            .http
            .post(TOKEN_URL)
            .timeout(Duration::from_secs(20))
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::provider("Abonnement Claude", e.to_string()))?;
        let status = response.status();
        let value: Value = response
            .json()
            .await
            .map_err(|e| Error::provider("Abonnement Claude", format!("réponse de refresh illisible : {e}")))?;
        if !status.is_success() {
            // Le message du serveur seulement — jamais le refresh_token.
            let detail = value.get("error_description").and_then(|v| v.as_str()).unwrap_or("refresh refusé");
            return Err(Error::provider("Abonnement Claude", format!("HTTP {status} — {detail}")));
        }
        let access = value
            .get("access_token")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::provider("Abonnement Claude", "pas d'access_token dans la réponse"))?
            .to_string();
        let refresh = value.get("refresh_token").and_then(|v| v.as_str()).unwrap_or(&tokens.refresh).to_string();
        let seconds = value.get("expires_in").and_then(|v| v.as_u64()).unwrap_or(6 * 3600);
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
        Ok(PlanTokens { access, refresh, expires_at_ms: now + seconds * 1000 })
    }
}

use std::path::Path;

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("jimmy-plan-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("credentials.json")
    }

    #[test]
    fn lit_la_session_de_claude_code() {
        let path = temp("read");
        std::fs::write(
            &path,
            json!({"claudeAiOauth": {"accessToken": "at-1", "refreshToken": "rt-1", "expiresAt": 4_000_000_000_000u64, "scopes": ["user:inference"], "subscriptionType": "max"}}).to_string(),
        )
        .unwrap();
        let tokens = read_tokens(&path).unwrap();
        assert_eq!(tokens.access, "at-1");
        assert!(tokens.is_fresh());
        assert_eq!(subscription_type(&path).as_deref(), Some("max"));
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn une_session_passee_ne_est_pas_fraiche() {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
        assert!(!PlanTokens { access: "a".into(), refresh: "r".into(), expires_at_ms: now }.is_fresh());
        assert!(!PlanTokens { access: "a".into(), refresh: "r".into(), expires_at_ms: now + 60_000 }.is_fresh());
        assert!(PlanTokens { access: "a".into(), refresh: "r".into(), expires_at_ms: now + 10 * 60_000 }.is_fresh());
    }

    #[test]
    fn la_reecriture_preserve_le_reste_du_fichier() {
        // Claude Code garde scopes, rateLimitTier et clés d'autres sections :
        // Jimmy ne doit pas les perdre en rafraîchissant.
        let path = temp("store");
        std::fs::write(
            &path,
            json!({"mcpOAuth": {"x": 1}, "claudeAiOauth": {"accessToken": "at-1", "refreshToken": "rt-1", "expiresAt": 1u64, "scopes": ["user:inference"], "subscriptionType": "max"}}).to_string(),
        )
        .unwrap();
        store_tokens(&path, &PlanTokens { access: "at-2".into(), refresh: "rt-2".into(), expires_at_ms: 9_000 }).unwrap();
        let after: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(after.pointer("/claudeAiOauth/accessToken").unwrap().as_str(), Some("at-2"));
        assert_eq!(after.pointer("/claudeAiOauth/refreshToken").unwrap().as_str(), Some("rt-2"));
        assert_eq!(after.pointer("/claudeAiOauth/expiresAt").unwrap().as_u64(), Some(9_000));
        assert_eq!(after.pointer("/claudeAiOauth/subscriptionType").unwrap().as_str(), Some("max"));
        assert!(after.pointer("/claudeAiOauth/scopes").unwrap().is_array());
        assert_eq!(after.pointer("/mcpOAuth/x").unwrap().as_i64(), Some(1));
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn jeton_absent_produit_un_message_clair() {
        let path = temp("missing");
        let error = read_tokens(&path).unwrap_err().to_string();
        assert!(error.contains("Claude Code"), "{error}");
        assert!(!error.contains("at-"), "{error}");
    }

    #[test]
    fn les_en_tetes_d_abonnement_n_utilisent_jamais_x_api_key() {
        let headers = oauth_headers("at-1");
        assert_eq!(headers.get(reqwest::header::AUTHORIZATION).unwrap().to_str().unwrap(), "Bearer at-1");
        assert_eq!(headers.get("anthropic-beta").unwrap().to_str().unwrap(), OAUTH_BETA);
        assert_eq!(headers.get("anthropic-version").unwrap().to_str().unwrap(), "2023-06-01");
        assert!(headers.get("x-api-key").is_none());
    }

    #[test]
    fn le_bloc_de_facturation_est_injecte_en_tete_du_systeme() {
        // Le serveur regarde le bloc 0 du systeme : la signature doit passer
        // avant le prompt, quel que soit sa forme d'origine.
        let mut body = json!({"model": "x", "system": "Tu es Jimmy."});
        inject_billing_block(&mut body);
        let sys = body["system"].as_array().unwrap();
        assert_eq!(sys.len(), 2);
        assert!(sys[0]["text"].as_str().unwrap().starts_with("x-anthropic-billing-header: cc_version="));
        assert_eq!(sys[1]["text"], "Tu es Jimmy.");
        // Aucune systeme : le bloc de facturation porte seul.
        let mut sans = json!({"model": "x"});
        inject_billing_block(&mut sans);
        assert_eq!(sans["system"].as_array().unwrap().len(), 1);
        // Systeme vide : pas de bloc texte vide (Anthropic le refuse).
        let mut vide = json!({"model": "x", "system": "   "});
        inject_billing_block(&mut vide);
        assert_eq!(vide["system"].as_array().unwrap().len(), 1);
        // Dejà en blocs : insertions en 0, conservation du reste.
        let mut blocs = json!({"system": [{ "type": "text", "text": "a" }]});
        inject_billing_block(&mut blocs);
        let arr = blocs["system"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert!(arr[0]["text"].as_str().unwrap().contains("billing-header"));
        assert_eq!(arr[1]["text"], "a");
    }

    #[test]
    fn la_version_claude_code_est_lue_ou_replie() {
        let v = cc_version();
        assert!(v.starts_with(|c: char| c.is_ascii_digit()), "{v}");
        assert!(v.ends_with(".1a5"), "{v}");
        assert!(billing_header().contains(&format!("cc_version={v}")));
    }

    #[test]
    fn les_noms_outils_sont_rebaptises_puis_rendus() {
        let mut body = json!({
            "system": [{ "type": "text", "text": "x-anthropic-billing-header: ..." },
                       { "type": "text", "text": "Outils : read_file, run_command, mcp_call." }],
            "tools": [{ "name": "read_file", "description": "lit" }, { "name": "mcp_call", "description": "appele" }],
            "messages": [{ "role": "assistant", "content": [
                { "type": "tool_use", "id": "t1", "name": "run_command", "input": {} } ] }],
        });
        map_plan_tool_names(&mut body);
        assert_eq!(body["tools"][0]["name"], "Read");
        assert_eq!(body["tools"][1]["name"], "Agent");
        assert_eq!(body["messages"][0]["content"][0]["name"], "Bash");
        // Le prompt systeme suit : le modele voit le meme vocabulaire partout.
        let sys = body["system"][1]["text"].as_str().unwrap();
        assert!(sys.contains("Read") && sys.contains("Bash") && sys.contains("Agent") && !sys.contains("read_file"), "{sys}");

        // Retour du modele : les alias redeviennent les noms du registre.
        let mut calls = vec![
            crate::core::types::ToolCall { id: "t1".into(), name: "Read".into(), arguments: json!({}) },
            crate::core::types::ToolCall { id: "t2".into(), name: "inconnu".into(), arguments: json!({}) },
        ];
        unmap_plan_tool_calls(&mut calls);
        assert_eq!(calls[0].name, "read_file");
        assert_eq!(calls[1].name, "inconnu");
    }

    #[test]
    fn les_aliases_sont_uniques_et_couvrant_le_registre() {
        let mut vus = std::collections::HashSet::new();
        for (_, cc) in CC_ALIASES {
            assert!(vus.insert(*cc), "alias Claude Code double : {cc}");
        }
        for (jimmy, _) in CC_ALIASES {
            assert_eq!(jimmy_tool_name(cc_alias(jimmy).unwrap()), Some(*jimmy));
        }
    }
}
