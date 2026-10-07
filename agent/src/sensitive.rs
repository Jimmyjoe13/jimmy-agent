//! Fichiers sensibles : rien n'y est modifié sans l'accord explicite de
//! l'utilisateur.
//!
//! Cas réel du 5 octobre 2026 : dans une conversation sur JobXpress, Jimmy a
//! sauvegardé puis réécrit `/home/user/app/secure/.env.api` sur le VPS
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

// Seule une **écriture** repérée déclenche la demande : lire, filtrer,
// masquer ou se servir d'une clé (`ssh -i x.key`) reste libre. Le 6 octobre,
// l'approche inverse (liste de commandes de lecture, tout le reste suspect) a
// demandé onze accords en une matinée pour des `Get-Content`, `Select-String`,
// `findstr` et `ssh -i` : aucune n'écrivait.

/// Commandes qui modifient les fichiers qu'elles nomment (Unix, PowerShell
/// et ses alias, cmd).
const WRITE_VERBS: &[&str] = &[
    "mv", "rm", "unlink", "shred", "truncate", "dd", "tee", "touch", "chmod", "chown", "chattr",
    "ssh-keygen", "set-content", "sc", "add-content", "ac", "out-file", "clear-content", "clc",
    "remove-item", "ri", "del", "erase", "rmdir", "rd", "move-item", "mi", "move", "rename-item",
    "rni", "ren", "rename", "new-item", "ni", "tee-object", "set-item", "si", "set-acl", "icacls",
];

/// Commandes de copie : seule la **destination** est modifiée (copier un
/// `.env` ailleurs le lit).
const COPY_VERBS: &[&str] = &["cp", "copy", "copy-item", "cpi", "scp", "rsync", "install", "ln", "xcopy", "robocopy"];

/// Interpréteurs dont le script (`-c`, `-e`) peut écrire.
const INTERPRETERS: &[&str] = &["python", "python3", "py", "node", "perl", "ruby", "php", "deno", "bun"];

/// Commandes qui en exécutent une autre (distante, conteneur, sous-shell) :
/// la commande portée est analysée à son tour.
const WRAPPERS: &[&str] = &[
    "ssh", "bash", "sh", "zsh", "dash", "pwsh", "powershell", "cmd", "wsl", "docker", "kubectl",
    "su", "runuser", "xargs", "invoke-expression", "iex",
];

/// Mots placés devant la vraie commande, sans effet propre.
const PREFIXES: &[&str] = &["sudo", "doas", "env", "nohup", "time", "exec", "if", "then", "else", "elif", "do", "while", "until", "!"];

/// Une commande d'un segment : ses mots (guillemets retirés) et les cibles de
/// ses redirections `>` / `>>`.
#[derive(Default)]
struct Segment {
    words: Vec<String>,
    targets: Vec<String>,
}

/// Découpe une ligne de commande en segments, hors guillemets : `; | & \n`
/// séparent les commandes ; `( ) { }` ouvrent ou ferment un bloc (sous-shell,
/// bloc PowerShell) dont le premier mot est une nouvelle commande. Un `|` ou
/// un `>` entre guillemets (`grep -E 'A|B'`) n'est donc ni un tube ni une
/// redirection.
fn segments(command: &str) -> Vec<Segment> {
    let mut out = Vec::new();
    let mut seg = Segment::default();
    let mut word = String::new();
    let mut has_word = false; // un mot est commencé (même `''`)
    let mut redirect = false; // le prochain mot est la cible d'un `>`
    let mut quote: Option<char> = None;
    let mut chars = command.chars().peekable();

    // Range le mot en cours, comme argument ou comme cible de redirection.
    let flush = |seg: &mut Segment, word: &mut String, has_word: &mut bool, redirect: &mut bool| {
        if *has_word {
            if *redirect {
                seg.targets.push(std::mem::take(word));
                *redirect = false;
            } else {
                seg.words.push(std::mem::take(word));
            }
        }
        word.clear();
        *has_word = false;
    };

    while let Some(c) = chars.next() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            } else {
                word.push(c);
            }
            continue;
        }
        match c {
            '\'' | '"' => {
                quote = Some(c);
                has_word = true;
            }
            '>' => {
                // `2>` : le chiffre est un descripteur, pas un mot.
                if has_word && word.chars().all(|d| d.is_ascii_digit() || d == '*') {
                    word.clear();
                    has_word = false;
                } else {
                    flush(&mut seg, &mut word, &mut has_word, &mut redirect);
                }
                while chars.peek() == Some(&'>') {
                    chars.next();
                }
                // `>&1`, `>&2` : vers un autre descripteur, aucun fichier.
                if chars.peek() == Some(&'&') {
                    chars.next();
                    while chars.peek().is_some_and(|d| d.is_ascii_digit()) {
                        chars.next();
                    }
                } else {
                    redirect = true;
                }
            }
            ';' | '|' | '&' | '\n' | '(' | ')' | '{' | '}' => {
                flush(&mut seg, &mut word, &mut has_word, &mut redirect);
                redirect = false;
                if !seg.words.is_empty() || !seg.targets.is_empty() {
                    out.push(std::mem::take(&mut seg));
                }
            }
            '<' => flush(&mut seg, &mut word, &mut has_word, &mut redirect),
            c if c.is_whitespace() => flush(&mut seg, &mut word, &mut has_word, &mut redirect),
            c => {
                word.push(c);
                has_word = true;
            }
        }
    }
    flush(&mut seg, &mut word, &mut has_word, &mut redirect);
    if !seg.words.is_empty() || !seg.targets.is_empty() {
        out.push(seg);
    }
    out
}

/// Le mot désigne-t-il (ou contient-il) un fichier sensible ?
fn sensitive_word(word: &str) -> Option<String> {
    if is_sensitive_path(word) {
        Some(word.to_string())
    } else {
        find_sensitive(word)
    }
}

/// Un script (heredoc, `python -c`) qui écrit : ouverture en écriture,
/// suppression, renommage, copie.
fn script_writes(script: &str) -> bool {
    let s = script.to_lowercase();
    [
        "'w'", "\"w\"", "'a'", "\"a\"", "'w+'", "'wb'", "'a+'", "'r+'", "write", "unlink", "os.remove",
        "rename", "shutil.", "truncate", "appendfile", "rmsync", "copyfile", "set-content", "out-file",
    ]
    .iter()
    .any(|marker| s.contains(marker))
}

/// Destination d'une copie : `-Destination X`, sinon le dernier argument
/// positionnel (s'il y en a au moins deux).
fn destination<'a>(rest: &[&'a str]) -> Option<&'a str> {
    if let Some(i) = rest.iter().position(|w| w.eq_ignore_ascii_case("-destination")) {
        return rest.get(i + 1).copied();
    }
    let positional: Vec<&str> = rest.iter().copied().filter(|w| !w.starts_with('-')).collect();
    if positional.len() >= 2 {
        positional.last().copied()
    } else {
        None
    }
}

/// Le segment (mots d'une commande) écrit-il dans un fichier sensible ?
fn words_modify(words: &[&str]) -> Option<String> {
    let start = words
        .iter()
        .position(|w| !PREFIXES.contains(&w.to_lowercase().as_str()) && !(w.contains('=') && !w.starts_with('-')))?;
    let (verb, rest) = words[start..].split_first()?;
    let verb = verb.to_lowercase();
    let verb = verb.rsplit(['/', '\\']).next().unwrap_or(&verb).trim_end_matches(".exe");
    let any_sensitive = || rest.iter().find_map(|w| sensitive_word(w));
    let has_flag = |pred: &dyn Fn(&str) -> bool| rest.iter().any(|w| pred(w));
    match verb {
        v if COPY_VERBS.contains(&v) => destination(rest).and_then(sensitive_word),
        v if WRITE_VERBS.contains(&v) => any_sensitive(),
        // Édition en place : `sed -i`, `perl -i`, `awk -i inplace`.
        "sed" | "perl" if has_flag(&|w| w.starts_with("-i") || w.starts_with("--in-place")) => any_sensitive(),
        "awk" | "gawk" if has_flag(&|w| w == "inplace") => any_sensitive(),
        "find" if has_flag(&|w| ["-delete", "-exec", "-execdir"].contains(&w)) => any_sensitive(),
        v if INTERPRETERS.contains(&v) => rest
            .iter()
            .filter(|w| script_writes(w))
            .find_map(|w| sensitive_word(w)),
        // Commande portée : chaque argument (la commande distante entre
        // guillemets) et chaque suite d'arguments (`docker cp a c:/app/.env`).
        v if WRAPPERS.contains(&v) => rest
            .iter()
            .find_map(|w| if w.contains(' ') { command_modifies_sensitive(w) } else { None })
            .or_else(|| (0..rest.len()).find_map(|i| words_modify(&rest[i..]))),
        _ => None,
    }
}

/// Commandes qui ne font que lire ou afficher (liste blanche : un verbe
/// inconnu n'est **pas** une lecture). `find` et `curl` sont vérifiés à part.
const READ_VERBS: &[&str] = &[
    "get-childitem", "gci", "ls", "dir", "get-content", "gc", "cat", "type", "head", "tail", "less", "more",
    "test-path", "select-string", "sls", "findstr", "grep", "egrep", "rg", "get-item", "gi", "get-itemproperty",
    "gp", "resolve-path", "get-location", "pwd", "get-process", "ps", "tasklist", "get-service", "get-command",
    "where", "where.exe", "which", "test-netconnection", "tnc", "get-nettcpconnection", "netstat", "ss",
    "nslookup", "ping", "ipconfig", "ip", "hostname", "whoami", "uname", "uptime", "df", "du", "free", "id",
    "write-output", "echo", "write-host", "format-table", "ft", "format-list", "fl", "select-object", "select",
    "where-object", "sort-object", "sort", "measure-object", "measure", "out-string", "wc", "uniq", "cut", "jq",
    "get-date", "date", "stat", "file", "realpath", "readlink", "systemctl", "journalctl", "get-filehash",
    "sha256sum", "md5sum", "env", "printenv", "true", "start-sleep", "sleep", "get-scheduledtask",
    "get-scheduledtaskinfo", "export-scheduledtask", "get-ciminstance", "gcim", "get-wmiobject", "gwmi",
    "get-acl", "get-netipaddress", "get-netadapter", "out-string", "out-null", "get-member", "gm",
    "get-variable", "get-host", "get-psdrive", "get-hotfix", "get-computerinfo",
    // Mots de contrôle PowerShell : leur bloc `{ … }` est un segment analysé à part.
    "foreach", "for", "try", "catch", "finally", "foreach-object",
    // Changer de dossier n'écrit rien.
    "cd", "set-location", "sl", "pushd", "popd",
];

/// Méthodes .NET qui modifient : `$k.Delete()`, `(…).Kill()` restent des actions.
const MUTATING_MEMBERS: &[&str] = &[
    "delete", "remove", "kill", "stop", "terminate", "set", "write", "move", "copy", "create", "start", "save", "append",
];

/// Sous-commandes de lecture de `git` et `docker`.
const GIT_READS: &[&str] = &["status", "log", "diff", "show", "branch", "remote", "rev-parse", "ls-files", "blame"];
const DOCKER_READS: &[&str] = &["ps", "logs", "inspect", "images", "version", "info", "stats"];

/// Options de `ssh` suivies d'une valeur (`-i clé`, `-o Option=…`).
const SSH_VALUE_OPTIONS: &[&str] = &[
    "-i", "-o", "-p", "-l", "-F", "-J", "-L", "-R", "-D", "-E", "-c", "-b", "-S", "-W", "-O", "-Q", "-w", "-m", "-e",
];

/// Un segment de commande qui ne fait que lire ?
fn words_read_only(words: &[&str]) -> bool {
    let Some(start) = words
        .iter()
        .position(|w| !PREFIXES.contains(&w.to_lowercase().as_str()) && !(w.contains('=') && !w.starts_with('-')))
    else {
        return true; // seulement des préfixes ou affectations `A=b`
    };
    let (verb, rest) = match words[start..].split_first() {
        Some(split) => split,
        None => return true,
    };
    let verb = verb.to_lowercase();
    let verb = verb.rsplit(['/', '\\']).next().unwrap_or(&verb).trim_end_matches(".exe");
    match verb {
        "find" => !rest.iter().any(|w| ["-delete", "-exec", "-execdir", "-ok", "-fprint"].contains(w)),
        // `systemctl status`, pas `systemctl restart`.
        "systemctl" => rest.first().is_some_and(|w| ["status", "is-active", "list-units", "show"].contains(w)),
        // `curl` en GET, sans envoi ni fichier écrit.
        "curl" => !rest.iter().any(|w| {
            let w = w.to_lowercase();
            ["-d", "--data", "-f", "--form", "-t", "--upload-file", "-o", "--output", "-x", "--request", "-O"]
                .iter()
                .any(|flag| w == *flag || (w.starts_with("--data") && flag.starts_with("--data")))
        }),
        // Requête web PowerShell en GET, sans corps ni fichier.
        "invoke-webrequest" | "iwr" | "invoke-restmethod" | "irm" => {
            let method_get = match rest.iter().position(|w| w.eq_ignore_ascii_case("-method")) {
                Some(i) => rest.get(i + 1).is_some_and(|m| m.eq_ignore_ascii_case("get")),
                None => true,
            };
            method_get
                && !rest.iter().any(|w| ["-body", "-outfile", "-infile", "-form"].contains(&w.to_lowercase().as_str()))
        }
        "schtasks" => rest.iter().any(|w| w.eq_ignore_ascii_case("/query")),
        // Variable, affectation ou propriété (`$k in …`, `$keys = …`,
        // `$_.Trim()`, `(…).StatusCode`) : la valeur est un autre segment,
        // analysé à part. Pas un appel qui modifie (`$k.Delete()`).
        v if (v.starts_with('$') || v.starts_with('.'))
            && v[1..].chars().all(|c| c.is_alphanumeric() || "_:,.".contains(c))
            && !MUTATING_MEMBERS.iter().any(|m| v.contains(m)) =>
        {
            true
        }
        v if READ_VERBS.contains(&v) => true,
        "git" => rest.iter().find(|w| !w.starts_with('-')).is_some_and(|w| GIT_READS.contains(w)),
        "docker" => rest.first().is_some_and(|w| DOCKER_READS.contains(w)),
        // Commande distante : options, hôte, puis la commande portée.
        "ssh" => {
            let mut i = 0;
            while i < rest.len() && rest[i].starts_with('-') {
                i += if SSH_VALUE_OPTIONS.contains(&rest[i]) { 2 } else { 1 };
            }
            let remote = rest.get(i + 1..).unwrap_or(&[]).join(" ");
            !remote.trim().is_empty() && command_is_read_only(&remote)
        }
        _ => false,
    }
}

/// La commande ne fait **que regarder** (lister, lire, tester, interroger un
/// état, y compris à distance par `ssh`) ? Liste blanche : dans le doute, ce
/// n'est pas une lecture. Sert au rappel « agis ou conclus » de la boucle
/// d'agent, pas à la sécurité (qui reste `command_modifies_sensitive`).
pub fn command_is_read_only(command: &str) -> bool {
    let lower = command.to_lowercase();
    if lower.contains("<<") || ["::write", "::append", "::delete", "::copy", "::move"].iter().any(|m| lower.contains(m)) {
        return false;
    }
    let segs = segments(command);
    !segs.is_empty()
        && segs.iter().all(|seg| {
            // Redirection : seule la poubelle (`2>$null`, `> /dev/null`) ne compte pas.
            seg.targets.iter().all(|t| ["$null", "nul", "/dev/null"].contains(&t.to_lowercase().as_str()))
                && words_read_only(&seg.words.iter().map(String::as_str).collect::<Vec<_>>())
        })
}

/// Une ligne de commande (PowerShell ou shell distant) qui **écrit** dans un
/// fichier sensible. Renvoie le fichier en cause ; `None` pour une lecture.
pub fn command_modifies_sensitive(command: &str) -> Option<String> {
    let lower = command.to_lowercase();
    // Heredoc : un script complet (`python3 <<'PY' … open(x,'w')`).
    if let Some(at) = lower.find("<<") {
        let body = lower[at..].split_once('\n').map_or("", |(_, b)| b);
        if script_writes(body) {
            if let Some(target) = find_sensitive(command) {
                return Some(target);
            }
        }
    }
    // Appels .NET depuis PowerShell : `[IO.File]::WriteAllText('.env', …)`.
    if ["::write", "::append", "::delete", "::copy", "::move", "::replace"].iter().any(|m| lower.contains(m)) {
        if let Some(target) = find_sensitive(command) {
            return Some(target);
        }
    }
    segments(command).iter().find_map(|seg| {
        seg.targets
            .iter()
            .find_map(|t| sensitive_word(t))
            .or_else(|| words_modify(&seg.words.iter().map(String::as_str).collect::<Vec<_>>()))
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

    /// Commandes réelles du 7 octobre (inspection SSH), et leurs contraires.
    #[test]
    fn les_commandes_d_inspection_sont_des_lectures() {
        for command in [
            "Get-ChildItem $env:USERPROFILE\\.ssh | Format-Table Name,Length,LastWriteTime",
            "Test-Path \"C:\\Users\\user\\Serveur\\vps.key\"; dir \"C:\\Users\\user\\Serveur\" | Format-Table Name,Length",
            "ssh -i 'C:\\Users\\user\\vps.key' -o BatchMode=yes -o ConnectTimeout=10 ubuntu@203.0.113.10 'docker ps --format x; curl -s localhost:8000/health'",
            "Get-Process ssh -ErrorAction SilentlyContinue 2>$null | Select Id,Path; netstat -ano | Select-String '127.0.0.1:8000'",
            "git status --short; git log --oneline -3",
            "cd /opt/app && git status",
            "Get-Content a.txt | ForEach-Object { $_.Trim() } | Select-Object -First 5",
            "find . -name '*.py' | head",
            "foreach($k in @($a, $b)) { Test-Path $k }",
            "schtasks /query /tn \"Services\" /xml",
            "(Invoke-WebRequest -Uri http://203.0.113.10:8000/v1/health -TimeoutSec 5).StatusCode",
            "Get-CimInstance Win32_Process -Filter \"ProcessId=42\" | Format-List CommandLine",
        ] {
            assert!(command_is_read_only(command), "« {command} » doit être une lecture");
        }
        for command in [
            "Remove-Item build -Recurse",
            "echo x > notes.txt",
            "ssh ubuntu@203.0.113.10 'docker restart api'",
            "ssh ubuntu@203.0.113.10",
            "docker compose up -d",
            "git commit -m x",
            "curl -X POST localhost:8000/v1/memories -d '{}'",
            "find . -name '*.tmp' -delete",
            "systemctl restart nginx",
            "schtasks /delete /tn Services /f",
            "Invoke-WebRequest -Uri http://203.0.113.10/x -Method Post -Body '{}'",
            "$k.Delete()",
            "(Get-Item x.log).Delete()",
            "npm install",
            "python -c \"print(1)\"",
            "",
        ] {
            assert!(!command_is_read_only(command), "« {command} » ne doit pas être une lecture");
        }
    }

    /// La commande réelle du 5 octobre (21:00) : sauvegarde puis réécriture
    /// d'un `.env` de production par un script Python.
    #[test]
    fn la_reecriture_reelle_du_env_est_reperee() {
        let command = "cd /home/user/app/repo && git pull --ff-only 2>&1 | tail -3; \
            cp /home/user/app/secure/.env.api /home/user/app/secure/.env.api.bak-20261005; \
            python3 <<'PY'\nopen('/home/user/app/secure/.env.api','w').write(x)\nPY";
        let args = json!({"server": "vps", "tool": "vps_exec", "arguments": {"command": command}});
        assert!(needs_approval("mcp_call", &args).is_some());
    }

    /// Les lectures réelles du même soir restent libres, masquage compris.
    #[test]
    fn les_lectures_restent_libres() {
        for command in [
            "cat -A /home/user/app/secure/.env.api | sed -n '1,40p' | head -45",
            "echo '--- .env.api (deploy) ---' ; grep -E 'OPENCODE|SESSION' /home/user/app/repo/deploy/.env.api | sed -E 's/(KEY=).*/\\1***/'",
            "ls -la /home/user/app/repo/deploy/.env.api 2>/dev/null",
            "Get-Content .env | Select-String KEY",
        ] {
            assert_eq!(command_modifies_sensitive(command), None, "{command}");
        }
    }

    /// Les onze commandes du 6 octobre qui ont toutes demandé un accord :
    /// aucune n'écrit (lecture masquée, filtre, clé SSH utilisée). Les trois
    /// dernières, tronquées dans le journal, sont complétées à l'identique.
    #[test]
    fn les_lectures_du_6_octobre_restent_libres() {
        for command in [
            r#"Get-Content "C:\Users\user\synaptiq\.env" | ForEach-Object { if ($_ -match '^\s*#' -or $_ -eq '') { $_ } else { ($_ -replace '=.+', '=***') } }"#,
            r#"if (Test-Path "$env:USERPROFILE\.ssh\config") { Get-Content "$env:USERPROFILE\.ssh\config" } else { "pas de config ssh" }; ls "$env:USERPROFILE\.ssh" | Select-Object Name"#,
            r#"Get-Content C:\Users\user\.ssh\known_hosts | ForEach-Object { ($_ -split ' ')[0] } | Sort-Object -Unique"#,
            r#"Select-String -Path C:\Users\user\aggregate-full-app\.env -Pattern "^(SSH_HOST|SSH_USER|SSH_PORT|SSH_KEY_FILE)=" | ForEach-Object { $_.Line }"#,
            r#"ssh -i "C:\Users\user\Serveur Ubuntu\vps.key" -o StrictHostKeyChecking=accept-new -o ConnectTimeout=15 -o BatchMode=yes ubuntu@203.0.113.10 "hostname; uptime; free -h; df -h /""#,
            r#"ssh -i "C:\Users\user\Serveur Ubuntu\vps.key" -o BatchMode=yes ubuntu@203.0.113.10 "docker stats --no-stream --format 'table {{.Name}}\t{{.MemUsage}}' 2>&1 | head -20""#,
            r#"findstr /B "SYNAPTIQ_AUTH_REQUIRED SYNAPTIQ_LLM_BASE_URL" .env | findstr /V KEY SECRET PASSWORD TOKEN"#,
            r#"Select-String -Path .env -Pattern '^SYNAPTIQ_AUTH_REQUIRED|^SYNAPTIQ_JUDGE' | ForEach-Object { $_.Line -replace '(KEY|SECRET|PASSWORD)=.*', '$1=***' }"#,
            r#"Select-String -Path .env -Pattern 'EMBED|LLM|JUDGE' | ForEach-Object { ($_.Line -split '=')[0] }"#,
            "cp /srv/app/.env /tmp/sauvegarde-lisible",
        ] {
            assert_eq!(command_modifies_sensitive(command), None, "{command}");
        }
    }

    /// Les écritures déguisées restent repérées : distante, conteneur, script,
    /// bloc PowerShell, API .NET.
    #[test]
    fn les_ecritures_indirectes_sont_reperees() {
        for command in [
            r#"ssh -i "C:\k\vps.key" ubuntu@1.2.3.4 "echo X=1 >> /srv/app/.env""#,
            "ssh vps sed -i s/a/b/ /srv/app/.env",
            "docker cp ./nouveau c1:/app/.env",
            r#"docker exec c1 sh -c "printf 'X=1' > /app/.env""#,
            r#"python3 -c "open('/srv/.env','w').write('x')""#,
            r#"if (Test-Path x) { Set-Content -Path .env -Value 1 }"#,
            r#"[IO.File]::WriteAllText('C:\app\.env', 'x')"#,
            "Get-Content a.txt | Out-File C:\\app\\.env",
        ] {
            assert!(command_modifies_sensitive(command).is_some(), "{command}");
        }
    }

    /// `--env-file` : docker lit le fichier, il ne le modifie pas.
    #[test]
    fn docker_compose_avec_env_file_est_libre() {
        let command = "cd /home/user/app/repo/deploy && sudo docker compose --env-file .env.api up -d --build api 2>&1 | tail -15";
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
        for path in [".env", "C:\\proj\\.env.local", "/srv/prod.env", "server.key", "/home/j/.ssh/config", "client_secret.json", "/home/user/app/secure/api.conf"] {
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
