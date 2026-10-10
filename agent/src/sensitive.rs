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
//! « Toujours autoriser » (10 octobre) : l'accord est retenu, par **type
//! d'action** pour le navigateur et par **fichier** pour les fichiers
//! sensibles ([`always_key`]), dans `data/approvals.json`. Une action ainsi
//! accordée passe sans carte, à la voix comme au Chat ; Paramètres → Sécurité
//! les liste et les retire.
//!
//! C'est une détection par motifs, pas un bac à sable : un script
//! intermédiaire qui ne nomme pas le fichier y échappe. Le prompt interdit ce
//! contournement ; le garde-fou couvre les cas réels constatés.

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
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

/// Préfixe de la cible d'une action du navigateur (carte « action en ton
/// nom ») : l'interface et les messages de refus la distinguent d'un fichier.
pub const BROWSER_TARGET: &str = "navigateur : ";

/// Mots d'un élément de page dont le clic **engage** l'utilisateur : envoyer,
/// payer, publier, supprimer… (navigation en son nom, décision du 7 octobre).
/// « Accepter » n'y est pas : il ferait demander pour chaque bandeau de
/// cookies.
// Formes précises : « book » prenait « Facebook », « command » le lien « Mes
// commandes », « pay » le logo PayPal (d'où « pay » suivi d'une espace).
const COMMITTING_WORDS: &[&str] = &[
    "envoy", "envoi", "send", "submit", "soumet", "payer", "paiement", "pay ", "pay now", "acheter", "achat",
    "buy", "purchase", "commander", "passer la commande", "place order", "checkout", "publier", "publish",
    "poster", "post ", "tweet", "partager", "share", "supprim", "delete", "remove", "effacer", "confirm",
    "valider", "validate", "souscri", "subscribe", "s'abonner", "virement", "transfer", "réserver", "reserve",
    "signer", "répondre", "reply",
];

/// Champs dont la validation (`submit`) ne fait que chercher ou naviguer.
const SEARCH_WORDS: &[&str] = &["recherch", "search", "cherch", "adresse", "address", "url", "filtr", "filter"];

/// Action du navigateur (serveur MCP Playwright, outils `browser_*`) qui
/// engage l'utilisateur et demande son accord : clic ou validation dont la
/// description contient un mot d'engagement, envoi d'un fichier, script
/// arbitraire. La lecture, la navigation et la saisie simple restent libres.
fn browser_action_needs_approval(tool: &str, args: &serde_json::Value) -> Option<String> {
    let element = args.get("element").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let lower = element.to_lowercase();
    let committing = COMMITTING_WORDS.iter().any(|w| lower.contains(w));
    let target = |what: &str| Some(format!("{BROWSER_TARGET}{what}"));
    match tool {
        "browser_click" if committing => target(&format!("clic sur « {element} »")),
        // Saisie validée par Entrée : envoie un message, un formulaire… sauf
        // dans une barre de recherche ou d'adresse.
        "browser_type"
            if args.get("submit").and_then(|v| v.as_bool()).unwrap_or(false)
                && !SEARCH_WORDS.iter().any(|w| lower.contains(w)) =>
        {
            target(&format!("saisie validée dans « {element} »"))
        }
        "browser_press_key"
            if args.get("key").and_then(|v| v.as_str()).is_some_and(|k| k.eq_ignore_ascii_case("enter")) =>
        {
            target("touche Entrée (peut valider un formulaire)")
        }
        "browser_file_upload" => target("envoi d'un fichier depuis le PC"),
        // Scripts : jugés sur ce qu'ils font, pas sur leur seule existence
        // (9 octobre : une carte par lecture de page rendait le navigateur
        // inutilisable).
        "browser_run_code" | "browser_run_code_unsafe" => {
            script_needs_approval(args.get("code").and_then(|v| v.as_str()).unwrap_or("")).and_then(|why| target(why))
        }
        "browser_evaluate" => {
            script_needs_approval(args.get("function").and_then(|v| v.as_str()).unwrap_or("")).and_then(|why| target(why))
        }
        _ => None,
    }
}

/// Un script de page (`browser_run_code`, `browser_evaluate`) engage-t-il
/// l'utilisateur ? Lire, faire défiler, naviguer, capturer, fermer une
/// fenêtre restent libres ; demandent l'accord : envoyer un fichier du PC,
/// une requête qui écrit, valider un formulaire, saisir un mot de passe,
/// cliquer ou appuyer sur Entrée là où ça engage (mêmes mots que les clics).
fn script_needs_approval(code: &str) -> Option<&'static str> {
    let lower = code.to_lowercase();
    // Sans espaces : « method : 'POST' » et « method:'post' » se valent.
    let compact: String = lower.chars().filter(|c| !c.is_whitespace()).collect();
    let has = |marks: &[&str]| marks.iter().any(|m| lower.contains(m));

    if has(&["setinputfiles", "filechooser"]) {
        return Some("envoi d'un fichier depuis le PC");
    }
    let writes = ["post", "put", "patch", "delete"].iter().any(|verb| {
        ["'", "\"", "`", ""].iter().any(|q| compact.contains(&format!("method:{q}{verb}")))
            || compact.contains(&format!("request.{verb}("))
    });
    if writes || lower.contains("sendbeacon") {
        return Some("requête qui envoie des données");
    }
    if has(&[".submit(", "requestsubmit("]) {
        return Some("validation d'un formulaire");
    }
    let types = has(&["fill(", "type(", "presssequentially(", ".value="]) || compact.contains(".value=");
    if types && has(&["password", "mot de passe", "passwd"]) {
        return Some("saisie d'un mot de passe ou d'un identifiant");
    }
    let presses_enter = lower.contains("enter") && has(&["press(", "keyboardevent", "dispatchevent"]);
    if presses_enter && !SEARCH_WORDS.iter().any(|w| lower.contains(w)) {
        return Some("touche Entrée envoyée par script (peut valider un message)");
    }
    // Le mot d'engagement se cherche dans les chaînes du script (sélecteur,
    // texte du bouton : `'Envoyer'`, `'button.send'`), jamais dans le code :
    // `el.remove()` ou une fonction `sendKey` ne sont pas des boutons
    // (9 octobre au soir : deux cartes à tort, bandeau cookies et jeu 2048).
    let acts = has(&["click(", ".tap(", "dispatchevent"]);
    let literals = string_literals(&lower);
    if acts && COMMITTING_WORDS.iter().any(|w| literals.contains(w)) {
        return Some("clic par script sur un élément qui engage");
    }
    None
}

/// Contenu des chaînes d'un script JS (`'…'`, `"…"`, `` `…` ``), mis bout à
/// bout et séparés par un saut de ligne. Une apostrophe isolée (commentaire en
/// français) ouvre une fausse chaîne : on demande alors un peu plus, jamais
/// moins — le bon côté de l'erreur.
fn string_literals(code: &str) -> String {
    let mut out = String::new();
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for c in code.chars() {
        match quote {
            None if matches!(c, '\'' | '"' | '`') => quote = Some(c),
            None => {}
            Some(_) if escaped => {
                escaped = false;
                out.push(c);
            }
            Some(_) if c == '\\' => escaped = true,
            Some(q) if c == q => {
                quote = None;
                out.push('\n');
            }
            Some(_) => out.push(c),
        }
    }
    out
}

/// L'appel d'outil modifie-t-il un fichier sensible ? Renvoie le fichier en
/// cause ; `None` pour tout le reste (lecture comprise).
pub fn needs_approval(tool: &str, args: &serde_json::Value) -> Option<String> {
    let arg = |key: &str| args.get(key).and_then(|v| v.as_str()).unwrap_or("");
    match tool {
        "write_file" => Some(arg("path")).filter(|p| is_sensitive_path(p)).map(str::to_string),
        "run_command" => command_modifies_sensitive(arg("command")),
        "mcp_call" => {
            let inner = inner_args(args.get("arguments"));
            browser_action_needs_approval(arg("tool"), &inner).or_else(|| mcp_modifies_sensitive(arg("tool"), &inner))
        }
        // Outil MCP enregistré directement (`mcp_<serveur>__<outil>`).
        name if name.starts_with("mcp_") => {
            let short = name.split("__").nth(1).unwrap_or(name);
            browser_action_needs_approval(short, args).or_else(|| mcp_modifies_sensitive(short, args))
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
        .find_map(|k| inner.get(*k).and_then(|v| v.as_str()).map(mask_text))
        .unwrap_or_else(|| serde_json::to_string_pretty(&mask_json(&inner)).unwrap_or_default());
    let body: String = body.chars().take(2000).collect();
    format!("{label}\n{body}")
}

/// Portée d'un « Toujours autoriser » pour une cible de [`needs_approval`].
/// Navigateur : le **type** d'action, sans l'élément visé (« clic sur
/// « Envoyer » » et « clic sur « Publier » » partagent un accord). Fichier
/// sensible : le chemin exact, casse et séparateurs normalisés.
pub fn always_key(target: &str) -> String {
    match target.strip_prefix(BROWSER_TARGET) {
        Some(action) if action.starts_with("clic sur « ") => {
            format!("{BROWSER_TARGET}clic sur un élément qui engage")
        }
        Some(action) if action.starts_with("saisie validée dans « ") => {
            format!("{BROWSER_TARGET}saisie validée (hors recherche)")
        }
        // Les autres types (script, Entrée, envoi de fichier…) n'embarquent
        // pas d'élément : la cible est déjà le type.
        Some(_) => target.to_string(),
        None => target.to_lowercase().replace('/', "\\"),
    }
}

// ── Demande d'autorisation ───────────────────────────────────────────────────

/// Demandes en attente d'une réponse de l'utilisateur (une par appel d'outil
/// suspendu). Partagé entre la boucle d'agent et la commande Tauri qui
/// transmet le clic « Autoriser / Refuser ».
#[derive(Default)]
pub struct Approvals {
    next: AtomicU64,
    /// Demandes en attente : la réponse à transmettre et la portée
    /// ([`always_key`]) à retenir si l'utilisateur choisit « Toujours ».
    pending: Mutex<HashMap<String, (oneshot::Sender<bool>, String)>>,
    /// Portées accordées une fois pour toutes.
    always: Mutex<BTreeSet<String>>,
    /// Fichier où elles survivent au redémarrage (`None` : en mémoire seule,
    /// pour les tests).
    store: Option<PathBuf>,
}

impl Approvals {
    /// Charge les accords permanents de `path` (absent ou illisible : aucun).
    pub fn load(path: PathBuf) -> Self {
        let always = std::fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str::<BTreeSet<String>>(&raw).ok())
            .unwrap_or_default();
        Approvals { always: Mutex::new(always), store: Some(path), ..Default::default() }
    }

    /// Ouvre une demande pour la portée `key` : son identifiant et l'attente
    /// de la réponse.
    pub fn request(&self, key: String) -> (String, oneshot::Receiver<bool>) {
        let id = format!("approval-{}", self.next.fetch_add(1, Ordering::Relaxed) + 1);
        let (tx, rx) = oneshot::channel();
        if let Ok(mut pending) = self.pending.lock() {
            pending.insert(id.clone(), (tx, key));
        }
        (id, rx)
    }

    /// Réponse de l'utilisateur. `always` (avec `approved`) retient la portée
    /// de la demande. `false` si la demande n'existe plus (expirée, tâche
    /// arrêtée) : rien n'est retenu non plus.
    pub fn respond(&self, id: &str, approved: bool, always: bool) -> bool {
        let Some((tx, key)) = self.pending.lock().ok().and_then(|mut p| p.remove(id)) else {
            return false;
        };
        if approved && always {
            self.grant(key);
        }
        tx.send(approved).is_ok()
    }

    /// La portée est-elle accordée une fois pour toutes ?
    pub fn is_always(&self, key: &str) -> bool {
        self.always.lock().is_ok_and(|always| always.contains(key))
    }

    /// Accords permanents, triés, pour Paramètres.
    pub fn always_list(&self) -> Vec<String> {
        self.always.lock().map(|always| always.iter().cloned().collect()).unwrap_or_default()
    }

    /// Retire un accord permanent : la carte reviendra. `false` s'il n'existait pas.
    pub fn revoke(&self, key: &str) -> bool {
        let removed = self.always.lock().is_ok_and(|mut always| always.remove(key));
        if removed {
            self.save();
        }
        removed
    }

    fn grant(&self, key: String) {
        log::info!("[sécurité] toujours autorisé désormais : {key}");
        if let Ok(mut always) = self.always.lock() {
            always.insert(key);
        }
        self.save();
    }

    fn save(&self) {
        let Some(path) = &self.store else { return };
        let list = self.always_list();
        let written = serde_json::to_string_pretty(&list)
            .map_err(|e| e.to_string())
            .and_then(|json| std::fs::write(path, json).map_err(|e| e.to_string()));
        if let Err(e) = written {
            log::warn!("[sécurité] accords permanents non enregistrés ({}) : {e}", path.display());
        }
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
    let key = always_key(&target);
    // Accordé une fois pour toutes : ni carte ni refus vocal.
    if approvals.is_always(&key) {
        log::info!("[sécurité] « {target} » : toujours autorisé ({key})");
        return None;
    }
    // Action du navigateur en son nom, ou écriture d'un fichier sensible :
    // même carte, mots différents.
    let what = match target.strip_prefix(BROWSER_TARGET) {
        Some(action) => format!("l'action dans le navigateur ({action})"),
        None => format!("la modification de « {target} » (fichier sensible)"),
    };
    if voice {
        log::warn!("[sécurité] {what} refusée : demande vocale, accord impossible");
        return Some(format!(
            "REFUSÉ : {what} exige l'accord explicite de l'utilisateur, impossible à donner à la voix. \
             Ne tente pas de la faire autrement ; dis-lui de refaire la demande dans le Chat écrit."
        ));
    }
    let (id, answer) = approvals.request(key.clone());
    log::info!("[sécurité] autorisation demandée ({id}) pour modifier « {target} »");
    let _ = events
        .send(AgentEvent::Approval {
            id: id.clone(),
            target: target.clone(),
            detail: describe(tool, args),
            scope: key,
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
            "REFUSÉ par l'utilisateur (ou sans réponse) : {what} n'a PAS été faite. Ne tente pas de la faire \
             autrement (autre commande, autre bouton, script intermédiaire) ; explique ce que tu voulais faire \
             et pourquoi, et laisse-le décider."
        ))
    }
}

// ── Masquage des secrets avant journalisation ou affichage ───────────────────
// (HANDOFF « Ensuite » 3 : les arguments d'outils partaient en clair dans
// `jimmy.log`, clés `aggregate` et SynaptiQ comprises. Même règle que
// `mcp::mask_command` : le nom reste visible, seule la valeur est cachée.)

/// Masque les secrets d'une valeur JSON : objet dont la clé parle d'un
/// secret → valeur cachée ; chaînes → motifs textuels (`mask_text`) ;
/// le reste est parcouru tel quel.
pub fn mask_json(value: &serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match value {
        Value::String(text) => Value::String(mask_text(text)),
        Value::Array(items) => Value::Array(items.iter().map(mask_json).collect()),
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(key, val)| {
                    if crate::mcp::is_secret_name(key) {
                        (key.clone(), Value::String("••••".to_string()))
                    } else {
                        (key.clone(), mask_json(val))
                    }
                })
                .collect(),
        ),
        _ => value.clone(),
    }
}

/// Masque les secrets d'un texte libre : `NOM=valeur` / `NOM: valeur` dont
/// le nom parle d'un secret, jetons `sk-…`, `Bearer …`, identifiants d'URL.
/// Mot à mot (sans dépendance) : la ponctuation autour est conservée pour
/// garder le diagnostic lisible.
pub fn mask_text(text: &str) -> String {
    let mut out = Vec::new();
    let mut mask_next = false;
    let mut after_key = false;
    for word in text.split_whitespace() {
        let (lead, core, trail) = split_punct(word);
        let masked = if (mask_next || after_key) && !core.is_empty() {
            format!("{lead}••••{trail}")
        } else {
            mask_word(lead, core, trail)
        };
        mask_next = core.eq_ignore_ascii_case("bearer");
        after_key = is_bare_secret_key(core);
        out.push(masked);
    }
    out.join(" ")
}

/// `NOM:` (nom de secret, sans valeur dans le même mot) : la valeur suit
/// dans le mot suivant (`X-API-Key: <clé>`, `PASSWORD: xxx`).
fn is_bare_secret_key(core: &str) -> bool {
    let Some(name) = core.strip_suffix(':') else {
        return false;
    };
    if name.contains("://") || name.len() <= 1 {
        return false;
    }
    crate::mcp::is_secret_name(name.trim_matches(['"', '\'']))
}

/// Découpe un mot en ponctuation d'entourage + cœur. `:` et `=` ne sont
/// jamais rognés : ce sont des séparateurs d'assignation.
fn split_punct(word: &str) -> (&str, &str, &str) {
    let lead_len = word
        .char_indices()
        .take_while(|(_, c)| matches!(c, '"' | '\'' | '(' | '[' | '{' | '<'))
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0);
    let trail_len = word
        .char_indices()
        .rev()
        .take_while(|(_, c)| matches!(c, '"' | '\'' | ',' | ';' | ')' | ']' | '}' | '>' | '.' | '!' | '?'))
        .count();
    let core_end = word.len().saturating_sub(trail_len);
    if lead_len >= core_end {
        return (word, "", "");
    }
    (&word[..lead_len], &word[lead_len..core_end], &word[core_end..])
}

fn mask_word(lead: &str, core: &str, trail: &str) -> String {
    if core.is_empty() {
        return format!("{lead}{trail}");
    }
    // Jeton `sk-…` (OpenRouter, SynaptiQ…) : préfixe gardé, valeur cachée.
    if core.len() > 8
        && core.starts_with("sk-")
        && core.chars().skip(3).all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        return format!("{lead}sk-••••{trail}");
    }
    // Identifiants dans une URL (`user:motdepasse@hôte`, `?api_key=…`).
    if let Some(masked) = mask_userinfo(core) {
        return format!("{lead}{masked}{trail}");
    }
    // `X-API-Key: valeur`, `--token=valeur`, `PASSWORD=…`.
    for sep in ['=', ':'] {
        if let Some((name, _)) = core.split_once(sep) {
            // `C:\…`, `https://…` : un `:` qui n'introduit pas de valeur.
            let is_path_or_url =
                sep == ':' && (name.len() == 1 || core[name.len()..].starts_with("://"));
            if !is_path_or_url && crate::mcp::is_secret_name(name.trim_matches(['"', '\''])) {
                return format!("{lead}{name}{sep}••••{trail}");
            }
        }
    }
    format!("{lead}{core}{trail}")
}

fn mask_userinfo(word: &str) -> Option<String> {
    let (_, rest) = word.split_once("://")?;
    let (credentials, host) = rest.split_once('@')?;
    if credentials.contains('/') {
        return None;
    }
    let scheme = word.split("://").next().unwrap_or("");
    let user = credentials.split(':').next().unwrap_or("");
    let (base, query) = match host.split_once('?') {
        Some((base, query)) => (base, Some(query)),
        None => (host, None),
    };
    let mut masked = format!("{scheme}://{user}:••••@{base}");
    if let Some(query) = query {
        let params: Vec<String> = query
            .split('&')
            .map(|param| match param.split_once('=') {
                Some((key, _)) if crate::mcp::is_secret_name(key) => format!("{key}=••••"),
                _ => param.to_string(),
            })
            .collect();
        masked.push('?');
        masked.push_str(&params.join("&"));
    }
    Some(masked)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Navigation en son nom : seules les actions qui engagent demandent
    /// l'accord (formes réelles des arguments de Playwright MCP 0.0.83).
    #[test]
    fn le_navigateur_demande_avant_d_engager_l_utilisateur() {
        let call = |tool: &str, arguments: serde_json::Value| {
            needs_approval("mcp_call", &json!({ "server": "navigateur", "tool": tool, "arguments": arguments }))
        };
        // Engagent : envoyer, payer, publier, supprimer, valider un message.
        for (tool, arguments) in [
            ("browser_click", json!({ "element": "Bouton Envoyer", "ref": "e12" })),
            ("browser_click", json!({ "element": "Payer 49,90 €", "ref": "e3" })),
            ("browser_click", json!({ "element": "Publish post button", "ref": "e3" })),
            ("browser_click", json!({ "element": "Supprimer le compte", "ref": "e9" })),
            ("browser_type", json!({ "element": "Zone de message", "ref": "e4", "text": "Salut", "submit": true })),
            ("browser_press_key", json!({ "key": "Enter" })),
            ("browser_file_upload", json!({ "paths": ["C:\\Users\\user\\cv.pdf"] })),
        ] {
            let target = call(tool, arguments.clone()).unwrap_or_else(|| panic!("{tool} {arguments} doit demander"));
            assert!(target.starts_with(BROWSER_TARGET), "{target}");
        }
        // Libres : naviguer, lire, chercher, saisir sans valider, cookies.
        for (tool, arguments) in [
            ("browser_navigate", json!({ "url": "https://mail.example.com" })),
            ("browser_snapshot", json!({})),
            ("browser_click", json!({ "element": "Lien Boîte de réception", "ref": "e2" })),
            ("browser_click", json!({ "element": "Accepter les cookies", "ref": "e5" })),
            ("browser_click", json!({ "element": "Lien Facebook", "ref": "e6" })),
            ("browser_click", json!({ "element": "Mes commandes", "ref": "e7" })),
            ("browser_click", json!({ "element": "Logo PayPal", "ref": "e8" })),
            ("browser_type", json!({ "element": "Barre de recherche", "ref": "e1", "text": "facture", "submit": true })),
            ("browser_type", json!({ "element": "Zone de message", "ref": "e4", "text": "Brouillon" })),
            ("browser_press_key", json!({ "key": "ArrowDown" })),
            ("browser_evaluate", json!({ "function": "() => document.title" })),
        ] {
            assert_eq!(call(tool, arguments.clone()), None, "{tool} {arguments} doit rester libre");
        }
        // Outil enregistré directement : même règle.
        assert!(needs_approval("mcp_navigateur__browser_click", &json!({ "element": "Send", "ref": "e1" })).is_some());
    }

    /// Scripts de page (9 octobre) : 64 cartes en deux jours pour des scripts
    /// qui ne faisaient que lire. Seul un script qui engage demande l'accord.
    #[test]
    fn les_scripts_du_navigateur_ne_demandent_que_s_ils_engagent() {
        let run = |code: &str| {
            needs_approval(
                "mcp_call",
                &json!({ "server": "navigateur", "tool": "browser_run_code_unsafe", "arguments": { "code": code } }),
            )
        };
        let eval = |function: &str| {
            needs_approval(
                "mcp_call",
                &json!({ "server": "navigateur", "tool": "browser_evaluate", "arguments": { "function": function } }),
            )
        };
        // Libres : scripts réels du journal (lecture, capture, navigation,
        // fermeture d'un tiroir) et défilement.
        for code in [
            "async (page) => { const html = await page.content(); return html.slice(0, 15000); }",
            "async (page) => { await page.screenshot({fullPage: false}); return 'screenshot taken, url='+page.url(); }",
            "async (page) => { await page.goto('https://www.linkedin.com/feed/'); return await page.title(); }",
            "async (page) => { await page.mouse.wheel(0, 2000); await page.waitForTimeout(1000); return page.url(); }",
            "async (page) => { await page.getByRole('button', { name: 'Fermer' }).click(); }",
            "async (page) => { const r = await fetch('/api/items'); return await r.json(); }",
            // Scripts réels du 9 octobre au soir : le mot d'engagement est un
            // identifiant JS (`el.remove()`, `sendKey`), pas un bouton.
            "async (page) => { await page.evaluate(() => { document.querySelectorAll('#cmpwrapper, div[role=\"dialog\"]').forEach(el => { el.remove(); }); }); try { await page.getByRole('button').nth(3).click({timeout:2000}); } catch(e){} }",
            "async (page) => { await page.evaluate(async () => { function sendKey(k){ const e1=new KeyboardEvent('keydown',{key:k,bubbles:true}); document.dispatchEvent(e1); } sendKey('ArrowLeft'); }); }",
        ] {
            assert_eq!(run(code), None, "{code} doit rester libre");
        }
        for function in [
            "() => { return document.body.innerText.slice(0,8000); }",
            "() => { document.querySelector('#joy-drawer--close')?.click(); return 'closed'; }",
            "() => { window.scrollBy(0, 1500); return document.title; }",
        ] {
            assert_eq!(eval(function), None, "{function} doit rester libre");
        }
        // Demandent : clic qui engage, Entrée qui envoie, formulaire validé,
        // requête qui écrit, fichier envoyé, mot de passe saisi.
        for code in [
            "async (page) => { await page.getByRole('button', { name: 'Envoyer' }).click(); }",
            "async (page) => { await page.locator('text=Publier').click(); }",
            "async (page) => { await page.fill('#message', 'Salut'); await page.keyboard.press('Enter'); }",
            "async (page) => { await page.locator('form').evaluate(f => f.requestSubmit()); }",
            "async (page) => { await page.setInputFiles('input[type=file]', 'C:\\\\Users\\\\user\\\\cv.pdf'); }",
            "async (page) => { await fetch('/api/posts', { method: 'POST', body: '{}' }); }",
            "async (page) => { await page.fill('input[type=password]', 'x'); }",
        ] {
            let target = run(code).unwrap_or_else(|| panic!("{code} doit demander"));
            assert!(target.starts_with(BROWSER_TARGET), "{target}");
        }
        for function in [
            "() => document.querySelector('button.send').click()",
            "() => document.forms[0].submit()",
            "() => fetch('/api/delete', { method: 'DELETE' })",
        ] {
            assert!(eval(function).is_some(), "{function} doit demander");
        }
    }

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

    /// Cas réel (HANDOFF « Ensuite » 3) : la clé du serveur MCP `aggregate`
    /// et une clé SynaptiQ figuraient en clair dans `jimmy.log`, car les
    /// arguments d'outils y sont journalisés tels quels. Le masquage doit
    /// les faire disparaître tout en gardant le reste lisible.
    #[test]
    fn les_arguments_journalises_ne_fuitent_pas_les_cles() {
        let arguments = json!({
            "name": "aggregate",
            "command": "npx mcp-remote https://api.example.com/mcp --header \"X-API-Key: cle-agregate-12345\"",
            "url": "https://user:motdepasse-67890@api.example.com/mcp?api_key=cle-requete-abcde",
            "env": { "SYNAPTIQ_API_KEY": "cle-synaptiq-xyz" },
            "model": "sk-or-v1-abcdef123456",
            "note": "Authorization: Bearer jeton-porte-999",
        });
        let logged = mask_json(&arguments).to_string();
        for secret in [
            "cle-agregate-12345",
            "motdepasse-67890",
            "cle-requete-abcde",
            "cle-synaptiq-xyz",
            "sk-or-v1-abcdef123456",
            "jeton-porte-999",
        ] {
            assert!(!logged.contains(secret), "fuite au journal : {secret}");
        }
        // Le diagnostic reste utile : noms visibles, valeurs cachées.
        assert!(logged.contains("aggregate"));
        assert!(logged.contains("api_key"));
    }

    /// Ponctuation double en fin de mot (`",` du JSON) : le cœur ne doit pas
    /// être rogné, sinon un bout de secret reste dans la traîne.
    #[test]
    fn la_ponctuation_double_ne_coupe_pas_le_masque() {
        assert_eq!(mask_text("TOKEN=abc123\","), "TOKEN=••••\",");
        assert_eq!(mask_text("(secret: xyz789);"), "(secret:•••• ••••);");
    }

    /// La carte « Autoriser / Refuser » montre la commande exacte : la clé
    /// n'a pas à y figurer en clair (même règle que le journal).
    #[test]
    fn la_carte_d_autorisation_masque_les_cles() {
        let detail = describe(
            "mcp_call",
            &json!({
                "server": "aggregate",
                "tool": "add",
                "arguments": {"command": "mcp-remote https://api.example.com --header \"X-API-Key: cle-carte-456\""}
            }),
        );
        assert!(!detail.contains("cle-carte-456"), "fuite sur la carte : {detail}");
        assert!(detail.contains("aggregate"));
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
        let Some(AgentEvent::Approval { id, target, detail, .. }) = rx.recv().await else { panic!("pas de carte") };
        assert_eq!(target, ".env");
        assert!(detail.contains("echo X=1 >> .env"), "{detail}");
        assert!(approvals.respond(&id, true, false));
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
        approvals.respond(&id, false, false);
        assert!(waiting.await.unwrap().unwrap().starts_with("REFUSÉ"));
        // Pas de réponse dans le délai : refus, et la demande expirée ne
        // peut plus être acceptée après coup.
        let (tx, mut rx) = mpsc::channel(8);
        let refus = authorize(&approvals, false, "run_command", &sensitive_call(), &tx, Duration::from_millis(50)).await;
        assert!(refus.is_some());
        let Some(AgentEvent::Approval { id, .. }) = rx.recv().await else { panic!("pas de carte") };
        assert!(!approvals.respond(&id, true, false));
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

    /// La portée de « Toujours » : le type d'action pour le navigateur (pas
    /// l'élément), le fichier exact pour les fichiers sensibles.
    #[test]
    fn la_portee_de_toujours() {
        let nav = |tool: &str, arguments: serde_json::Value| {
            needs_approval("mcp_call", &json!({ "server": "navigateur", "tool": tool, "arguments": arguments }))
                .map(|target| always_key(&target))
                .expect("carte attendue")
        };
        let envoyer = nav("browser_click", json!({ "element": "Bouton Envoyer", "ref": "e1" }));
        let publier = nav("browser_click", json!({ "element": "Publish post button", "ref": "e2" }));
        assert_eq!(envoyer, publier, "deux clics qui engagent = un seul accord");
        let saisie = nav("browser_type", json!({ "element": "zone prompt Gemini", "ref": "e3", "text": "x", "submit": true }));
        assert_eq!(saisie, "navigateur : saisie validée (hors recherche)");
        let envoi = nav("browser_file_upload", json!({ "paths": [r"C:\Users\user\cv.pdf"] }));
        assert_ne!(envoi, envoyer, "accorder les clics n'accorde pas l'envoi de fichiers");
        let script = nav("browser_run_code_unsafe", json!({ "code": "await page.getByRole('button', { name: 'Envoyer' }).click()" }));
        assert_eq!(script, "navigateur : clic par script sur un élément qui engage");
        assert_eq!(always_key("C:/Projet/App/.ENV"), always_key(r"c:\projet\app\.env"));
        assert_ne!(always_key(r"C:\a\.env"), always_key(r"C:\b\.env"), "par fichier, pas tous les .env");
    }

    /// « Toujours autoriser » : plus de carte pour la même portée, à la voix
    /// comme au Chat, après redémarrage ; retiré, la carte revient.
    #[tokio::test]
    async fn toujours_autoriser_retient_l_accord() {
        let path = std::env::temp_dir().join(format!("approvals-{}.json", uuid::Uuid::new_v4()));
        let approvals = std::sync::Arc::new(Approvals::load(path.clone()));
        let clic = |element: &str| {
            json!({ "server": "navigateur", "tool": "browser_click", "arguments": { "element": element, "ref": "e1" } })
        };
        let (tx, mut rx) = mpsc::channel(8);
        let waiting = {
            let approvals = approvals.clone();
            let call = clic("Bouton Envoyer");
            tokio::spawn(async move { authorize(&approvals, false, "mcp_call", &call, &tx, Duration::from_secs(5)).await })
        };
        let Some(AgentEvent::Approval { id, scope, .. }) = rx.recv().await else { panic!("pas de carte") };
        assert_eq!(scope, "navigateur : clic sur un élément qui engage");
        assert!(approvals.respond(&id, true, true));
        assert_eq!(waiting.await.unwrap(), None);

        // Rechargé depuis le disque : un autre bouton qui engage passe sans
        // carte, même à la voix ; un envoi de fichier demande toujours.
        let approvals = Approvals::load(path.clone());
        let (tx, mut rx) = mpsc::channel(8);
        assert!(authorize(&approvals, true, "mcp_call", &clic("Publier"), &tx, Duration::from_secs(5)).await.is_none());
        let envoi = json!({ "server": "navigateur", "tool": "browser_file_upload", "arguments": { "paths": [r"C:\x.pdf"] } });
        assert!(authorize(&approvals, true, "mcp_call", &envoi, &tx, Duration::from_secs(5)).await.is_some());
        drop(tx);
        assert!(rx.recv().await.is_none(), "aucune carte émise");

        // Retiré : la carte revient (refus vocal immédiat).
        assert!(approvals.revoke("navigateur : clic sur un élément qui engage"));
        let (tx, _rx) = mpsc::channel(8);
        assert!(authorize(&approvals, true, "mcp_call", &clic("Publier"), &tx, Duration::from_secs(5)).await.is_some());
        assert!(Approvals::load(path.clone()).always_list().is_empty(), "retrait enregistré");
        let _ = std::fs::remove_file(&path);
    }

    /// « Autoriser une fois » ne retient rien.
    #[tokio::test]
    async fn autoriser_une_fois_ne_retient_rien() {
        let approvals = std::sync::Arc::new(Approvals::default());
        let (tx, mut rx) = mpsc::channel(8);
        let waiting = {
            let approvals = approvals.clone();
            tokio::spawn(async move {
                authorize(&approvals, false, "run_command", &sensitive_call(), &tx, Duration::from_secs(5)).await
            })
        };
        let Some(AgentEvent::Approval { id, .. }) = rx.recv().await else { panic!("pas de carte") };
        assert!(approvals.respond(&id, true, false));
        assert_eq!(waiting.await.unwrap(), None);
        assert!(approvals.always_list().is_empty());
    }
}
