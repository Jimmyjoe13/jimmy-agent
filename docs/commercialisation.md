# Jimmy — Plan de commercialisation (audit + roadmap)

> Statut du document : **plan uniquement, aucune modification de code**.
> Rédigé le 08/10/2026 à partir d'un audit lecture-seule du dépôt.
> Hypothèses validées : **open source + contenus additionnels payants** (skins,
> fonctionnalités), cible prioritaire **grand public**.
> Référence stratégie d'origine : `PLAN.md` §6 (« commercialisation traitée
> après validation », skins/marketplace/abonnement explicitement hors V1).

---

## 1. Où en est Jimmy (vérifié, pas d'impression)

| Domaine | État réel | Preuve |
|---|---|---|
| Produit | Prototype personnel avancé, Windows local, avatar + voix + agent + navigateur + vision + tâches de fond | `HANDOFF.md` « Ce qui est vérifié », `README.md` |
| Tests | ~194 unitaires annoncés, 14 fichiers d'intégration, suite UI 28/29 | `agent/tests/`, `scripts/test-ui.ps1`, `scripts/ui-test/suite.js` |
| Architecture | Saine : `jimmy-agent` sans Tauri, Godot = rendu seul, HTTP local | `agent/src/lib.rs:1-7`, `docs/architecture.md` |
| Docs | Très denses (HANDOFF ~92 pièges, JOURNAL, 3 docs) mais chiffres parfois périmés | `docs/development.md:138` fige « 151 tests / 26 parcours » |
| Dépôt | Public MIT depuis le 06/10, `main` propre, 0 commit non poussé | `LICENSE`, `INSTRUCTIONS.md:116` |
| CI/lint | Aucune (ni GitHub Actions, ni clippy bloquant, ni pre-commit) | absence `.github/`, `.pre-commit*` |

Conclusion : **excellent prototype personnel, pas encore un produit
distribuable au grand public**. L'écart n'est pas la qualité du cœur, c'est
tout ce qu'un particulier attend : installateur 1-clic, pas de clé à
bidouiller, sécurité par défaut, mises à jour, support.

---

## 2. Scores honnêtes (échelle audit : 9 = commercialisable)

| Dimension | Score | Pourquoi (une ligne) |
|---|---|---|
| Sécurité grand public | 3/10 | Permissions tout-ouvert par défaut, `run_command` sans sandbox, secrets en clair (logs/DB/config) |
| Confidentialité / RGPD | 4/10 | Local-first réel, mais sous-traitance LLM/TTS + historique non chiffré + consentements implicites |
| Tests / qualité | 7/10 | Couverture réelle et tests sur vrais services, mais 228 `unwrap/expect`, erreurs parfois avalées, TS sans tests |
| Ingénierie (CI, build) | 3/10 | Procédure locale solide (`build.ps1`, `test-ui.ps1`) mais rien d'automatique ni reproductible |
| Distribuabilité | 2/10 | Pas d'installateur signé, Godot en mode dev obligatoire, binaires whisper hors git, clés requises |
| Documentation produit | 6/10 | Excellente pour un dev, insuffisante pour un particulier (pas de parcours « sans clé », pas de FAQ coûts) |
| Monétisation-readiness | 2/10 | MIT partout (même les skins), pas de store, pas de billing, pas de mise à jour signée |

---

## 3. P0 — bloquant avant toute diffusion grand public

Chaque point = à traiter avant de mettre un installateur entre des mains
non-techniques. Estimations entre parenthèses.

### 3.1 Secrets : ne plus fuiter les clés (1–2 j)

- Les clés `aggregate` et SynaptiQ figurent **en clair dans `data/logs/jimmy.log`**
  (arguments d'outils journalisés tels quels) — admis par écrit, jamais corrigé.
  Voir `INSTRUCTIONS.md:122-124`, `HANDOFF.md:1564-1567`, cause
  `agent/src/core/agent.rs:424-430` (log de `call.arguments` en clair).
- Clés LLM et `env` des serveurs MCP persistées en clair dans
  `data/config.json` (`agent/src/config.rs:242-255`, `agent/src/tools/mcp.rs:225-232`).
- Résultats d'outils (dont lectures de `.env`) stockés en clair dans
  `data/jimmy.db` (`agent/src/core/history.rs`, `agent/src/db.rs:24-28`).
- À faire : **rotation** des clés exposées (`OPENCODE`, `OPENROUTER`,
  `SYNAPTIQ`, `aggregate`), **masquage centralisé** avant journalisation
  (`KEY=`, `TOKEN=`, `SECRET=`, `sk-…`), **coffre OS** (DPAPI/keyring) au lieu
  du `.env`/JSON en clair, chiffrement `jimmy.db` + `config.json`.
- Compléter `.gitignore` (`.playwright-mcp/`, `JIMMY_LLM_DUMP/`, `requete-*.json`)
  + scan de secrets en pre-commit (ex. gitleaks) : le dépôt est public,
  une fausse manip est irréversible.

### 3.2 Permissions : passer en refus par défaut (2–3 j)

- `AccessRule::default()` : `granted: true`, `allow_commands: [*]`,
  réseau sans restriction d'hôte (`agent/src/permissions.rs:56-107`).
  Le modèle peut donc exécuter des commandes et POSTer des secrets lus vers
  n'importe quel hôte **sans accord**.
- `sensitive::authorize` n'est appelé qu'en un point
  (`agent/src/core/agent.rs:438-446`), ne couvre que l'**écriture** sensible,
  la lecture restant volontairement libre (test `sensitive.rs:833`).
  Aveu en code : « détection par motifs, pas un bac à sable »
  (`agent/src/sensitive.rs:17-19`).
- À faire : profil **grand public = lecture seule par défaut**, onboarding
  explicite pour exécution/réseau, `http_request` et `mcp_add_server` à accord
  explicite systématique, centraliser l'autorisation (aucun outil exécutable
  sans passer par `authorize`).

### 3.3 Installateur et dépendances : le « 1-clic » n'existe pas (1–2 sem)

- **Godot obligatoire en mode dev** : l'avatar exige `Godot.exe` + le projet
  `godot/` (`agent/src/lib.rs:728-764`, résolution `agent/src/paths.rs:156-216`).
  Pas d'export Godot (gabarits ~1 Go non faits, `README.md:420-422`).
  Sans Godot : avatar mort.
- **Whisper hors git** (`data/components/` ignoré) : téléchargé par
  `scripts/install.ps1:144-183` (8 + 57 + 181 Mo) + ports 8178/8179.
  Antivirus/réseau client = voix KO.
- **Clés obligatoires** : sans `OPENCODE_API_KEY` + `OPENROUTER_API_KEY`, chat
  et voix échouent (`agent/src/config.rs:792-800`, diagnostic
  `desktop/src-tauri/src/commands.rs:1068-1077`). Impossible pour du grand
  public de « saisir deux clés API ».
- Tauri : `bundle targets=[nsis] installMode=currentUser`
  (`desktop/src-tauri/tauri.conf.json:31-44`), **pas d'updater, pas de
  signature**.
- À faire : export Godot embarqué OU avatar désactivable proprement, whisper
  embarqué ou téléchargé au premier lancement avec progress, **offre sans clé**
  (clé incluse avec quotas — voir §5), installeur NSIS **signé**, `check_update`
  signé ou retiré du discours (`desktop/src-tauri/src/commands.rs:1137-1170`).

### 3.4 Ports locaux sans authentification (0,5 j)

- Godot `127.0.0.1:8787` et pont Tauri `127.0.0.1:8790` : aucune auth
  (`godot/scripts/http_server.gd:31-32`, `desktop/src-tauri/src/bridge.rs:19-39`).
  Tout processus local peut piloter l'avatar ou forcer l'affichage.
- À faire : token local aléatoire (fichier à permissions restreintes) sur les
  deux, devtools/CDP coupés en release
  (`capabilities/default.json:16`, `scripts/test-ui.ps1:34` = test-only à garder
  tel quel).

### 3.5 Juridique open source + contenus payants (avec conseil, 1 sem)

- **Licence actuelle : MIT partout**, y compris skins et assets
  (`LICENSE`, `Cargo.toml:8`). MIT = cualquiera peut forker **et revendre**,
  skins comprises. Pour vendre des skins/fonctionnalités, il faut **séparer** :
  code sous MIT (ou autre OSI) + **contenus sous licence propriétaire**
  (dossier `assets-premium/` ou dépôt séparé, jamais sous MIT).
- **Abonnement Claude via signature Claude Code** (`agent/src/providers/llm.rs`,
  `claude_plan`, pièges 90–92 : mapping de noms d'outils + bloc de facturation
  pour passer le bridage Anthropic) : ne **jamais commercialiser tel quel**
  sans validation écrite du ToS Anthropic — risque de coupure et de grief.
- CGU/TTS/LLM : Fish Audio, OpenCode Go, Playwright/Chromium, whisper.cpp,
  Godot — vérifier redistribution + mention sous-traitance dans CGU/consentement.
- Marque : « Jimmy » est générique + wake word — recherche d'antériorité avant
  logo/store.

### 3.6 Coûts et modèle sans clé (décision business, pas de code)

- Aujourd'hui : l'utilisateur paie **deux abonnements tiers** (LLM + voix) +
  latence modèle gratuit mesurée **2–25 s** (non corrigeable côté Jimmy).
- Grand public = **prix unique ou abonnement Jimmy**, quotas inclus, BYOK en
  option avancée. Sans ça, le support croulera sous « ça ne répond pas ».
- Prévoir : page coûts transparente, mode dégradé offline (STT local OK, LLM
  KO → message clair), pas de promesse de gratuité illimitée sur modèle
  `free:free`.

---

## 4. P1 — qualité et ingénierie (avant ou pendant la bêta fermée)

1. **Robustesse Rust** (2–3 j) : 146 `unwrap` + 15 `expect` dans `agent/src`,
   dont `Mutex::lock().unwrap()` en production
   (`agent/src/core/history.rs:40-58`, `agent/src/memory/mod.rs:136-151`) et un
   `expect` sur entrée externe (`desktop/src-tauri/src/commands.rs:1207`).
   Remplacer par erreurs typées (`agent/src/error.rs`), sans `anyhow` mort
   (`Cargo.toml:20` déclaré, 0 usage).
2. **Erreurs silencieuses** (1 j) : `let _ = history.append`,
   `events.send(Final/Failed)` ignorés (`agent/src/lib.rs:525-533`),
   état de revue perdu (`agent/src/growth.rs:59`). Compter + journaliser.
3. **CI minimale** (1 j) : GitHub Actions `fmt --check` + `clippy -D warnings`
   + `cargo test --workspace` + `npm run build`. Aujourd'hui tout est manuel.
4. **Docs produit** (1 j) : `docs/development.md:138` périmé (151 tests/26
   parcours), `tests/` racine vide à supprimer, écrire le parcours
   « sans clé » + FAQ coûts + limites assumées (`README.md` § Limites).
5. **Données locales** (2 j) : TTL + purge historique/audio debug, export +
   suppression de compte (RGPD), consentement explicite capture fenêtre
   (déjà « sur demande seulement », `agent/src/screen.rs` — le formaliser).

---

## 5. P2 — monétisation (ton modèle : base open source + add-ons)

### Ce qui se vend bien avec cette architecture

| Produit | Pourquoi ça marche ici | Technique requise |
|---|---|---|
| **Packs de skins** (ex. « Bestiaire », « Métal », saisons) | Système déjà paramétrique : palette + `tail_scale`/`metallic`/`has_fur`, 5 skins sans toucher au TS | Dossier contenus propriétaire + signature + boutique in-app + prévisualisation |
| **Voix premium** (voix FR exclusives, styles) | Bibliothèque Fish Audio déjà branchée, « Le narrateur » par défaut | Catalogue privé + licence voix + cache `data/audio/cues/` |
| **Packs de skills** (ex. « Productivité », « Créateurs », « Étudiants ») | Capture/curation déjà existante (`skills/capture.rs`, revue) | Store de skills signés, versionnés, désinstallables |
| **Fonctions premium** (tâches planifiées, multi-projets, historique illimité) | Tâches de fond mono-instance aujourd'hui (`README.md` limite 8) | Feature-flags + licence locale vérifiable offline |
| **Support prioritaire / installateur assisté** | Cible grand public, installation lourde (VS Build Tools, Godot, whisper) | Offre + canal support + diagnostic exportable |
| **À éviter** : quotas LLM opaques, pub, revente de données, cloud imposé | Casserait la promesse local-first du `README.md:11-16` | — |

### Pré-requis boutique (2–4 sem, après P0)

- Séparation physique code MIT / contenus propriétaires + licence contenus.
- Signature des contenus (hash + clé), feature-flags liés à la licence.
- Billing : prestataire externe (Stripe/Paddle/Lemon Squeezy) — **jamais de
  CB dans l'app**, pas de PCI maison. Facturation UE (TVA) via le prestataire.
- Mise à jour signée (Tauri updater + minisign) — `docs/troubleshooting.md:39-43`
  rappelle la clé requise.
- Télémétrie **opt-in** + crash reports + diagnostic exportable (base du support
  grand public). Aujourd'hui : logs locaux verbeux incluant des secrets (§3.1).

---

## 6. Roadmap proposée

| Phase | Contenu | Durée indicative |
|---|---|---|
| **Phase A — Stopper les fuites** | Rotation clés, masquage logs, `.gitignore` + pre-commit, token ports locaux | ~1 sem |
| **Phase B — Bêta fermée sûre** | Permissions deny-by-default, onboarding sans jargon, CI, installateur non signé à testeurs connus, CGU bêta | ~2–3 sem |
| **Phase C — 1-clic** | Export Godot embarqué, whisper au premier lancement, offre sans clé (quotas), NSIS signé, update signé | ~3–6 sem |
| **Phase D — Boutique** | Licence contenus, 1er pack skins payant + 1 pack skills, billing, télémétrie opt-in, support | ~4–8 sem |

Ordre non négociable : **A avant tout envoi à un tiers** (les logs actuels
contiennent des clés), B avant bêta, C avant grand public, D en dernier.

---

## 7. Les 3 prochains pas (si tu valides)

1. **Rotation + masquage secrets** (~4 h) : révoquer/régénérer les 4 clés,
   masquage centralisé avant log, `.gitignore` + pre-commit gitleaks.
2. **Décision offre sans clé** (business, 0 code) : qui paie le LLM/TTS ?
   Tant que ce n'est pas tranché, ne pas polir la boutique.
3. **Séparation licence contenus** (0,5 j, que du rangement) : créer
   l'emplacement propriétaire + y déplacer les futurs skins payants, garder le
   socle MIT intact.

---

## Annexe — sources

- Sub-audits lecture-seule du 08/10/2026 (sécurité, architecture/distribution,
  qualité/tests) + `INSTRUCTIONS.md`, `CONTRIBUTION.md`, `HANDOFF.md`,
  `JOURNAL.md`, `README.md`, `PLAN.md`, `LICENSE`, `Cargo.toml`,
  `.env.example`, `desktop/src-tauri/tauri.conf.json`.
- Mémoire long-terme (SynaptiQ) indisponible au moment de l'audit
  (API `127.0.0.1:8000` injoignable) : décision « open source + add-ons, cible
  grand public » à y consigner quand le service sera relancé.
- Ce document est un **plan** : aucun code, config ou donnée n'a été modifié
  pour le produire.
