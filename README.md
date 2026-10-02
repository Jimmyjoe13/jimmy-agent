# JIMMY — agent IA desktop incarné

> **Statut : prototype personnel V1 — local**
> Un assistant agentique qui vit sur le bureau de Windows, incarné par un
> renard humanoïde 3D, et qui travaille **à la voix**.

Jimmy n'est pas un chatbot dans une fenêtre. C'est un personnage que l'on peut
déplacer sur son bureau, qui réagit à ce qu'il fait, et à qui l'on parle
principalement en disant « Jimmy ».

Tout tourne sur votre machine. Les seuls échanges réseau sont ceux, explicitement
nécessaires, vers les fournisseurs d'IA (OpenCode Go pour le modèle, Fish Audio
pour la voix). L'audio, lui, **ne quitte jamais l'ordinateur**.

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
| Communication Tauri ↔ Godot | ✅ HTTP local, 6 routes | tests `curl` sur 8787 |
| Chat texte + LLM | ✅ OpenCode Go, outils, réponse en français | `agent/tests/happy_path.rs` |
| **Loop agentique** | ✅ outils enchaînés, erreurs relues, itérations | test happy path (4 appels d'outils) |
| Outils fichiers | ✅ lister, lire, écrire, chercher | test happy path |
| Outil CLI | ✅ PowerShell, stdout/stderr, code de sortie | test happy path |
| Permissions | ✅ LECTURE / MODIFICATION / EXÉCUTION / RÉSEAU | 4 tests unitaires |
| Mémoire locale | ✅ vectorielle + FTS5, apprentissage auto | 3 tests unitaires + test happy path |
| Skills | ✅ création, lecture, amélioration, suggestion | 3 tests unitaires |
| Synaptiq | ✅ consultation conditionnelle, écriture | appelé dans le test happy path |
| Synaptiq — déclenchement | ✅ **décliné** sur demande courte, sans ré-interroger | 3 tests unitaires |
| Wake word « Jimmy » | ✅ détection locale, variantes ASR gérées | 5 tests unitaires |
| STT local | ✅ whisper.cpp, français, hors-ligne | WAV de test transcrit correctement |
| TTS Fish Audio | ✅ voix française, un appel par piste | appels réels à OpenRouter |
| Boucle vocale | ✅ wake → STT → agent → voix → avatar | `voice/listener.rs` |
| Profils graphiques | ✅ `low` / `medium` / `high` appliqués à Godot | logs `[godot/main]` |
| Diagnostic | ✅ 7 vérifications, chemins, secrets | vue Diagnostic |

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
│                     ┌─────────────┐      ┌──────────┐  ┌────────┐│
│                     │  OpenCodeGo │      │Fish Audio│  │Synaptiq││
│                     │  (LLM)      │      │  (voix)  │  │(local) ││
│                     └─────────────┘      └──────────┘  └────────┘│
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
pas une commodité. Le cœur de Jimmy est donc vérifiable en une commande, sans
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
│   │   ├── memory/         vecteurs, apprentissage, recherche
│   │   ├── providers/      LLM, TTS, STT, avatar
│   │   ├── skills/         création et amélioration de skills
│   │   ├── mcp/            client MCP (stdio)
│   │   ├── voice/          micro, wake word, boucle d'écoute
│   │   └── synaptiq.rs     client Synaptiq
│   ├── skills/             skills créés par Jimmy
│   └── tests/              test du chemin heureux
├── desktop/                application Tauri
│   ├── src/                interface TypeScript
│   └── src-tauri/          commandes, pont HTTP, fenêtre
├── godot/                  projet Godot (avatar)
│   ├── scripts/            personnage, animations, serveur HTTP
│   └── scenes/
├── data/                   données locales (SQLite, audio, composants)
├── scripts/                outils de développement et d'installation
├── .env.example
└── PLAN.md
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
| Synaptiq | local, API sur 8000 | ✅ |

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

---

## Lancement

```powershell
.\scripts\dev.ps1
```

Ou, pour la version installée :

```powershell
jimmy.exe
```

Au premier lancement, Jimmy :

1. démarre son avatar sur le bureau ;
2. affiche l'onboarding (nom, modèle, voix, qualité, langue) ;
3. se met en écoute du mot d'activation.

**Fermer la fenêtre ne tue pas Jimmy** : il reste sur le bureau avec son
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

# Optionnels
JIMMY_LLM_MODEL=space-bunny-free
JIMMY_TTS_VOICE=5567200c7d8341738f0892bbacd3be3c
JIMMY_STT_LANGUAGE=fr
SYNAPTIQ_API_URL=http://127.0.0.1:8000
SYNAPTIQ_API_KEY=...
JIMMY_LOG=info
```

`.env` est dans `.gitignore` et ne doit jamais être commité.

### Variables d'environnement lues par le code

| Variable | Défaut | Rôle |
|---|---|---|
| `OPENCODE_API_KEY` | — | authentification LLM (requis) |
| `OPENROUTER_API_KEY` | — | authentification voix (requis pour parler) |
| `SYNAPTIQ_API_KEY` | — | authentification Synaptiq |
| `JIMMY_GODOT_EXE` | auto-détecté | chemin de l'exécutable Godot |
| `JIMMY_GODOT_PORT` | 8787 | port du serveur HTTP de l'avatar |
| `JIMMY_BRIDGE_PORT` | 8790 | port du pont Tauri |
| `JIMMY_HOME` | auto | force le répertoire de données |

### Voix

Deux voix françaises sont proposées :

| Voix | Identifiant | Caractère |
|---|---|---|
| **Féminine** (défaut) | `5567200c7d8341738f0892bbacd3be3c` | calme, posée — validée à l'essai |
| **Clémence** | `a288bdc744da4ad194921adad6863175` | douce, claire |

> Le PLAN.md désigne « Clémence » ; le réglage effectivement validé sur cette
> machine (skill `synthese-vocale-fr`, 2 octobre 2026) est la voix « Féminine ».
> Les deux restent disponibles dans les paramètres.

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
outils : ["list_directory", "synaptiq_search", "read_file", "read_file"]
durée : 13951 ms
```

À la main, dans l'interface :

> « Jimmy, analyse ce dossier et explique-moi ce que tu trouves. »

Jimmy doit détecter « Jimmy », écouter, transcrire, analyser, utiliser ses
outils, répondre à la voix, animer l'avatar et afficher la réponse dans la
bulle.

### Tests automatisés

```powershell
.\scripts\with-msvc.ps1 cargo test     # 35 tests unitaires
```

---

## Documentation

| Document | Contenu |
|---|---|
| [`docs/architecture.md`](docs/architecture.md) | architecture détaillée, décisions et justifications |
| [`docs/development.md`](docs/development.md) | mise en route, commandes, conventions |
| [`docs/troubleshooting.md`](docs/troubleshooting.md) | pannes courantes et solutions |
| [`HANDOFF.md`](HANDOFF.md) | état d'avancement et prochaines étapes |

---

## Limites connues

Ces points sont assumés pour la V1 et documentés plutôt que masqués :

1. **Mémoire vectorielle par hachage.** Le moteur (`memory/embed.rs`) projette
   les termes dans 512 dimensions par hachage. C'est local, rapide et sans
   dépendance, mais ce n'est **pas** un modèle sémantique : « voiture » et
   « véhicule » ne se rapprochent pas. Le classement est compensé par une
   recherche FTS5 lexicale. Le trait `MemoryStore` est le seul point à
   réécrire pour adopter un vrai modèle d'embeddings.

2. **Skin unique.** L'architecture prévoit d'autres skins ; seul le renard est
   implémenté. Le chargeur lit `--skin=` et le chemin de chargement est isolé.

3. **Mise à jour automatique non branchée.** Il n'existe pas de source de
   distribution publique pour un prototype personnel. La commande
   `check_update` est écrite et branchée sur `JIMMY_UPDATE_ENDPOINT` ; sans
   cette variable, elle le dit clairement plutôt que de faire semblant.

4. **Pas d'export Godot.** L'avatar tourne depuis le projet en mode
   développement. L'export (gabarits ~1 Go) n'a pas été fait : le prototype
   fonctionne en l'état, l'installateur installera l'exécutable pré-compilé.

5. **MCP : stdit seule.** Le client MCP implémente le transport stdio, le plus
   répandu. Le transport HTTP streamable n'est pas implémenté.

6. **Latence du modèle STT.** `base-q5` (1,3 s) est le défaut pour le wake
   word ; `small-q5` (4,7 s) est plus précis sur le français. Les deux sont
   sélectionnables dans les paramètres.

---

## Licence

MIT — prototype personnel.