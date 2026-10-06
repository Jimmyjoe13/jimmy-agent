# Architecture

Ce document explique **pourquoi** Jimmy est construit ainsi. Le PLAN.md décrit
*quoi* construire ; ici, on explique *pourquoi de cette façon*.

---

## 1. Trois processus, trois responsabilités

Le principe directeur : **un composant, une responsabilité**. Aucun composant
ne sait ce que font les autres.

```text
Tauri (fenêtre + agent)   ──HTTP──▶   Godot (avatar)
        │
        ├──HTTP──▶  OpenCode Go   (LLM)
        ├──HTTP──▶  Fish Audio   (voix)
        ├──lecture/écriture──▶  Vault Obsidian (mémoire persistante, local)
        └──sous-processus──▶  whisper.cpp (STT local)
```

| Composant | Sait | Ne sait pas |
|---|---|---|
| Tauri / `jimmy-agent` | tout du reste | — |
| Godot | qu'on lui demande de changer d'animation | ce qu'est un LLM |
| Vault Obsidian | ce qu'il contient | que Jimmy existe |

**Pourquoi cette séparation ?** Le PLAN identifie le risque n° 2 : « deux
runtimes doivent fonctionner ensemble ». La réponse n'est pas d'éviter le
problème, c'est de rendre la frontière impossible à ambiguëter. Godot ne peut pas
accéder au disque, au réseau ni au LLM : il ne peut donc pas être la source
d'un bug non traçable dans l'agent.

### Le crate `jimmy-agent` ne dépend pas de Tauri

C'est un choix structurant. Toute la logique — boucle agentique, outils,
mémoire, permissions, voix, vault Obsidian — vit dans un crate Rust ordinaire,
sans une seule ligne liée à Tauri.

Conséquences concrètes :

- `cargo test` vérifie le cœur de Jimmy **sans démarrer d'interface** ;
- le crate `desktop/src-tauri` se réduit à l'adaptation Tauri ↔ agent ;
- si l'interface change, la logique ne bouge pas.

Le test `agent/tests/happy_path.rs` le démontre : il instancie l'application,
lance un vrai agent, et vérifie le résultat sans fenêtre.

---

## 2. Communication Tauri ↔ Godot

### HTTP local, dans un seul sens

Tauri **envoie**, Godot **reçoit**. Godot expose un petit serveur HTTP et n'a
jamais besoin de savoir qui l'appelle.

```text
POST http://127.0.0.1:8787/state     {"state": "thinking"}
POST http://127.0.0.1:8787/say       {"text": "...", "duration_ms": 1200}
POST http://127.0.0.1:8787/quality   {"level": "high"}
POST http://127.0.0.1:8787/skin      {"skin": "renard"}
POST http://127.0.0.1:8787/position  {"x": 120, "y": 240}
GET  http://127.0.0.1:8787/health
```

Le PLAN propose `POST /state` ; c'est exactement ce qui a été fait, étendu à
cinq autres routes qui évitent d'autres channels.

### Pourquoi un serveur HTTP écrit à la main dans Godot

Godot n'embarque pas de serveur HTTP. Plutôt que d'ajouter une dépendance GDExtension
à compiler pour la V1, `godot/scripts/http_server.gd` implémente le nécessaire
sur `TCPServer` : lecture de la requête, extraction de `Content-Length`, réponse
`200`. Une centaine de lignes, aucune dépendance, et le protocole reste lisible.

### Le pont inverse

Un seul cas exige l'autre sens : « l'utilisateur a cliqué sur Jimmy, ouvre
l'interface ». `desktop/src-tauri/src/bridge.rs` expose donc
`POST /ui/open` sur `127.0.0.1:8790`, écouté uniquement en local.

C'est un deuxième mini-serveur, mais il évite un canal bidirectionnel
complet. **Un seul mécanisme, une seule chose à déboguer** : c'est le critère
retenu.

---

## 3. Boucle agentique

```text
demande
  │
  ├─▶ mémoire locale      (souvenirs pertinents)
  ├─▶ Vault Obsidian      (notes, uniquement si la demande référence un contexte)
  │
  ▼
LLM + outils  ──────────────────────────┐
  │                                     │
  ├─ pas d'appel d'outil ? ──▶ réponse finale
  │                                     │
  └─ appel d'outil ──▶ permission ──▶ exécution
                              │
                    refus ────┘ (message renvoyé au modèle)
```

Trois garde-fous contre les boucles coûteuses :

1. **Budget d'itérations** par demande (`llm.max_iterations`, 12 par défaut) ;
2. **Mémoire d'appels** : rappeler le même outil avec les mêmes arguments
   renvoie le premier résultat et prévient le modèle, au lieu de recalculer ;
3. **Borne de temps** globale (10 minutes).

### Les erreurs d'outil ne sont pas des erreurs dJimmy

Quand un outil échoue, le message d'erreur est renvoyé au modèle comme s'il
s'agissait d'un résultat. Le modèle peut alors se corriger — corriger une
commande, changer d'argument, abandonner. Seules les erreurs *structurelles*
(fatal LLM, provider indisponible) remontent comme `AgentEvent::Failed`.

### Assemblage du prompt

Le prompt système est reconstruit **à chaque tour**, à partir de ce que Jimmy
sait au moment précis : identité, outils disponibles, catalogue de skills,
souvenirs pertinents, notes du vault. Ce n'est pas un bloc figé, parce
qu'un prompt figé devient faux dès qu'un paramètre change.

---

## 4. Vault Obsidian : quand le consulter, et quand ne pas le consulter

Le PLAN était explicite : « Ne force PAS [la mémoire longue] sur toutes les
requêtes. »

La règle (`memory::vault::should_consult`) est volontairement simple et
lisible — une heuristique, pas un classifieur :

```text
la demande référence-t-elle un contexte antérieur ?     → oui : on consulte
la demande dépasse-t-elle vault_min_request_chars (180) ? → oui : on consulte
sinon                                                      non
```

Les marqueurs de contexte sont explicites : « la dernière fois », « mon
projet », « comme d'habitude », « tu te souviens », « mon infrastructure »…

**Si le vault est introuvable, la demande continue.** Le vault est un
complément, jamais un préalable : la mémoire locale SQLite reste la première
source.

### Écriture

Jimmy écrit ses souvenirs dans `<vault>\0_Inbox\Jimmy\` : une note Markdown
datée par souvenir, avec un frontmatter minimal (`type`, `source`, `date`).
L'apprentissage automatique alimente les deux couches en parallèle — SQLite
pour le rappel rapide, le vault pour la mémoire que l'utilisateur relit.

---

## 5. Mémoire locale

Trois tables, trois familles de souvenirs :

| Table | Rôle |
|---|---|
| `messages` | historique des conversations |
| `memories` + `memory_vectors` | souvenirs et vecteurs |
| `memories_fts` | index plein texte FTS5 |

### Recherche hybride

```text
requête → vecteur 512 dim ── similarité cosinus ──┐
requête → FTS5        ── score BM25             ──┴─▶ score pondéré par importance
```

Le vecteur vient d'abord de **LM Studio** (`memory/semantic.rs`, même modèle
d'embeddings que SynaptiQ) : « véhicule » retrouve « voiture ». LM Studio est
facultatif : s'il ne répond pas, Jimmy retombe sur le **vectoriseur de
hachage** (`memory/embed.rs`) — normalisation (minuscules, sans accents), mots
+ trigrammes, projection FNV-1a, poids `tf` sous-linéaire, normalisation L2.
Zéro dépendance, quelques microsecondes, mais **pas** sémantique ; la
recherche lexicale FTS5 compense. Ne pas retirer ce repli : c'est lui qui
garde la mémoire utilisable service éteint.

### Apprentissage automatique

Après chaque échange, un appel LLM court extrait ce qui mérite d'être retenu
au format JSON :

```json
[{"kind": "semantic", "content": "L'utilisateur préfère des réponses courtes."}]
```

Un modèle est utilisé plutôt que des règles, parce qu'un mot-clé « toujours »
attrape peu de choses et produit beaucoup de faux positifs. Coût : une requête
par échange. Garde-fou : réponse illisible → rien n'est appris, jamais d'échec.

---

## 6. Voix

```text
cpal (micro, 44/48 kHz)
  → rééchantillonnage 16 kHz
  → VAD par énergie RMS
  → fenêtre glissante 2,4 s
  → whisper.cpp (base-q5)
  → « Jimmy » détecté ?
      non → fenêtre suivante (CPU libre entre les phrases)
      oui → accumulation jusqu'au silence → transcription de la phrase
           → agent → TTS → avatar
```

### Pourquoi whisper.cpp pour le wake word

Un modèle de wake word dédié (openWakeWord, Porcupine) serait plus rapide et
plus fiable. Porcupine exige une clé et un envoi audio réseau : exclu, le PLAN
demande du local. openWakeWord ajouterait ONNX Runtime et un pipeline de
features non trivial.

whisper.cpp avec un **VAD en amont** est le meilleur compromis disponible : le
CPU est au repos entre les phrases, et la latence n'existe que pendant la
parole. Mesuré : **1,3 s** pour `base-q5`.

### Le problème du mot court

« Jimmy » isolé est précisément le cas le plus difficile pour Whisper. Sur le
modèle `base`, la transcription observée est **« J'y mise »**. Ce n'est pas un
bug du détecteur mais une limite du modèle.

La réponse (`voice::find_wake_prefix`) compare sur des tokens normalisés et
accepte :

- le mot exact (`jimmy`) ;
- une distance d'édition bornée selon la longueur (`jimi`, `jimie`) ;
- **trois formes exactes** observées en production : `j ai mis`, `j y mise`,
  `chemise`.

Les formes exactes ne peuvent pas provoquer de faux positif : « j'ai besoin »
donne `j ai besoin`, qui ne correspond à aucune. Un test unitaire verrouille ce
cas (`pas_de_faux_positif_sur_jai`).

### TTS : les règles qui viennent de l'échec

Deux règles apprises à l'usage, reprises dans `providers/tts.rs` :

1. **Les retours à la ligne sont remplacés par des espaces.** Ils provoquent des
   coupures dans la lecture — c'est ce qui rendait la première voix saccadée.
2. **Un seul appel par piste, avec la voix imposée.** Sans voix explicite, Fish
   Audio choisit au hasard et le timbre change d'une réponse à l'autre.

---

## 7. Permissions

Trois capacités (`read`, `write`, `execute`) plus `network`. Une règle porte :

```rust
struct AccessRule {
    granted: bool,           // false = refus net
    allow_paths: Vec<String>,   // motifs, vide = tous
    allow_commands: Vec<String>,// motifs, vide = tous
    deny_commands: Vec<String>, // évalués en priorité
}
```

**Vérification avant exécution**, jamais après. Une liste de commandes
dangereuses (`diskpart`, `bcdedit`, `format`…) est refusée par défaut.

La structure est déjà prête pour un raffinement futur par outil, par service,
par application ou par capacité : il suffit d'ajouter un niveau de règle, sans
changer les appelants.

**Invariant de sécurité** (PLAN, risque n° 5) : créer un skill ne donne aucun
droit nouveau. Les skills passent par exactement les mêmes vérifications que
n'importe quel outil.

### Fichiers sensibles : l'accord au cas par cas

Les permissions ci-dessus sont accordées une fois pour toutes ; elles ne
distinguent pas un `.env` d'un fichier de code. `agent/src/sensitive.rs`
ajoute une seconde barrière, appelée dans la boucle avant chaque outil : une
**écriture** repérée dans un fichier sensible (`.env*`, clés, `secret*`,
`credential*`, dossiers `.ssh/`, `secure/`…) — par `write_file`, une commande
PowerShell, une commande distante MCP (`vps_exec`, `ssh`, `docker`…) ou un
envoi de fichier — suspend l'outil et émet `AgentEvent::Approval`. Le Chat
affiche une carte « Autoriser / Refuser » ; refus, 5 min sans réponse ou
demande vocale (personne pour cliquer) = outil non exécuté. La lecture reste
libre. Détection par motifs, pas un bac à sable (HANDOFF, piège 77).

---

## 8. Frontend : TypeScript sans framework

Sept vues, un routeur de vingt lignes, un magasin réactif de dix lignes.

**Pourquoi pas React ?** La V1 tient en quatre écrans. Une dépendance de plus
n'apporte rien ici et alourdirait le chargement de la fenêtre. Si l'interface
grandit, `ui.ts` est le seul fichier à remplacer.

Une conséquence importante : **toute la surface Rust passe par
`desktop/src/api.ts`**. Aucun `invoke` n'est appelé ailleurs — si une commande
change de nom, il n'y a qu'un fichier à corriger.

---

## 9. Ce que le PLAN demandait, et l'écart assumé

| Demande du PLAN | Réalisé | Écart |
|---|---|---|
| Renard humanoïde | ✅ procédural, primitives Godot | pas de modèle 3D externe : zéro asset à maintenir |
| Fenêtre flottante sur le bureau | ✅ Godot transparent + always-on-top | — |
| Clic sur Jimmy ouvre l'interface | ✅ pont HTTP | — |
| Trois niveaux graphiques | ✅ `low`/`medium`/`high` | — |
| Webcam / streaming vidéo | ❌ hors périmètre prototype | — |
| Provider LLM configurable | ✅ catalogue OpenCode Go, sélection en direct, trois formats d'API (Chat, Responses, Messages) | — |
| Mémoire vectorielle locale | ✅ embeddings LM Studio, repli hachage 512 dim, + FTS5 | sémantique seulement si LM Studio tourne |
| Skills auto-créés | ✅ | — |
| MCP | ✅ client stdio | pas de transport HTTP, pas de marketplace |
| Vault Obsidian dynamique | ✅ heuristique documentée, lecture du vault, écriture des souvenirs | — |
| Voix « Clémence » | ✅ bibliothèque Fish Audio, « Le narrateur » par défaut | voir README |
| Installateur qui vérifie les prérequis | ✅ `scripts/install.ps1` | pas d'export Godot ni d'installateur NSIS signé |
| Mise à jour automatique | ⚠️ commande écrite, sans source branchée | pas de distribution publique |