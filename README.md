# JIMY — agent IA desktop incarné

> **Statut : prototype personnel V1 — local**
> Un assistant agentique qui vit sur le bureau de Windows, incarné par un
> renard humanoïde 3D, et qui travaille **à la voix**.

Jimy n'est pas un chatbot dans une fenêtre. C'est un personnage que l'on peut
déplacer sur son bureau, qui réagit à ce qu'il fait, et à qui l'on parle
principalement en disant « Jimy ».

Tout tourne sur votre machine. Les seuls échanges réseau sont ceux, explicitement
nécessaires, vers les fournisseurs d'IA (OpenCode Go pour le modèle, Fish Audio
pour la voix), les serveurs MCP que vous branchez (dont la mémoire SynaptiQ) et
les sites que Jimy visite dans son navigateur. L'audio **ne quitte jamais
l'ordinateur** ; une capture de fenêtre ne part au modèle **que si vous la
demandez**.

---

## Sommaire

1. [Ce qui fonctionne aujourd'hui](#ce-qui-fonctionne-aujourdhui)
2. [Architecture](#architecture)
3. [Installation](#installation)
4. [Lancement](#lancement)
5. [Configuration](#configuration)
6. [Test du chemin heureux](#test-du-chemin-heureux)
7. [Documentation](#documentation)
8. [Limites connues](#limites-connues)

---

## Ce qui fonctionne aujourd'hui

Chaque ligne ci-dessous a été **exécutée et vérifiée** sur la machine cible
(Windows 11, Ryzen 7 5825U, 16 Go de RAM).

| Domaine | État | Vérifié par |
|---|---|---|
| Application Tauri | ✅ démarre, fenêtre, navigation, 8 vues | lancement + captures d'écran |
| Avatar Godot | ✅ renard humanoïde 3D, fenêtre transparente sur le bureau | captures d'écran |
| États de l'avatar | ✅ `idle`, `listening`, `thinking`, `speaking`, `executing`, `success`, `error`, `waiting` | `POST /state` + rendu |
| Bulle de dialogue | ✅ affichage, auto-masquage, parole synchronisée | `POST /say` + capture |
| Clic sur l'avatar | ✅ ouvre l'interface Tauri | pont HTTP 8790 |
| Communication Tauri ↔ Godot | ✅ HTTP local, 10 routes (dont `/helper`, `/snapshot`) | tests `curl` sur 8787 |
| Chat texte + LLM | ✅ OpenCode Go, outils, réponse en français, **écrite en direct** (flux SSE) | `agent/tests/happy_path.rs` |
| Formats d'API des modèles | ✅ Chat, Responses (Muse Spark, GPT, Grok) et Messages (Qwen 3.7+, MiniMax), choisis d'après le catalogue | `agent/tests/protocols.rs` (un vrai modèle par format) |
| Fichiers sensibles | ✅ toute **écriture** dans un `.env`, une clé, un secret attend ton accord (carte dans le Chat) ; la lecture reste libre | tests `sensitive` + suite d'interface |
| Tâches de fond | ✅ une tâche qui travaille encore 45 s après son premier outil passe en arrière-plan : Chat libre, voix qui réécoute, annonce de fin, « arrête tout » | `agent/tests/background.rs` + suite d'interface |
| Lapin des tâches de fond | ✅ un lapin travaille à côté du renard pendant la tâche, saute à la réussite, baisse les oreilles à l'échec | captures de l'écran réel (`scripts/photo-avatar.ps1`) |
| Vision | ✅ la fenêtre active est jointe à la demande (bouton « Joindre ma fenêtre » ou « regarde mon écran »), jamais à l'initiative du modèle | `--test protocols` (image), `--test screen` |
| Navigateur | ✅ son propre Chrome (Playwright MCP, profil gardé) ; accord demandé avant d'envoyer, payer, publier, supprimer | `--test browser` + tests `sensitive` |
| SynaptiQ | ✅ mémoire partagée entre agents, par MCP (identité `jimmy`) | serveur connecté, 9 outils |
| **Loop agentique** | ✅ outils enchaînés, erreurs relues, itérations | test happy path (4 appels d'outils) |
| Outils fichiers | ✅ lister, lire, écrire, chercher | test happy path |
| Outil CLI | ✅ PowerShell, stdout/stderr, code de sortie | test happy path |
| Permissions | ✅ LECTURE / MODIFICATION / EXÉCUTION / RÉSEAU | 4 tests unitaires |
| Mémoire locale | ✅ vectorielle (embeddings LM Studio, repli par hachage) + FTS5, apprentissage auto | tests unitaires + `--test memory_semantic` |
| Skills | ✅ création, lecture, amélioration, suggestion | 3 tests unitaires |
| Vault Obsidian | ✅ lecture plein texte du vault, écriture des souvenirs en notes | recherche réelle sur le vault `C:\Obsidian\Jimmy` |
| Vault — déclenchement | ✅ **décliné** sur demande courte, sans ré-interroger | 3 tests unitaires |
| Wake word « Jimy » | ✅ détection locale, variantes ASR gérées | 5 tests unitaires |
| Écoute permanente | ✅ micro ouvert, `whisper-server` démarré | `test audio` |
| STT local | ✅ whisper.cpp, français, hors-ligne | WAV de test transcrit correctement |
| TTS Fish Audio | ✅ voix française **lue** | `test audio` — PCM 44,1 kHz |
| Lecture audio | ✅ cpal, rééchantillonnage si la carte son diffère | `test audio` |
| Boucle vocale | ✅ wake → STT → agent → voix → avatar | `voice/listener.rs` |
| Profils graphiques | ✅ `low` / `medium` / `high` appliqués à Godot | logs `[godot/main]` |
| Diagnostic | ✅ 8 vérifications, chemins, secrets | vue Diagnostic |
| Interface | ✅ 28 parcours sur la vraie application (Chat, tâches de fond, capture, Voix, Skin, Paramètres, Historique…) | `scripts/test-ui.ps1` |

### Chiffres mesurés sur cette machine

| Mesure | Valeur |
|---|---|
| Wake word → agent | ~1,3 s de reconnaissance + latence LLM |
| Modèle STT `base-q5` (4,7 s d'audio) | 1,28 s |
| Modèle STT `small-q5` (4,7 s d'audio) | 4,7 s |
| Requête LLM seule (modèle gratuit) | 1 à 3 s |
| Demande complète avec 4 outils | ~14 s |
| Mémoire vectorielle, 1 souvenir | < 5 ms |

---

## Architecture

```text
┌─────────────────────────── WINDOWS ───────────────────────────┐
│                                                               │
│  ┌────────────────┐   agent-event    ┌────────────────────┐   │
│  │  Interface     │◄────────────────►│  TAURI             │   │
│  │  (TypeScript)  │    invoke()      │  fenêtre + pont   │   │
│  │  chat / voix   │                  └─────────┬──────────┘   │
│  │  memoire/skins │                            │              │
│  └────────────────┘                  ┌─────────▼──────────┐   │
│                                      │  crate jimmy-agent │   │
│                                      │  boucle agentique  │   │
│                                      │  outils · mémoire  │   │
│                                      │  skills · perms    │   │
│                                      └───┬───────┬─────┬───┘   │
│                                          │       │     │       │
│                            ┌─────────────┘       │     └────┐  │
│                            ▼                     ▼          ▼  │
│                     ┌─────────────┐      ┌──────────┐  ┌──────────────┐│
│                     │  OpenCodeGo │      │Fish Audio│  │Vault Obsidian││
│                     │  (LLM)      │      │  (voix)  │  │  (mémoire)   ││
│                     └─────────────┘      └──────────┘  └──────────────┘│
│                     ┌─────────────┐      ┌──────────┐            │
│                     │ whisper.cpp │      │  cpal    │            │
│                     │ (STT local) │      │ (micro)  │            │
│                     └─────────────┘      └──────────┘            │
│                     ┌─────────────────────────────────┐        │
│                     │  GODOT — avatar 3D               │        │
│                     │  serveur HTTP local :8787        │        │
│                     └─────────────────────────────────┘        │
└───────────────────────────────────────────────────────────────┘
```

**Séparation des responsabilités, stricte :**

- **Tauri** : interface, fenêtre, agent, outils, mémoire, configuration.
- **`jimmy-agent`** (crate Rust) : *toute* la logique, sans aucune dépendance à
  Tauri. C'est ce qui rend le cœur testable avec `cargo test` sans démarrer
  d'application.
- **Godot** : rendu 3D et animations uniquement. Il ne sait rien du LLM, de la
  mémoire ni des outils ; il ne fait qu'exécuter des ordres d'animation reçus
  en HTTP.

Le crate `jimmy-agent` ne dépend pas de Tauri : c'est un choix d'architecture,
pas une commodité. Le cœur de Jimy est donc vérifiable en une commande, sans
fenêtre.

### Arborescence

```text
jimmy-agent-personnel/
├── Cargo.toml              workspace Rust
├── docs/                   documentation détaillée
├── agent/                  crate jimmy-agent — le cœur
│   ├── src/
│   │   ├── core/           boucle agentique, historique, prompt
│   │   ├── tools/          fichiers, CLI, réseau, mémoire, skills
│   │   ├── memory/         vecteurs, apprentissage, recherche, vault Obsidian
│   │   ├── providers/      LLM, TTS, STT, avatar
│   │   ├── skills/         création et amélioration de skills
│   │   ├── mcp/            client MCP (stdio)
│   │   └── voice/          micro, wake word, boucle d'écoute
│   ├── skills/             skills créés par Jimy
│   └── tests/              test du chemin heureux
├── desktop/                application Tauri
│   ├── src/                interface TypeScript
│   └── src-tauri/          commandes, pont HTTP, fenêtre
├── godot/                  projet Godot (avatar)
│   ├── scripts/            personnage, animations, serveur HTTP
│   └── scenes/
├── data/                   données locales (SQLite, audio, composants)
├── scripts/                outils de développement et d'installation
└── .env.example
```

---

## Installation

### Prérequis vérifiés sur la machine

| Composant | Version | Statut |
|---|---|---|
| Windows | 11 | ✅ |
| Node.js | 24.x | ✅ |
| Rust | 1.99 | ✅ |
| Visual Studio Build Tools | 2022 (C++) | ✅ |
| WebView2 | 154 | ✅ |
| Godot | 4.5.1 stable | ✅ |
| Vault Obsidian | dossier des notes (par défaut `C:\Obsidian\Jimmy`) | ✅ |

L'installateur vérifie tout cela et indique précisément ce qui manque :

```powershell
.\scripts\install.ps1
```

Il peut installer automatiquement les composants manquants (avec votre accord,
car l'installation de Visual Studio Build Tools demande les droits
d'administrateur).

### Installation manuelle

```powershell
# 1. Rust + Build Tools (si absents)
winget install Microsoft.VisualStudio.2022.BuildTools `
  --override "--quiet --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
# rustup : https://rustup.rs

# 2. Dépendances front
cd desktop; npm install; cd ..

# 3. Composants locaux (whisper.cpp + modèle)
.\scripts\install.ps1 -SkipBuildTools
```

### Composants locaux

`data/components/whisper/` contient :

- `Release/whisper-server.exe` — binaire whisper.cpp (8 Mo)
- `models/ggml-base-q5_1.bin` — modèle par défaut, bon compromis (57 Mo)
- `models/ggml-small-q5_1.bin` — modèle plus précis, français impeccable (181 Mo)
- `models/ggml-silero-v6.2.0.bin` — détection de la parole

### Installeur Windows (NSIS)

```powershell
.\scripts\build.ps1 -Release -Bundles
```

Produit un installeur dans `target/release/bundle/nsis/` (mode
utilisateur courant : pas de droits administrateur). Limites connues,
lues dans `tauri.conf.json` et `paths.rs` — **non vérifié de bout en
bout** :

- l'installeur n'embarque ni `godot/`, ni whisper, ni les skills :
  l'avatar et la voix y sont indisponibles ;
- les données partent dans `%APPDATA%\Jimy` (base, config et logs
  frais, sans vos réglages) ;
- **pas de mise à jour automatique** depuis un installeur : `/update`
  exige un clone git (voir ci-dessous).

Pour un Jimy complet (avatar, voix, mises à jour), passez par les
sources (`git clone` + `install.ps1`).

### Deuxième PC (depuis les sources)

```powershell
git clone https://github.com/Jimmyjoe13/jimmy-agent.git
cd jimmy-agent
.\scripts\install.ps1
```

Puis : clés dans l'interface (Paramètres → LLM) ou `.env` recréé
depuis `.env.example`, et `.\scripts\shortcut.ps1` pour le raccourci.
**Ne recopiez pas `data/`** (base, journaux, extraits audio et chemins
de l'autre machine) ; les modèles whisper sont retéléchargés par
`install.ps1`. SynaptiQ reste optionnel (tunnel SSH vers le serveur,
voir HANDOFF) : sans lui, Jimy travaille avec le vault seul.

---

## Lancement

### Le plus simple : le raccourci Bureau

Un raccourci **Jimy** est posé sur le bureau. Un double-clic lance
l'application avec son avatar, sans fenêtre de console et sans compilation.

Pour le (re)créer, ou gérer le démarrage automatique :

```powershell
.\scripts\shortcut.ps1                # crée le raccourci Bureau
.\scripts\shortcut.ps1 -Autostart     # ajoute le démarrage avec Windows
.\scripts\shortcut.ps1 -Remove        # supprime le raccourci
```

Le raccourci pointe sur `scripts/launcher.ps1`, qui lance le binaire release
(`target/release/jimmy.exe`). Si ce binaire n'existe pas encore, il propose de
le compiler plutôt que d'échouer en silence.

> Le binaire de **debug** n'est pas un repli possible : en développement il
> attend un serveur Vite, que le raccourci ne lance pas.

### En développement

```powershell
.\scripts\dev.ps1
```

Au premier lancement, Jimy :

1. démarre son avatar sur le bureau ;
2. affiche l'onboarding (nom, modèle, voix, qualité, langue) ;
3. se met en écoute du mot d'activation.

**Fermer la fenêtre ne tue pas Jimy** : il reste sur le bureau avec son
avatar. Pour le quitter, fermez le processus ou utilisez la zone de
notification.

---

## Configuration

Tout se règle depuis **Paramètres**, dans l'interface. Le fichier `.env` ne
contient que les secrets.

### `.env`

```bash
# Requis
OPENCODE_API_KEY=sk-...          # OpenCode Go
OPENROUTER_API_KEY=sk-or-v1-...  # Fish Audio (voix)

# Optionnels (valeurs de départ : ensuite, tout se choisit dans l'interface)
JIMMY_LLM_MODEL=space-bunny-free
JIMMY_TTS_VOICE=4f2a0684dd0247dda68f339738c780e6
JIMMY_STT_LANGUAGE=fr
JIMMY_LOG=info
```

`.env` est dans `.gitignore` et ne doit jamais être commité. Le modèle et la
voix ne sont lus dans `.env` qu'au premier lancement : ensuite, le choix fait
dans **Paramètres** (bibliothèque de modèles) et **Voix** est conservé.

### Variables d'environnement lues par le code

| Variable | Défaut | Rôle |
|---|---|---|
| `OPENCODE_API_KEY` | — | authentification LLM (requis) |
| `OPENROUTER_API_KEY` | — | authentification voix (requis pour parler) |
| `JIMMY_VAULT_PATH` | `C:\Obsidian\Jimmy` | racine du vault Obsidian |
| `JIMMY_GODOT_EXE` | auto-détecté | chemin de l'exécutable Godot |
| `JIMMY_GODOT_PORT` | 8787 | port du serveur HTTP de l'avatar |
| `JIMMY_BRIDGE_PORT` | 8790 | port du pont Tauri |
| `JIMMY_HOME` | auto | force le répertoire de données |

### Voix

La voix se choisit dans la vue **Voix** : bibliothèque du catalogue Fish Audio,
avec recherche et écoute avant choix. Voix de départ :

| Voix | Identifiant | Caractère |
|---|---|---|
| **Le narrateur** (défaut, choisi le 4 octobre) | `4f2a0684dd0247dda68f339738c780e6` | grave, posée |
| **Féminine** | `5567200c7d8341738f0892bbacd3be3c` | calme, posée |
| **Clémence** | `a288bdc744da4ad194921adad6863175` | douce, claire |

> Le PLAN.md désignait « Clémence » ; « Féminine » a été la voix par défaut
> jusqu'au 4 octobre, puis « Le narrateur ».

**L'écoute permanente** se pilote depuis la vue **Voix** — bouton « Activer
l'écoute » — et s'active automatiquement à la fin de l'onboarding. Tant qu'elle
n'est pas active, Jimy n'ouvre pas le micro et n'entend rien : c'est un choix,
le micro ne s'active jamais sans que vous l'ayez demandé.

> Fish Audio ne produit pas de WAV (`wav` renvoie 400). Jimy demande donc du
> **PCM 16 bits** : le flux arrive déjà dans le format que la carte son consomme,
> ce qui évite d'embarquer un décodeur MP3.

### Permissions

Trois capacités, configurables indépendamment : **lecture**, **modification**,
**exécution** (plus réseau). Une capacité accordée s'exerce sans redemander,
comme demandé dans le PLAN. Chaque règle porte ses propres listes de chemins et
de commandes autorisées — la structure est déjà prête pour un raffinement par
outil ou par service.

---

## Test du chemin heureux

```powershell
.\scripts\test-happy.ps1
```

Ce test lance un vrai agent contre un vrai modèle, sur un dossier temporaire, et
vérifie qu'il utilise bien un outil au lieu de deviner. Dernier résultat réel :

```text
--- réponse ---
Le dossier contient deux petits fichiers texte.

**todo.md** — une liste de tâches au format Markdown, avec deux lignes. La
première est une tâche à faire, la deuxième est marquée comme terminée...

états : [Thinking, Executing, Executing, Speaking]
outils : ["list_directory", "vault_search", "read_file", "read_file"]
durée : 13951 ms
```

Le même script enchaîne sur la **chaîne audio** (`.\scripts\test-happy.ps1 -SkipAudio`
pour l'omettre), deux tests qui ouvrent de vrais périphériques :

| Test | Ce qu'il vérifie | Dernier résultat |
|---|---|---|
| `tts_synthetise_et_se_lit` | appel Fish Audio, décodage, **lecture réelle** | 225 280 octets, 44 100 Hz, 4,3 s |
| `ecoute_permanente_demarre` | micro ouvert + `whisper-server` lancé | micro ouvert, Whisper prêt |

Isolément :

```powershell
.\scripts\with-msvc.ps1 cmd /c "cargo test -p jimmy-agent --test audio -- --ignored --nocapture tts"
.\scripts\with-msvc.ps1 cmd /c "cargo test -p jimmy-agent --test audio -- --ignored --nocapture ecoute"
```

À la main, dans l'interface :

> « Jimy, analyse ce dossier et explique-moi ce que tu trouves. »

Jimy doit détecter « Jimy », écouter, transcrire, analyser, utiliser ses
outils, répondre à la voix, animer l'avatar et afficher la réponse dans la
bulle.

**L'écoute doit être active** (bouton en tête de la vue **Voix**, ou
automatiquement après l'onboarding). Sinon Jimy reste muet : c'est voulu.

### Tests automatisés

```powershell
.\scripts\with-msvc.ps1 cargo test --workspace   # 151 tests unitaires (6 octobre), 0 avertissement
.\scripts\test-ui.ps1                            # 26 parcours sur la vraie application
```

Les tests qui consultent de vrais services sont marqués `#[ignore]` et se
lancent explicitement (le `'--'` entre guillemets : PowerShell avale le `--`
nu), par exemple les trois formats d'API des modèles :

```powershell
.\scripts\with-msvc.ps1 cargo test -p jimmy-agent --test protocols '--' --ignored --nocapture
```

---

## Documentation

| Document | Contenu |
|---|---|
| [`docs/architecture.md`](docs/architecture.md) | architecture détaillée, décisions et justifications |
| [`docs/development.md`](docs/development.md) | mise en route, commandes, conventions |
| [`docs/troubleshooting.md`](docs/troubleshooting.md) | pannes courantes et solutions |

---

## Limites connues

Ces points sont assumés pour la V1 et documentés plutôt que masqués :

1. **Mémoire sémantique dépendante de LM Studio.** Les embeddings viennent de
   LM Studio (`memory/semantic.rs`) ; s'il est éteint, Jimy retombe sur le
   hachage 512 dimensions (`memory/embed.rs`), local mais **non** sémantique
   (« voiture » et « véhicule » ne se rapprochent pas), compensé par la
   recherche lexicale FTS5.

2. **Skins procéduraux.** Cinq skins (renard, arctique, fennec, ours, robot),
   construits par primitives Godot : aucun modèle 3D externe.

3. **Fichiers sensibles : détection par motifs.** Le garde-fou repère les
   écritures dans les `.env`, clés et secrets d'après la commande ; un script
   intermédiaire qui ne nomme pas le fichier y échappe. Ce n'est pas un bac à
   sable.

4. **Mise à jour depuis un installeur.** Le mécanisme de mise à jour
   (pastille + `/update` : `pull` fast-forward, recompilation,
   redémarrage) exige un clone git : ni le zip GitHub ni l'installeur
   NSIS ne peuvent s'auto-mettre à jour. Sans `.git`, la vérification
   se tait. L'ancienne commande `check_update` (source
   `JIMMY_UPDATE_ENDPOINT`, jamais configurée ni appelée) ne sert plus.

5. **Pas d'export Godot.** L'avatar tourne depuis le projet en mode
   développement. L'export (gabarits ~1 Go) n'a pas été fait : le prototype
   fonctionne en l'état, l'installateur installera l'exécutable pré-compilé.

6. **MCP : stdio seule.** Le client MCP implémente le transport stdio, le plus
   répandu. Le transport HTTP streamable n'est pas implémenté.

7. **Navigateur : accord d'après la description.** La carte « action en ton
   nom » se fonde sur la description de l'élément donnée par le modèle : un
   bouton mal décrit y échappe. Une page dont l'instantané dépasse 12 000
   caractères est tronquée.

8. **Tâches de fond en mémoire.** Une seule à la fois ; un redémarrage de
   Jimy perd la tâche en cours (l'historique garde ce qui a été fait).

9. **Latence du modèle STT.** `base-q5` (1,3 s) est le défaut pour le wake
   word ; `small-q5` (4,7 s) est plus précis sur le français. Les deux sont
   sélectionnables dans les paramètres.

---

## Licence

MIT — voir [LICENSE](LICENSE). Prototype personnel.