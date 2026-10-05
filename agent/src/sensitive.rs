//! Fichiers sensibles : rien n'y est modifié sans l'accord explicite de
//! l'utilisateur.
//!
//! Cas réel du 5 octobre 2026 : dans une conversation sur JobXpress, Jimmy a
//! sauvegardé puis réécrit `/home/jimmy/jobxpress/secure/.env.api` sur le VPS
//! (`vps_exec` : `cp …; python3 <<'PY' …`) et relancé le conteneur, sans rien
//! demander. Les permissions de la V1 (accordées une fois pour toutes) ne
//! distinguent pas un `.env` d'un fichier de code.
//!
//! Ce module repère, avant l'exécution d'un outil, une **modification** d'un
//! fichier sensible (`.env`, clés, secrets…) — écriture locale, commande
//! PowerShell, commande distante MCP (`vps_exec`), envoi de fichier. La
//! **lecture** reste libre. La boucle d'agent suspend alors l'outil et demande
//! l'accord dans le Chat ([`authorize`]) ; refus, silence ou demande vocale =
//! l'outil n'est pas exécuté.
//!
//! C'est une détection par motifs, pas un bac à sable : un script
//! intermédiaire qui ne nomme pas le fichier y échappe. Le prompt interdit ce
//! contournement ; le garde-fou couvre les cas réels constatés.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};

use crate::core::types::AgentEvent;

/// Délai laissé à l'utilisateur pour répondre ; au-delà, c'est un refus.
pub const APPROVAL_TIMEOUT: Duration = Duration::from_secs(300);

// ── Détection ────────────────────────────────────────────────────────────────

/// Le chemin désigne-t-il un fichier sensible (secrets, clés, identifiants) ?
pub fn is_sensitive_path(path: &str) -> bool {
    let p = path
        .trim()
        .trim_matches(|c| c == '"' || c == '\'' || c == '`')
        .to_lowercase()
        .replace('\\', "/");
    if p.is_empty() {
        return false;
    }
    // Dossiers qui ne contiennent que des secrets.
    let in_dir = |dir: &str| p.contains(&format!("/{dir}/")) || p.starts_with(&format!("{dir}/"));
    if ["secure", "secrets", ".ssh", ".gnupg", ".aws"].iter().any(|d| in_dir(d)) {
        return true;
    }
    let name = p.rsplit('/').next().unwrap_or(&p);
    // Modèles commités sans valeur réelle : pas sensibles.
    if [".example", ".sample", ".template", ".dist"].iter().any(|s| name.ends_with(s)) {
        return false;
    }
    name == ".env"
        || name.starts_with(".env.")
        || name.ends_with(".env")
        || [".pem", ".key", ".p12", ".pfx", ".jks", ".keystore", ".kdbx"]
            .iter()
            .any(|ext| name.ends_with(ext))
        || ["id_rsa", "id_ed25519", "id_ecdsa", "id_dsa"].iter().any(|k| name.starts_with(k))
        || [
            "authorized_keys",
            ".netrc",
            ".pgpass",
            ".htpasswd",
            ".npmrc",
            ".pypirc",
            ".git-credentials",
            "shadow",
        ]
        .contains(&name)
        || name.contains("secret")
        || name.contains("credential")
}

/// Premier fichier sensible nommé dans un texte (commande, script, JSON).
fn find_sensitive(text: &str) -> Option<String> {
    text.split(|c: char| c.is_whitespace() || ";|&<>()'\"`=,{}[]".contains(c))
        .find(|token| is_sensitive_path(token))
        .map(|token| token.to_string())
}

/// Commandes qui ne font que lire (premier mot d'un segment).
const READ_ONLY: &[&str] = &[
    "cat", "less", "more", "head", "tail", "grep", "egrep", "fgrep", "rg", "ls", "ll", "dir", "stat",
    "file", "wc", "readlink", "realpath", "test", "[", "[[", "echo", "printf", "diff", "cmp", "md5sum",
    "sha1sum", "sha256sum", "sort", "uniq", "cut", "tr", "awk", "sed", "find", "cd", "pwd", "which",
    "type", "jq", "column", "xxd", "hexdump", "base64", "true", "false", "exit", "get-content", "gc",
    "select-string", "get-childitem", "gci", "test-path", "get-item", "gi", "resolve-path",
    "measure-object", "select-object", "where-object", "format-list", "format-table", "out-string",
    "out-null", "write-output", "write-host", "sls",
];

/// Une ligne de commande (PowerShell ou shell distant) qui touche un fichier
/// sensible sans être une pure lecture. Renvoie le fichier en cause.
pub fn command_modifies_sensitive(command: &str) -> Option<String> {
    // `docker compose --env-file X` lit X, il ne le modifie pas.
    let mut cleaned = String::new();
    let mut skip_next = false;
    for token in command.split(' ') {
        if skip_next {
            skip_next = false;
            continue;
        }
        if token == "--env-file" {
            skip_next = true;
            continue;
        }
        if token.starts_with("--env-file=") {
            continue;
        }
        cleaned.push_str(token);
        cleaned.push(' ');
    }
    let target = find_sensitive(&cleaned)?;
    if is_read_only(&cleaned) {
        None
    } else {
        Some(target)
    }
}

/// Le texte entre guillemets remplacé par des `_` : un `|` ou un `>` dans un
/// motif (`grep -E 'A|B'`) n'est pas un tube ni une redirection.
fn mask_quoted(command: &str) -> String {
    let mut quote: Option<char> = None;
    command
        .chars()
        .map(|c| match quote {
            Some(q) if c == q => {
                quote = None;
                c
            }
            Some(_) => '_',
            None => {
                if c == '\'' || c == '"' {
                    quote = Some(c);
                }
                c
            }
        })
        .collect()
}

/// La commande ne fait-elle que lire ? Au moindre doute : non.
fn is_read_only(command: &str) -> bool {
    let lower = mask_quoted(&command.to_lowercase());
    // Heredoc : un script complet (python3 <<'PY' …) peut tout écrire.
    if lower.contains("<<") {
        return false;
    }
    // Redirections vers un fichier. Celles vers la sortie standard ou le néant
    // sont retirées d'abord.
    let mut probe = lower.clone();
    for harmless in ["2>&1", "1>&2", ">&2", ">&1", "&>/dev/null", "2>/dev/null", ">/dev/null", "> /dev/null", "2>$null", ">$null"] {
        probe = probe.replace(harmless, " ");
    }
    if probe.contains('>') {
        return false;
    }
    probe
        .split(|c| c == ';' || c == '|' || c == '&' || c == '\n')
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .all(|segment| {
            let mut words = segment
                .split_whitespace()
                // Préfixes sans effet propre : sudo, variables d'environnement.
                .skip_while(|w| *w == "sudo" || (w.contains('=') && !w.starts_with('-')));
            let Some(verb) = words.next() else { return true };
            let verb = verb.rsplit('/').next().unwrap_or(verb);
            let rest: Vec<&str> = words.collect();
            match verb {
                // `sed -i`, `awk -i inplace` : édition en place.
                "sed" | "awk" => !rest.iter().any(|w| *w == "-i" || w.starts_with("-i") || w.starts_with("--in-place") || *w == "inplace"),
                // `find -delete` / `-exec` : effet de bord.
                "find" => !rest.iter().any(|w| *w == "-delete" || *w == "-exec" || *w == "-execdir"),
                verb => READ_ONLY.contains(&verb),
            }
        })
}

/// Noms d'outils MCP qui écrivent (envoi, création, mise à jour, suppression).
fn is_write_tool(tool: &str) -> bool {
    let tool = tool.to_lowercase();
    ["write", "upload", "put", "patch", "create", "update", "delete", "edit", "append", "save", "remove", "move", "rename", "copy"]
        .iter()
        .any(|w| tool.contains(w))
}

/// Toutes les chaînes d'une valeur JSON (arguments imbriqués).
fn strings(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::String(s) => out.push(s.clone()),
        serde_json::Value::Array(items) => items.iter().for_each(|v| strings(v, out)),
        serde_json::Value::Object(map) => map.values().for_each(|v| strings(v, out)),
        _ => {}
    }
}

/// Arguments d'un outil MCP : objet, ou chaîne JSON (certains modèles
/// sérialisent les arguments imbriqués).
fn inner_args(value: Option<&serde_json::Value>) -> serde_json::Value {
    match value {
        Some(serde_json::Value::String(raw)) => serde_json::from_str(raw).unwrap_or(serde_json::Value::Null),
        Some(other) => other.clone(),
        None => serde_json::Value::Null,
    }
}

/// Un appel d'outil MCP (`tool` sur le serveur) modifie-t-il un fichier
/// sensible ?
fn mcp_modifies_sensitive(tool: &str, args: &serde_json::Value) -> Option<String> {
    // Outil qui exécute une commande (vps_exec, local_run, server_run…).
    for key in ["command", "cmd", "script"] {
        if let Some(command) = args.get(key).and_then(|v| v.as_str()) {
            return command_modifies_sensitive(command);
        }
    }
    if is_write_tool(tool) {
        let mut all = Vec::new();
        strings(args, &mut all);
        return all.iter().find_map(|s| find_sensitive(s));
    }
    None
}

/// L'appel d'outil modifie-t-il un fichier sensible ? Renvoie le fichier en
/// cause ; `None` pour tout le reste (lecture comprise).
pub fn needs_approval(tool: &str, args: &serde_json::Value) -> Option<String> {
    let arg = |key: &str| args.get(key).and_then(|v| v.as_str()).unwrap_or("");
    match tool {
        "write_file" => Some(arg("path")).filter(|p| is_sensitive_path(p)).map(str::to_string),
        "run_command" => command_modifies_sensitive(arg("command")),
        "mcp_call" => mcp_modifies_sensitive(arg("tool"), &inner_args(args.get("arguments"))),
        // Outil MCP enregistré directement (`mcp_<serveur>__<outil>`).
        name if name.starts_with("mcp_") => {
            let short = name.split("__").nth(1).unwrap_or(name);
            mcp_modifies_sensitive(short, args)
        }
        _ => None,
    }
}

/// Ce que la carte d'autorisation montre : l'outil et la commande exacte.
pub fn describe(tool: &str, args: &serde_json::Value) -> String {
    let (label, inner) = if tool == "mcp_call" {
        let server = args.get("server").and_then(|v| v.as_str()).unwrap_or("?");
        let name = args.get("tool").and_then(|v| v.as_str()).unwrap_or("?");
        (format!("{server} · {name}"), inner_args(args.get("arguments")))
    } else {
        (tool.to_string(), args.clone())
    };
    let body = ["command", "cmd", "script"]
        .iter()
        .find_map(|k| inner.get(*k).and_then(|v| v.as_str()).map(str::to_string))
        .unwrap_or_else(|| serde_json::to_string_pretty(&inner).unwrap_or_default());
    let body: String = body.chars().take(2000).collect();
    format!("{label}\n{body}")
}

// ── Demande d'autorisation ───────────────────────────────────────────────────

/// Demandes en attente d'une réponse de l'utilisateur (une par appel d'outil
/// suspendu). Partagé entre la boucle d'agent et la commande Tauri qui
/// transmet le clic « Autoriser / Refuser ».
#[derive(Default)]
pub struct Approvals {
    next: AtomicU64,
    pending: Mutex<HashMap<String, oneshot::Sender<bool>>>,
}

impl Approvals {
    /// Ouvre une demande : son identifiant et l'attente de la réponse.
    pub fn request(&self) -> (String, oneshot::Receiver<bool>) {
        let id = format!("approval-{}", self.next.fetch_add(1, Ordering::Relaxed) + 1);
        let (tx, rx) = oneshot::channel();
        if let Ok(mut pending) = self.pending.lock() {
            pending.insert(id.clone(), tx);
        }
        (id, rx)
    }

    /// Réponse de l'utilisateur. `false` si la demande n'existe plus
    /// (expirée, tâche arrêtée).
    pub fn respond(&self, id: &str, approved: bool) -> bool {
        let sender = self.pending.lock().ok().and_then(|mut p| p.remove(id));
        sender.is_some_and(|tx| tx.send(approved).is_ok())
    }

    fn forget(&self, id: &str) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.remove(id);
        }
    }
}

/// Garde-fou avant l'exécution d'un outil. `None` : l'outil peut s'exécuter
/// (rien de sensible, ou accord donné). `Some(texte)` : refusé, le texte est
/// le résultat d'outil renvoyé au modèle.
///
/// À la voix, personne ne peut cliquer : refus immédiat, l'utilisateur
/// refera la demande dans le Chat.
pub async fn authorize(
    approvals: &Approvals,
    voice: bool,
    tool: &str,
    args: &serde_json::Value,
    events: &mpsc::Sender<AgentEvent>,
    timeout: Duration,
) -> Option<String> {
    let target = needs_approval(tool, args)?;
    if voice {
        log::warn!("[sécurité] modification de « {target} » refusée : demande vocale, accord impossible");
        return Some(format!(
            "REFUSÉ : modifier « {target} » (fichier sensible) exige l'accord explicite de l'utilisateur, \
             impossible à donner à la voix. Ne tente pas de le modifier autrement ; dis-lui de refaire \
             la demande dans le Chat écrit."
        ));
    }
    let (id, answer) = approvals.request();
    log::info!("[sécurité] autorisation demandée ({id}) pour modifier « {target} »");
    let _ = events
        .send(AgentEvent::Approval {
            id: id.clone(),
            target: target.clone(),
            detail: describe(tool, args),
        })
        .await;
    let approved = matches!(tokio::time::timeout(timeout, answer).await, Ok(Ok(true)));
    approvals.forget(&id);
    let _ = events.send(AgentEvent::ApprovalResolved { id: id.clone(), approved }).await;
    log::info!("[sécurité] {id} : {}", if approved { "autorisé" } else { "refusé" });
    if approved {
        None
    } else {
        Some(format!(
            "REFUSÉ par l'utilisateur (ou sans réponse) : la modification de « {target} » (fichier sensible) \
             n'a PAS été faite. Ne tente pas de la faire autrement (autre commande, script intermédiaire) ; \
             explique ce que tu voulais changer et pourquoi, et laisse-le décider."
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// La commande réelle du 5 octobre (21:00) : sauvegarde puis réécriture
    /// d'un `.env` de production par un script Python.
    #[test]
    fn la_reecriture_reelle_du_env_est_reperee() {
        let command = "cd /home/jimmy/jobxpress/repo && git pull --ff-only 2>&1 | tail -3; \
            cp /home/jimmy/jobxpress/secure/.env.api /home/jimmy/jobxpress/secure/.env.api.bak-20261005; \
            python3 <<'PY'\nopen('/home/jimmy/jobxpress/secure/.env.api','w').write(x)\nPY";
        let args = json!({"server": "vps", "tool": "vps_exec", "arguments": {"command": command}});
        assert!(needs_approval("mcp_call", &args).is_some());
    }

    /// Les lectures réelles du même soir restent libres, masquage compris.
    #[test]
    fn les_lectures_restent_libres() {
        for command in [
            "cat -A /home/jimmy/jobxpress/secure/.env.api | sed -n '1,40p' | head -45",
            "echo '--- .env.api (deploy) ---' ; grep -E 'OPENCODE|SESSION' /home/jimmy/jobxpress/repo/deploy/.env.api | sed -E 's/(KEY=).*/\\1***/'",
            "ls -la /home/jimmy/jobxpress/repo/deploy/.env.api 2>/dev/null",
            "Get-Content .env | Select-String KEY",
        ] {
            assert_eq!(command_modifies_sensitive(command), None, "{command}");
        }
    }

    /// `--env-file` : docker lit le fichier, il ne le modifie pas.
    #[test]
    fn docker_compose_avec_env_file_est_libre() {
        let command = "cd /home/jimmy/jobxpress/repo/deploy && sudo docker compose --env-file .env.api up -d --build api 2>&1 | tail -15";
        assert_eq!(command_modifies_sensitive(command), None);
    }

    #[test]
    fn les_ecritures_sont_reperees() {
        for command in [
            "echo KEY=1 >> .env",
            "sed -i 's/a/b/' /srv/app/.env.production",
            "cp new.env /srv/app/.env",
            "Set-Content -Path C:\\app\\.env -Value 'X=1'",
            "rm ~/.ssh/id_ed25519",
            "tee /etc/app/secrets.yml < x",
            "mv /tmp/key.pem /etc/ssl/private/server.key",
        ] {
            assert!(command_modifies_sensitive(command).is_some(), "{command}");
        }
    }

    #[test]
    fn les_fichiers_sensibles_sont_reconnus() {
        for path in [".env", "C:\\proj\\.env.local", "/srv/prod.env", "server.key", "/home/j/.ssh/config", "client_secret.json", "/home/jimmy/jobxpress/secure/api.conf"] {
            assert!(is_sensitive_path(path), "{path}");
        }
        for path in [".env.example", "src/main.rs", "environment.ts", "README.md", "keyboard.rs"] {
            assert!(!is_sensitive_path(path), "{path}");
        }
    }

    #[test]
    fn ecriture_locale_et_envoi_mcp() {
        assert!(needs_approval("write_file", &json!({"path": "C:\\proj\\.env"})).is_some());
        assert!(needs_approval("write_file", &json!({"path": "C:\\proj\\src\\main.rs"})).is_none());
        let upload = json!({"server": "vps", "tool": "vps_upload", "arguments": "{\"local_path\":\"C:/tmp/x\",\"remote_path\":\"/srv/app/.env\"}"});
        assert!(needs_approval("mcp_call", &upload).is_some(), "arguments en chaîne JSON");
        let download = json!({"server": "vps", "tool": "vps_download", "arguments": {"remote_path": "/srv/app/.env"}});
        assert!(needs_approval("mcp_call", &download).is_none(), "télécharger = lire");
        assert!(needs_approval("mcp_vps__vps_exec", &json!({"command": "echo X > /srv/.env"})).is_some());
        assert!(needs_approval("read_file", &json!({"path": ".env"})).is_none());
    }

    fn sensitive_call() -> serde_json::Value {
        json!({"command": "echo X=1 >> .env"})
    }

    /// Au Chat : la carte part, l'outil attend, un « Autoriser » le libère.
    #[tokio::test]
    async fn l_accord_libere_l_outil() {
        let approvals = std::sync::Arc::new(Approvals::default());
        let (tx, mut rx) = mpsc::channel(8);
        let waiting = {
            let approvals = approvals.clone();
            tokio::spawn(async move {
                authorize(&approvals, false, "run_command", &sensitive_call(), &tx, Duration::from_secs(5)).await
            })
        };
        let Some(AgentEvent::Approval { id, target, detail }) = rx.recv().await else { panic!("pas de carte") };
        assert_eq!(target, ".env");
        assert!(detail.contains("echo X=1 >> .env"), "{detail}");
        assert!(approvals.respond(&id, true));
        assert_eq!(waiting.await.unwrap(), None, "accord donné : l'outil s'exécute");
        assert!(matches!(rx.recv().await, Some(AgentEvent::ApprovalResolved { approved: true, .. })));
    }

    #[tokio::test]
    async fn le_refus_et_le_silence_bloquent_l_outil() {
        let approvals = std::sync::Arc::new(Approvals::default());
        // Refus explicite.
        let (tx, mut rx) = mpsc::channel(8);
        let waiting = {
            let approvals = approvals.clone();
            tokio::spawn(async move {
                authorize(&approvals, false, "run_command", &sensitive_call(), &tx, Duration::from_secs(5)).await
            })
        };
        let Some(AgentEvent::Approval { id, .. }) = rx.recv().await else { panic!("pas de carte") };
        approvals.respond(&id, false);
        assert!(waiting.await.unwrap().unwrap().starts_with("REFUSÉ"));
        // Pas de réponse dans le délai : refus, et la demande expirée ne
        // peut plus être acceptée après coup.
        let (tx, mut rx) = mpsc::channel(8);
        let refus = authorize(&approvals, false, "run_command", &sensitive_call(), &tx, Duration::from_millis(50)).await;
        assert!(refus.is_some());
        let Some(AgentEvent::Approval { id, .. }) = rx.recv().await else { panic!("pas de carte") };
        assert!(!approvals.respond(&id, true));
    }

    #[tokio::test]
    async fn a_la_voix_refus_immediat_et_rien_de_sensible_passe() {
        let approvals = Approvals::default();
        let (tx, mut rx) = mpsc::channel(8);
        assert!(authorize(&approvals, true, "run_command", &sensitive_call(), &tx, Duration::from_secs(5)).await.is_some());
        let anodin = json!({"command": "git status"});
        assert!(authorize(&approvals, false, "run_command", &anodin, &tx, Duration::from_secs(5)).await.is_none());
        drop(tx);
        assert!(rx.recv().await.is_none(), "aucune carte émise");
    }
}
