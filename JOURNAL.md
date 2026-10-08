# Journal de bord — Jimmy

Journal chronologique du projet : ce qui a été fait, pourquoi, ce qui a été
mesuré, ce qui a cassé, ce qui reste. Il raconte l'histoire ; il ne remplace pas
les autres documents.

| Document | À lire pour… |
|---|---|
| `JOURNAL.md` (ici) | comprendre **comment on est arrivé là** et pourquoi |
| `HANDOFF.md` | l'**état technique actuel**, les commandes et les 87 pièges numérotés |
| `PLAN.md` | la vision et le cahier des charges d'origine |
| `README.md`, `docs/` | installer, lancer, architecture, dépannage |

Le projet tient en **cinq journées**. Le **3 octobre 2026**, de 00h29 à 23h :
une session **OpenCode** construit la V1, puis des sessions **Claude Code**
l'améliorent et la réparent à l'usage. Le **4 octobre**, une journée d'**usage
réel** (Claude Code) : chaque retour de l'utilisateur est reproduit puis
corrigé (section 3ter). Les **5 et 6 octobre** : streaming, garde-fou des
fichiers sensibles, trois formats d'API des modèles et publication du dépôt
sur GitHub (section 3quater). Le **7 octobre** : tâches de fond, vision,
SynaptiQ par MCP, lapin et navigateur (section 3octies).

---

## 1. En bref

**Jimmy** est un assistant IA personnel, incarné par un renard 3D qui vit sur le
bureau Windows. On lui parle (« Jimmy, … ») ou on lui écrit ; il comprend, agit
sur la machine avec des outils, répond à voix haute et s'anime. Prototype
personnel : tout tourne en local, sauf le modèle de langage et la synthèse
vocale.

```
Windows
├── Tauri (Rust)  : interface, agent, mémoire, outils, MCP, voix       agent/ + desktop/
├── Godot 4.5.1   : avatar 3D du renard (rendu seulement)              godot/
├── whisper.cpp   : reconnaissance vocale locale, deux serveurs        data/components/
└── services      : OpenCode Go (LLM), Fish Audio via OpenRouter (voix),
                    SynaptiQ et LM Studio en local (mémoire)
```

### État au 3 octobre 2026, 20h30

| Domaine | État | Vérifié par |
|---|---|---|
| Agent (boucle, outils fichiers/commandes/web, permissions, skills) | fonctionne | test de bout en bout avec le vrai modèle, 70 tests unitaires |
| Voix : mot d'éveil « Jimmy », « Oui ? », commande, réponse parlée | fonctionne | 4 scénarios d'écoute réels (audio injecté), pas encore en usage quotidien |
| Conversation continue (8 s sans redire le nom) | fonctionne | scénario à deux tours chronométré |
| Avatar : 3 skins, 3 profils graphiques, ombre au sol, esquive de la souris | fonctionne | captures de rendu, mesures de position de fenêtre |
| Interface (8 vues, pastille d'état, bandeau de phase) | fonctionne | 15 parcours sur la vraie application |
| MCP en stdio | fonctionne | 3 tests contre un vrai serveur Node ; pas encore un serveur du catalogue |
| Mémoire sémantique (LM Studio, repli sur le hachage) | fonctionne | test réel (« véhicule » retrouve « voiture ») |
| Choix du modèle de langage (principal et vocal) | fonctionne | liste de 36 modèles du compte, test réel de chaque modèle |
| Avatar : se relance après une mort silencieuse | fonctionne | Godot tué à la main, relancé par le chien de garde en 1 s |
| Mémoire longue : vault Obsidian | fonctionne | recherche réelle sur le vault, 370 notes en 0,39 s |
| Dépôt distant | **rien n'a été poussé** | 20 commits locaux |

### Ajouts du 4 octobre 2026

| Domaine | État | Vérifié par |
|---|---|---|
| Écoute : hésitations et parole longue | fonctionne | test d'écoute réel (1,8 s de pause au milieu de la phrase) |
| Fil de conversation du Chat | **réparé** (une session par message depuis la V1) | parcours d'interface : le second message garde le fil |
| Serveurs MCP visibles (Skills) | fonctionne | 3 serveurs réels connectés, 78 outils |
| Bibliothèque de voix, « Le narrateur » par défaut | fonctionne | synthèse réelle + parcours d'interface |
| Avatar qui regarde l'utilisateur | fonctionne | captures avant/après, validé par l'utilisateur |
| Dépôt | **aucun commit le 4 octobre** | 44 fichiers en attente de relecture |

**Chiffres** : 20 commits, environ 17 000 lignes de code (11 700 Rust,
3 000 TypeScript, 1 400 GDScript, 1 000 de scripts), 70 tests unitaires,
14 tests réels contre de vrais services, 15 parcours d'interface.

---

## 2. Session 1 — OpenCode : la V1 (00h29 → 14h38)

**Session** `ses_f0143431fffevk3YBOk6F7BJPM`, à rouvrir avec :

```
opencode -s ses_f0143431fffevk3YBOk6F7BJPM
```

| | |
|---|---|
| Titre | « Développement de l'agent desktop JIMMY selon PLAN.md » |
| Dossier | `C:/Users/user/Projet/jimmy-agent-personnel` |
| Agent et modèle | `build`, modèle `space-bunny-free` (fournisseur OpenCode Go, variante `max`) |
| Période | 3 octobre 2026, de 00h29 à 14h26 |
| Activité | 581 messages de l'agent : 280 commandes shell, 199 éditions, 84 lectures, 82 écritures, 16 recherches |
| Volume | 2,0 M de jetons en entrée, 254 k en sortie, 85 k de raisonnement ; coût négligeable (modèle gratuit) |
| Tes demandes | 4 seulement (voir plus bas) |

La session est stockée dans la base locale d'OpenCode
(`~/.local/share/opencode/opencode.db`, tables `session_v2` et
`session_message`). Elle a été relue pour ce journal à partir de cette base et
du résumé de contexte qu'OpenCode a lui-même produit (compaction de 13h12).

### 2.1 Le brief (00h29)

Une mission autonome de 17 000 caractères : transformer `PLAN.md` en une
application qui marche, sans demander confirmation à chaque décision, en
**testant réellement** chaque fonction (« compiler ne suffit pas »).

- **Contraintes** : local d'abord, aucun backend Jimmy dans le cloud, aucun
  SaaS (ni paiement, ni comptes, ni synchronisation). Tauri pour le bureau
  (pas Electron), Godot réservé au rendu de l'avatar, communication par HTTP
  local.
- **Personnage** : un renard humanoïde, premier *skin* d'une identité qui n'est
  pas le renard. États minimum : idle, listening, thinking, speaking.
- **Voix** : mot d'éveil « Jimmy » détecté localement, reconnaissance vocale
  locale, synthèse Fish Audio (`fish-audio/s2.1-pro-free:free`), modèle de
  langage OpenCode Go **configurable**.
- **Agent** : de vrais outils (commandes, fichiers, MCP, navigateur), des
  skills, une mémoire, SynaptiQ mobilisé quand c'est utile, trois niveaux de
  permissions.
- **Ordre de priorité** : Tauri, Godot, communication, chat, voix, outils, MCP,
  skills, mémoire, SynaptiQ, installateur.
- **Critère de fin** — le « happy path » : dire « Jimmy, analyse ce dossier et
  explique-moi ce que tu trouves », et obtenir la détection, la transcription,
  l'analyse avec outils, la réponse parlée, l'animation et la bulle.

### 2.2 Environnement (inspecté puis complété)

- **Déjà là** : Godot 4.5.1 (son nom `Godot_v4.5.1-stable_win64.exe` est un
  *dossier*), WebView2, Node 24.
- **Absent, installé** : Rust (rustup), Visual Studio Build Tools 2022, et
  whisper.cpp avec les modèles `base-q5`, `small-q5` et le détecteur de silence
  Silero (dans `data/components/whisper/`).
- **Secrets** : la clé OpenRouter est rangée dans `~/.secrets/openrouter.json`,
  un `.env` local est ignoré par git ; aucun secret dans l'historique.
- **APIs validées pour de vrai** : OpenCode Go (l'en-tête `x-opencode-session`
  est obligatoire), Fish Audio via OpenRouter, SynaptiQ local.

### 2.3 Ce qui a été construit en 1h25 (00h29 → 01h53, commit `69754b8`)

- Un espace de travail Cargo en deux parties : `agent/` (le cœur, **sans**
  dépendance à Tauri, donc testable sans fenêtre) et `desktop/src-tauri/`
  (l'enveloppe de la fenêtre).
- Une interface en TypeScript sans framework (7 vues, un routeur de
  20 lignes) ; toute la surface Rust passe par `desktop/src/api.ts`.
- Un avatar Godot **entièrement procédural** (des primitives, zéro fichier
  d'asset), animé par état, avec un petit serveur HTTP écrit à la main en
  GDScript.
- L'agent : boucle d'outils, prompt, permissions LECTURE / MODIFICATION /
  EXÉCUTION / RÉSEAU, skills, mémoire SQLite, client SynaptiQ.
- Le script d'installation (8 vérifications), les scripts de build, le test de
  bout en bout, la documentation (README, `docs/`, HANDOFF).
- **Preuves** : 35 tests unitaires verts, build sans avertissement, test réel
  de 35 s avec 4 outils et une réponse en français, avatar rendu avec fenêtre
  transparente, bulle avec accents, clic qui ouvre l'interface.

### 2.4 Décisions prises (et leur raison)

| Décision | Raison |
|---|---|
| Cœur Rust séparé de Tauri | tester l'agent avec `cargo test` sans ouvrir de fenêtre |
| Avatar procédural, sans asset | rien à maintenir ; contrepartie : un rendu « assemblage de primitives » |
| Serveur HTTP en GDScript (`TCPServer`) | pas de GDExtension à compiler |
| Reconnaissance vocale : `whisper-server.exe` en processus annexe | le modèle reste chargé, une requête coûte ~1 s |
| `base-q5` pour le mot d'éveil | 1,3 s contre 4,7 s pour `small-q5` |
| Mémoire par hachage de mots (512 dimensions) + recherche plein texte | simple et sans dépendance ; assumé comme non sémantique |
| Voix « Féminine » par défaut, au lieu de « Clémence » du plan | déjà validée par toi dans un skill ; les deux restent choisissables |
| Release par `tauri build --no-bundle` | c'est la CLI Tauri qui embarque les assets du frontend |

### 2.5 La suite de la session (10h00 → 14h38)

1. **Raccourci Bureau** (10h04, commit `93e11a2`) : `Jimmy.lnk` lance l'appli et
   l'avatar sans console. Ce qu'il a fallu corriger pour que ça marche :
   l'avatar démarrait depuis le JavaScript de l'interface (donc pas du tout si la
   fenêtre échouait) → déplacé dans `setup()` côté Rust ; et un doublon
   `frontendDist` dans la config Tauri faisait viser un serveur de développement.
2. **Voix et mot d'éveil en panne** (11h10, commit `652a978`) : ta capture
   d'écran a conduit à deux vrais bugs, reproduits puis testés.
   - Fish Audio ne produit pas de WAV (HTTP 400) ; il fallait demander du PCM.
     Et `build_output_stream` ne joue rien sans appel à `.play()` : la lecture
     bloquait indéfiniment.
   - La commande d'écoute existait côté Rust mais **n'était jamais appelée** par
     l'interface : le micro n'était jamais ouvert.
3. **Mise à jour du HANDOFF** (14h26, commit `56430d6`), à ta demande, pour
   préparer la suite : retravailler la qualité graphique de l'avatar et les
   fonctionnalités.

### 2.6 Ce que cette session a laissé

Une V1 qui marche sur le chemin principal, avec ses manques écrits noir sur
blanc dans le HANDOFF : avatar plat (pas d'occlusion ambiante, pas d'ombre),
skins sans effet, MCP « en configuration seulement », mémoire non sémantique,
modèle `small` sélectionnable mais jamais utilisé, pas de sons d'état. C'est le
point de départ de la session suivante.

---

## 3. Session 2 — Claude Code : améliorer, puis réparer à l'usage (14h40 → 20h45)

Reprise par la lecture du HANDOFF. Le modèle est passé d'Opus 5.5 à Sonnet 5.5
en cours de route. 18 commits (`79d83f0` → `76c23b2`).

### 3.1 Audit et plan (14h40)

Le HANDOFF a été vérifié ligne par ligne dans le code. L'avatar : exact. Mais
l'audit a trouvé des choses qu'il ne disait pas :

- **Le MCP n'était pas « en configuration seulement »** : un client complet
  existait (366 lignes) mais n'était instancié nulle part — le même piège que la
  commande d'écoute jamais appelée.
- `with-msvc.ps1` plantait dès qu'on redirigeait la sortie d'erreur ; un
  caractère corrompu dans `jimmy.gd` ; du code mort dans la bulle.
- Une recherche de skills Godot : le seul skill « 3D » du catalogue était une
  coquille vide ; installés à la place `godot-api` (doc de l'API 4.5.1 en local,
  842 classes) et `godot-shader-review`.

Plan en sept lots (0 à 6), validé par toi : nettoyage, rendu, style, skins,
sons d'état, MCP, puis STT et mémoire. Tu as tranché : sons avec la voix Fish Audio, deux
serveurs Whisper, embeddings par LM Studio.

### 3.2 Avatar : rendu, proportions, skins (lots 0 à 3, 15h04 → 15h12)

| Changement | Détail |
|---|---|
| Cadrage | caméra plus proche et plongeante : le renard passe de 35 % à ~55 % de la hauteur |
| Rendu | tonemapper AgX (FILMIC grisait l'orange), saturation, occlusion ambiante à l'échelle du personnage |
| Ancrage | ombre de contact + ombre portée sur fond transparent (voir piège 17) |
| Proportions | cou raccourci (effet « girafe »), bassin, épaules, bras rapprochés |
| Style | contour par coque inversée (sauf yeux et cônes), maillage ×1 / ×1,5 / ×2 selon le profil, fourrure par normal map **générée** (toujours zéro asset) |
| Skins | renard, arctique, fennec (grandes oreilles), changés à chaud |

La recette de l'ombre sur fond transparent a coûté trois essais : le matériau
`shadow_to_opacity` est invisible avec un albedo noir, et les lumières sans
ombre l'effacent. Elle est consignée (HANDOFF piège 17, et mémoire SynaptiQ).

Au passage : le changement de skin et de qualité dans les Paramètres ne
touchait jamais l'avatar (la méthode Rust n'était appelée nulle part).

### 3.3 Sons d'état, MCP, deux serveurs Whisper, mémoire sémantique (lots 4 à 6)

- **Sons d'état** (`3dedf4f`) : « Oui ? », « C'est prêt. », « Oups… »
  synthétisés avec la voix de Jimmy et mis en cache par voix.
- **MCP** (`d64ed54`) : client en stdio réellement branché, outils exposés au
  modèle, outil `mcp_add_server`, lanceur `cmd /C` pour `npx`, délais d'attente.
  Trois défauts trouvés en chemin : serveur introuvable quand son nom contenait
  un espace, noms d'outils refusés par l'API, `isError` traité comme un succès.
- **Deux serveurs Whisper** (`5c652e9`, ton choix « les deux ») : `base` sur le
  port 8178 pour le mot d'éveil, `small` sur le 8179 pour la commande, avec
  repli automatique si le second ne démarre pas.
- **Mémoire sémantique** : embeddings par LM Studio (même modèle que SynaptiQ,
  384 dimensions), repli sur le hachage s'il est éteint.

### 3.4 « L'écoute ne fonctionne pas » : audit du fond et de l'interface (`14e86e6`, 16h33)

Premier vrai retour d'usage. Au lieu de deviner, l'interface réelle a été
pilotée à distance (débogage WebView2, Playwright) et les logs de la release —
jusque-là perdus car l'appli n'a pas de console — ont été écrits dans
`data/logs/jimmy.log`.

**Pourquoi l'écoute ne marchait pas : quatre causes qui s'additionnaient.**

1. **Le son du micro était déformé.** Le flux est entrelacé et à 48 kHz stéréo ;
   il était lu comme du mono 16 kHz. Whisper recevait un son ralenti.
2. **Le seuil de détection était fixe et trop haut** pour ton micro.
3. **Whisper écrivait « Guimmi »** pour « Jimmy » : prompt de contexte, comparaison
   phonétique, interjections (« hé Jimmy »).
4. **La détection tombait en plein milieu de la phrase** : Jimmy concluait « Jimmy
   seul », jouait « Oui ? » et vidait le micro… pendant que tu parlais.

**Côté interface** : chaque changement de page **empilait** une vue sous les
précédentes (10 vues montées après un tour), des écouteurs jamais libérés, un
journal d'activité jamais inséré, des messages de succès affichés même en cas
d'échec, des libellés en anglais.

Résultat : 12 parcours d'interface sur la vraie application, un scénario
d'écoute de bout en bout par **injection audio** (ton micro intégré n'entend pas
les haut-parleurs, donc on ne peut pas tester en faisant parler Jimmy).

### 3.5 Premières conversations réelles (`ccf5906` puis `7b26272`, 18h11 → 18h45)

- **« Un mot rapide incompréhensible »** : la sortie audio est entrelacée comme
  l'entrée ; un son mono écrit une case sur deux en stéréo est joué **deux fois
  trop vite**. Toutes les voix étaient touchées.
- **« Il réagit à Jimmy, puis plus rien »** : après le « Oui ? », le micro était
  vidé puis lu par fenêtre fixe : vide, donc la boucle s'arrêtait. Remplacé par
  une lecture par curseur.
- **Le chat ne parlait jamais** : il ne jouait que le petit son « C'est prêt. »
  (lui aussi trop rapide). Il lit maintenant sa réponse, nettoyée du Markdown.
- **La mémoire plantait** : la table `memory_semantic` n'avait jamais été créée
  sur ta base existante (le numéro de schéma n'avait pas bougé) → schéma v2.
- **Auto-réparation** : un serveur Whisper mort est relancé à la volée. Elle est
  née d'un incident de méthode : tu avais relancé Jimmy pendant que mes tests
  d'écoute tournaient ; il avait réutilisé leurs serveurs Whisper, qui ont été
  arrêtés à la fin des tests, et plus rien ne se transcrivait. Règle ajoutée :
  on n'exécute pas les tests pendant que Jimmy sert l'utilisateur (piège 35).
- **Avatar, ta demande en cours de route** : seule la silhouette capte la souris
  (le reste de la fenêtre laisse passer les clics) et Jimmy **s'écarte** quand
  le curseur approche, puis revient. On peut le rattraper là où il s'est
  réfugié ; l'endroit où on le lâche devient sa nouvelle place.

### 3.6 « Pas assez fluide » : mesurer avant de corriger (`73eadd4`, `a1c9aa9`, 19h54 → 20h02)

Le journal détaillé (niveaux, durées, textes entendus) a permis de chiffrer ce
qui était ressenti.

| Poste | Avant | Après | Cause et correction |
|---|---|---|---|
| Fin de phrase → commande transmise | 4,2–4,5 s | **1,6–2,2 s** | silence de fin 900→700 ms, analyse par trames de 50 ms, et surtout… |
| Transcription précise (`small`) | 3,4 s | **~1,4 s** | Whisper traite toujours 30 s : `-ac 640` (contexte 12,8 s), sans perte mesurée de précision |
| « Jimmy » seul → « Oui ? » | ~5 s | ~1,5 s | si le modèle rapide l'a reconnu et que la prise est brève, `small` n'est pas appelé |
| Réponse → début de la voix | +3 à 5 s | immédiat | l'**apprentissage mémoire** (un appel de plus au modèle) précédait l'envoi de la réponse |
| Première requête après une pause | échec | réussie | connexions HTTP gardées vers Whisper, fermées côté serveur |

Ajouts : **conversation continue** (8 s d'écoute après chaque réponse sans
redire « Jimmy »), un événement `Listen` et une pastille avec compte à rebours
(« à toi · 6 s ») pour savoir à chaque instant ce que Jimmy attend, un filtre
des phrases que Whisper invente sur du bruit, un mode vocal de l'agent
(réponses courtes, sans les outils de mémoire déjà injectés).

Puis un défaut vu dans ton propre journal : un bruit à 0,0036 pour un seuil de
0,0035 rouvrait sans cesse la parole, **une prise a duré 19,9 s pour 250 ms de
parole** et Jimmy était aveugle. Corrigé par un garde-fou (parole trop éparse →
prise abandonnée) et un seuil appris des fausses alertes.

**Ce qui n'est pas corrigeable de notre côté** : la latence du modèle de langage
varie de 2 s à 25 s pour la **même requête**. Tout ce qui dépendait de nous a
été écarté un à un (outils, taille du prompt, historique, session, appels
concurrents, raisonnement caché). Une requête « couverte » (la doubler après
3 s) ne gagne rien : quand l'une est lente, l'autre l'est aussi. Réponse :
« Un instant. » dit à voix haute au-delà de 5 s.

Incident : la validation complète a été **arrêtée par manque de mémoire** de la
machine (compilation, deux serveurs Whisper, Godot et le modèle en même temps).
Elle a été relancée en étapes séparées.

### 3.7 L'erreur HTTP 400 et la bibliothèque de modèles (`76c23b2`, 20h31)

**L'erreur** : au bout d'une conversation, Jimmy a lu à voix haute
« HTTP 400 Bad Request… invalid request ». Le journal montrait une longue
exploration (6 appels au modèle, 17 outils) suivie d'une commande qui échouait
après 3 s. L'erreur a été **reproduite à l'identique** par un test (tour 2 en
échec) avant d'être corrigée.

- **Cause** : l'historique rechargé contenait des résultats d'outils **sans
  leur identifiant d'appel** (jamais stocké), et la coupure aux 20 derniers
  messages laissait des résultats orphelins en tête. L'API refuse les deux.
- **Correction** : on ne recharge que les demandes et les réponses finales
  (le contexte passe de 15 000 à ~3 000 jetons). Filet de sécurité : sur un 400
  au premier appel, nouvel essai sans historique.
- **Autour** : une limite de 6 étapes ou de 90 s en vocal, avec un dernier appel
  qui conclut (au lieu de lire une phrase d'annonce) ; l'erreur dite à voix
  haute tient en une phrase, le JSON reste dans le chat ; `localhost` →
  `127.0.0.1` pour LM Studio (2,3 s perdues à chaque rappel de mémoire).

**La bibliothèque de modèles** (Paramètres) : la liste vient du compte
(36 modèles) enrichie par le catalogue public (32 reconnus). L'ancien code
lisait une clé qui n'existe pas dans ce catalogue : la liste était toujours
vide. « Tester » envoie la requête **dans les conditions de Jimmy** (simple puis
avec un outil) : `grok-4.6` et `muse-spark-*` répondent « Model does not support
this protocol » et sont inutilisables. Un modèle n'est appliqué que s'il vient de
répondre. On choisit un modèle principal et un modèle vocal.

Un bug évité de peu : l'identifiant renvoyé à l'interface avait le préfixe
`opencode-go/` ; cliquer sur « Principal » aurait écrit cette valeur dans la
configuration et cassé toutes les requêtes. La suite d'interface l'a attrapé ;
elle vérifie maintenant la valeur réellement enregistrée.

---

## 3bis. Session 3 — l'avatar réparé et la mémoire au vault (22h → 23h)

Deux chantiers, sur un constat d'usage et une décision.

### « L'avatar ne se lance pas »

Le renard apparaissait, puis **disparaissait sans trace** : aucun crash dans le
journal Windows, aucun dump, le journal de Godot s'arrêtait net. L'utilisateur
relançait Jimmy plusieurs fois par minute (le journal en garde la trace : deux
lancements à 20h29 et 20h30, une minute d'écart).

La reproduction a montré que Godot lancé à la main tenait parfaitement — le
défaut était donc dans **Jimmy**, côté Rust, et il y en avait trois :

1. **Une course au lancement.** Deux appels concurrents à `start_avatar`
   testaient `is_up()` avant que Godot n'ouvre son port, lançaient chacun un
   processus ; le second écrasait le premier dans le champ `godot`, et
   `kill_on_drop` **tuait l'avatar vivant**. Le second mourait sur le port
   occupé. Il ne restait rien — et rien dans le journal.
2. **Un statut qui mentait.** `"running"` testait `godot.is_some()`, vrai même
   pour un processus **terminé**. L'interface affichait « avatar en marche »
   sans avatar.
3. **Aucun filet.** Un Godot mort ne revenait jamais.

Correction : un `Mutex` sérialise les lancements ; une sortie immédiate est
détectée (`try_wait`) et expliquée au lieu d'attendre 10 s ; le statut interroge
le vrai état du processus ; un **chien de garde** relance l'avatar tué, en
notant le code de sortie — la prochaine mort sera datée. Vérifié en tuant Godot
à la main : relancé en une seconde.

### La mémoire persistante passe au vault Obsidian

Décision de Jimmy : **plus de service de mémoire externe**. Les souvenirs de
Jimmy sont des notes Markdown dans son vault (`C:\Obsidian\Jimmy`), qu'il relit
comme les autres notes.

- `synaptiq.rs` est supprimé ; ses heuristiques de déclenchement sont reprises
  par le module `memory/vault.rs`.
- Trois outils : `vault_search` (plein texte, accents et casse ignorés),
  `vault_read`, `vault_write`. Le prompt injecte les notes proches comme le
  faisait Synaptiq, mais depuis le disque, sans réseau.
- L'apprentissage automatique écrit désormais **dans les deux couches** :
  SQLite pour la réactivité, le vault pour ce que l'utilisateur relit.
- Vérifié en réel : `[vault] ouvert : C:\Obsidian\Jimmy (370 notes)`, une
  recherche réelle sur le vault en 0,39 s.

### « En plein travail, puis plus rien, et le fil perdu »

Analyse demandée à partir du journal et de la base. Deux causes distinctes.

1. **Un outil gelait la conversation.** Le cas réel : « Le contenu du dossier
   agent-pentest ». `list_directory` répond en 0,2 s, puis `search_files` sur
   `C:\Users\user\Projet` tourne **4 min 22 s** à lire chaque fichier pour y
   chercher une chaîne, sans aucune borne. La conversation reste muette ; le
   budget vocal de 90 s ne peut rien interrompre, il se teste *entre* les
   itérations, jamais pendant un outil. Au retour, budget dépassé : un dernier
   appel conclut « Il reste à lire ses fichiers ».
2. **La session mourait avec l'écoute.** `voice_session` était une variable
   locale à la boucle d'écoute. L'écoute s'arrête à 20:47:49, l'utilisateur la
   relance à 20:54:25 → session neuve. Sa question « j'attends toujours ton
   rapport » ouvre une autre session, qui ignore le travail précédent ; Jimmy
   répond « Je n'ai pas de rapport en cours dans cette session ». L'écart
   (~7 min) était pourtant **sous** les 10 min de `VOICE_SESSION_IDLE` : le
   fautif est la reprise d'écoute, pas le délai.

Corrections : `search_files` porte un budget de 10 s (fichiers > 2 Mo sautés) ;
chaque outil est logué avec son nom et sa durée ; `voice_session` vit sur `App`
et survit à un arrêt/relance de l'écoute.

---

## 3ter. Session 4 — 4 octobre : l'usage réel (Claude Code)

Une journée sans plan préalable : l'utilisateur se sert de Jimmy, signale ce
qui coince, et chaque point est **reproduit avant d'être corrigé** (test qui
échoue, rejeu d'une requête réelle, capture avant/après). Le récit suit
l'ordre des demandes.

### L'écoute coupait la parole

Retour d'usage : « la période d'écoute est assez courte et ne tient pas bien
le flux ». Le journal le montre : « Je viens de voir que dans ta mémoire tu as
mis... » est transmis coupé, une pause de réflexion ayant clos la prise (700 ms
de silence suffisaient). Et toute parole au-delà de 12 s était tranchée.

Corrigé sans relever la limite de 12 s (le serveur de commande ne voit que
12,8 s d'audio) : silence de fin qui s'allonge pour les longues phrases (jusqu'à
1,2 s), coupure des longs segments dans une pause, et reprise de l'écoute quand
la phrase paraît inachevée ou a été coupée, les segments étant recollés.
Reproduit puis vérifié par un test d'écoute de bout en bout (vrai Whisper,
vraie boucle, vrai modèle) : sans la reprise, Jimmy répondait « Ta phrase est
coupée » ; avec, la phrase arrive entière (piège 57). Les autres tests d'écoute
passent ; release recompilée, interface 14/15 (l'échec, la bibliothèque de
modèles, est sans lien). Reste à l'essayer à la voix humaine.

### Le fil, la mémoire et les dérives du modèle

Même jour : sous-onglet **Serveurs MCP** dans Skills (état réel, outils,
commandes aux clés masquées). Puis « Jimmy n'a pas la mémoire du fil » : en
réalité, **chaque message du Chat ouvrait une session neuve depuis la V1**
(`sessionId` ignoré par Tauri dans une structure imbriquée, piège 59). Corrigé,
et le contexte de chaque demande porte désormais un résumé des outils récents
et un socle de mémoire, sans hausse du coût en jetons (liste d'outils MCP
regroupée).

Puis « Jimmy a buggé grave » : deux réponses du modèle parties en charabia
multilingue. Rejouée 32 fois, la requête réelle ne déraille qu'une fois :
c'est le modèle gratuit, pas le contexte. Ajout d'un filet (régénération, ou
coupure à la dernière phrase saine) avant affichage et lecture à voix haute
(piège 60).

### Le modèle, la voix, le regard

Le soir : MiMo choisi dans les Paramètres revenait toujours à
`space-bunny-free` — le `.env` l'écrasait à chaque lancement (piège 62), et la
suite d'interface effaçait le modèle vocal (piège 61). Puis une bibliothèque de
voix (catalogue Fish Audio, écoute avant choix), avec « Le narrateur » comme
voix par défaut (piège 63).

Fin de soirée : panne HTTP 500 du fournisseur en vocal, désormais journalisée
et rejouée si elle est rapide (piège 64) ; bibliothèque de voix déplacée dans
l'onglet Voix (piège 65) ; Jimmy sait que son vault est partagé avec d'autres
agents (piège 66) ; et l'avatar regarde l'utilisateur au lieu de la souris
(piège 67). Validé par l'utilisateur : « l'avatar me regarde et a en plus de
temps en temps des animations d'attente, c'est parfait ».

### L'onglet Chat façon Codex Desktop (lot 1)

Dernière demande de la journée, cadrée avant de coder : l'utilisateur veut
naviguer dans un dossier et ouvrir un projet depuis le chat. Lot 1 livré : un
projet par conversation (Jimmy y travaille), sélecteur « Ouvrir un dossier… »
natif et projets récents, panneau « Fichiers » avec arborescence, aperçu et
ouverture dans l'application par défaut de Windows (les exécutables ne sont
jamais lancés). Le parcours d'interface a attrapé un vrai bug : le menu des
projets ne se cachait jamais (`display: flex` plus fort que `hidden`, piège
68). Restent les points 3 à 5 : « @ » pour citer un fichier, chemins
cliquables, historique par projet.

### « Trois inputs et il n'a toujours pas démarré la tâche »

Le soir, une vraie session de travail vocale sur `agent-reddit` : quatre
« oui, go » et `notify.py` jamais écrit. Chaque tour s'arrêtait pile à 6 étapes
après avoir relu les mêmes fichiers. Le test de bout en bout sur une copie du
projet a fait remonter, un par un, **sept freins cumulés** : lecture de fichier
tronquée sans suite possible, budget vocal minuscule, `max_tokens` trop court
pour un modèle qui raisonne (l'appel d'écriture arrivait coupé, en texte, pris
pour la réponse finale), 9 500 jetons de définitions MCP à chaque appel, un
modèle qui explore sans fin, une erreur qui jetait tout le travail, et le
silence pendant les minutes de travail (piège 70). Après corrections : la même
tâche aboutit en **un seul tour vocal** (22 étapes, 3 min 30), fichier écrit,
branché, 276 tests verts — avec Jimmy qui dit où il en est.

### Bibliothèque de skins : ours et robot (4 octobre, soir)

Demande de l'utilisateur : cinq skins de base — les trois renards existants,
un ours et un robot. Le système était déjà fait pour ça : une entrée de
palette + paramètres de forme dans `jimmy.gd`, le libellé dans
`providers::avatar::SKINS`, et l'interface suit (les 5 boutons apparaissent
dans Skin et Paramètres sans une ligne de TypeScript). Trois paramètres de
forme ont été ajoutés : `tail_scale` (0 = pas de queue), `metallic`
(surface métallique) et `has_fur` (normal map de fourrure ou surface lisse).

Deux enseignements. D'abord, un paramètre nommé `fur` entrait en collision
avec la clé de couleur `fur` : Godot refusait le script entier et l'avatar
devenait **invisible** (fenêtre vivante, rendu vide, rien dans `jimmy.log`)
— piège 72. Ensuite, la vérification qui a compté est la capture `/snapshot`
de chaque skin, pas la compilation : la suite d'interface était passée alors
que le script Godot était cassé (elle n'affiche pas l'avatar).

### La croissance, lot 1 : le journal d'expérience (4 octobre, soir)

Chantier validé dans son entier (recherche sur les agents auto-améliorants,
puis plan en quatre lots). Lot 1 = le mécanisme « Reflexion » : après un tour
qui a connu des échecs d'outils, l'extraction (jusqu'ici aveugle à la
trajectoire) reçoit la liste des outils en échec et peut produire des
souvenirs de genre `lesson` — l'échec observé + la façon validée de s'y
prendre — stockés en base (importance 0,65) et dans le vault, rappelés par la
recherche sémantique aux tâches suivantes.

Deux enseignements de mise au point, tous deux venus du test réel :
- **Deux extracteurs séparés = deux fois plus d'expositions à la dérive du
  modèle gratuit** (une réponse illisible sur deux sur l'extracteur de
  leçons). Fusionnés en un appel unique qui couvre faits et leçons.
- **Une extraction silencieusement perdue = une leçon jamais écrite.** Un
  tour incident où le fournisseur déraille ne devait pas passer à la trappe :
  l'extraction est déclenchée à chaque tour incident (hors échantillonnage
  1-tour-sur-4) et rejouée une fois quand la réponse n'est pas un JSON
  analysable. Observé en réel : `extraction illisible (tentative 1)` →
  réessai → leçon écrite.

### La croissance, lot 2 : la capture de compétence (4 octobre, soir)

La bibliothèque de compétences de Jimmy avait une moitié manquante : les
outils `create_skill`/`update_skill` existaient depuis la V1 (et le prompt y
invitait), mais **rien ne regardait en arrière** après une trajectoire
réussie — la condensation ne se faisait que si le modèle y pensait pendant la
tâche. Le mécanisme « Voyager » tient au contraire au cumul : après une
demande qui a mobilisé ≥ 3 outils, un appel de capture condense la démarche
en SKILL.md, en tâche de fond. Le catalogue existant est passé à
l'extracteur : pas de doublon, et une amélioration nette réutilise le nom du
skill existant (aiguisage, cf. SkillWeaver).

L'enseignement technique de ce lot : **un modèle gratuit ne sort pas du JSON
multi-lignes fiable** — il pretty-printe ses chaînes avec de vrais sauts de
ligne, JSON invalide, et les deux tentatives s'y sont cassées quatre fois
d'affilée. Le format « trois lignes » (name / description / body) traverse :
la prose autour est ignorée, le corps libre peut être long. Vérifié en réel :
la même trajectoire d'analyse passait de 2 captures perdues à 1 capture
écrite, au premier essai.

### La croissance, lot 3 : la revue périodique (5 octobre)

La revue hebdomadaire ferme la boucle du curriculum : une boucle de fond
(contrôle toutes les 30 minutes, fenêtre `growth.review_days` = 7 jours par
défaut, 0 = jamais) relit les leçons nouvelles depuis la dernière revue —
l'état vit dans `data/growth_state.json`, Jimmy relancé n'oublie pas
l'échéance — et rédige une session « Revue » dans le Chat.

Deux choix de sécurité, faits avant de coder. D'abord, la revue **ne passe
pas par la boucle d'agent** : un appel unique de rédaction, zéro outil. Une
session qui ne peut rien appliquer, même si le modèle désobéit. Ensuite, la
revue n'est déclenchée que par des **leçons nouvelles** : pas de revue
vide, pas d'appel payé pour rien. L'utilisateur lit la session (elle
apparaît dans Historique) et décide : il peut demander l'application dans
une conversation ordinaire, où les permissions s'appliquent.

Vérifié en réel : deux leçons en mémoire → session « Revue » contenant une
skill proposée en règles (`inspection-ciblee-fichiers`), seconde revue
immédiate non due.

### La croissance, lot 4 : l'auto-édition du prompt et son filet (5 octobre)

Le lot le plus risqué, gardé pour la fin. Le prompt système de Jimmy gagne
une section mutable : les **amendements** (`data/growth_amendments.md`,
absent par défaut — prompt inchangé). Une revue qui a vu les leçons montrer
une instruction manquante propose UNE phrase impérative ; l'utilisateur
demande l'application dans une conversation ordinaire (où Jimmy écrit le
fichier, `.bak` d'abord) ; et l'amendement n'est acté que si le **filet de
rejeu** passe : sur les 3 dernières demandes réelles, les réponses avec
l'amendement restent saines et de longueur comparable — le mécanisme
« GEPA » ramené à la taille d'un prototype, avec un filet volontairement
grossier (santé, pas qualité mesurée).

La règle de sécurité qui a façonné le lot : **le modèle ne peut jamais se
valider lui-même**. La validation vit dans un test réel ignoré
(`cargo test --test amendment -- --ignored`), que l'utilisateur lance ; un
outil de Jimmy ne fait pas d'A/B, ne lit pas le corpus. Si un jour un
amendement tourne mal : le `.bak`, et la suppression de la ligne suffit —
pas de migration, pas de base touchée.

Bilan du chantier « croissance » : leçon après échec (Reflexion), compétence
après succès (Voyager), revue qui propose (curriculum), amendement qui se
rejoue (GEPA) — quatre mécanismes, tous vérifiés sur le vrai modèle, aucun
appel qui retarde une réponse (tous en tâche de fond ou sur action
explicite).

### L'interface Chat « façon Codex » : 4 lots d'un coup (5 octobre)

Après le bilan de logs (le premier usage réel où la croissance a capturé
deux compétences), l'utilisateur veut l'expérience Codex dans l'onglet
Chat. Quatre lots livrés d'affilée, chacun passé par la vraie fenêtre :

- **@ pour citer un fichier** : un menu des fichiers du projet, filtré
  (casse et accents repliés via le normaliseur de Jimmy), Entrée insère le
  chemin relatif sans envoyer. Le parcours récursif est borné (4000 entrées
  visitées, 20 résultats), sans `.git`/`node_modules`/`.venv` ; sans filtre,
  seulement le premier niveau, sans fichiers cachés (`.env` s'obtient en le
  tapant explicitement).
- **Chemins cliquables** : une réponse qui contient `src/notify.py:45` fait
  naître des puces qui ouvrent un aperçu (saut à la ligne surlignée), via
  `fs_preview`, la commande du panneau — zéro commande nouvelle. Un
  exécutable n'est jamais lancé, règle héritée de l'explorateur. Une URL
  (« https://… ») n'est jamais prise pour un fichier.
- **Bloc « travaux »** : sous la réponse qui a écrit des fichiers, une puce
  par fichier + un **diff git lecture seule** (borné 10 s / 500 Ko, le
  dépôt trouvé depuis le fichier). Fichier non commité : le diff renvoie sa
  raison au lieu de rien. Le bloc ne suit que `write_file` : les écritures
  par `run_command` (sed, résultats d'écriture) échappent — assumé.
- **Historique par projet** : sessions groupées, champ de recherche en
  direct, Ctrl+K depuis n'importe quelle vue (un écouteur global unique au
  démarrage, pas empilé par navigation).

Lot 0 avant tout ça : `max_tokens` de l'extraction 400 → 800 et capture
800 → 1 200 — à l'usage, une extraction du 12:50 mourait sur un JSON en
```fence``` qui dépassait 400 jetons ; les extraits bruts des échecs
illiterals sont maintenant au journal en `warn`, donc la prochaine dérive
se lira sans relance en debug.

Les deux enseignements de la journée : (1) un ajustement de comportement
(« lit le @ de la compose ») se teste cent fois mieux par un étape qui
VÉRIFE l'insertion **sans envoi** que par des captures ; (2) la suite
d'interface a rattrapé trois de mes erreurs de plomberie (formulaire avec
syntaxe TS dans du JS, garde TypeScript manquante, attente mal écrite) —
elle ne teste pas que Jimmy : elle teste ce que j'écris.

### Priorité suivante, décidée par l'usage : la sortie de Jimmy (5 octobre, soir)

Bilan de deux sessions d'usage : le cœur tient — tâches de bout en bout,
croissance qui capte des compétences — mais **la façon dont Jimmy parle est
indigeste** : la voix récite tout, code compris, et les réponses débordent
de jargon de dev. Reproduction facile : relire n'importe quelle réponse
d'analyse dans le journal. Traité en session suivante : du court, du net,
du précis ( Jimmy n'explique que l'essentiel, à l'écran comme à voix
haute) — et une voix qui ne récite **jamais** le code, le détail restant
sur l'écran.

### La sortie de Jimmy : texte court et net (5 octobre)

Le journal chiffre le défaut : la plupart des réponses font 6 à 93 caractères,
mais deux réponses d'analyse ont été **lues à voix haute en 9 147 et 3 046
caractères** (`[tts] lecture de N caractères`) — plusieurs minutes de récitation,
code et jargon de développement compris.

Corrigé en deux endroits.

1. **Le prompt** (`IDENTITY`, section « Sortie ») : une à trois phrases, la
   réponse en tête, pas de préambule ni de récapitulatif d'étapes ; le détail
   technique (code, chemins, commandes) va dans un bloc de code, lu à l'écran
   seulement. La section « Voix » interdit désormais explicitement le code et
   les chemins dans la prose ; la règle 5 renvoie vers cette section.
2. **La lecture** : `prepare_for_speech` retirait déjà Markdown et code ;
   `limit_for_speech` borne ce qui est **dit** (phrases entières,
   `SPOKEN_MAX_CHARS` = 240 caractères). Le reste demeure dans la bulle : ce qui
   est dit n'est plus tout ce qui est affiché.

Défaut trouvé en relisant la sortie réelle : la découpe sur le point coupait les
noms de fichiers (« todo.md » → « todo. » puis « md »), à l'écrit comme à
l'oral. Corrigé par `ends_sentence` (un point ne finit une phrase que suivi d'un
blanc).

Vérifié : 130 tests unitaires ; `--test happy_path
chemin_heureux_analyse_un_dossier --ignored` sur le vrai modèle, deux rejeux →
réponse d'environ 650 caractères, **190 à 228 dits à voix haute** selon le tour,
nom de fichier intact.

### Le mot d'arrêt « STOP »

Demande de l'utilisateur : la transcription lance parfois des tâches pour rien,
il veut pouvoir tout arrêter à la voix. Pendant une tâche, la boucle d'écoute
attendait sans lire le micro : un guetteur écoute désormais les courtes prises
de parole et, sur « STOP » (phrase réduite au mot d'arrêt), la tâche est
abandonnée, ses commandes tuées, la voix coupée ; Jimmy répond « D'accord,
j'arrête. Je t'écoute. ». Bouton « Arrêter » dans le Chat. Vérifié par un test
vocal réel (« Stop ! » dit en pleine tâche) et par la suite d'interface (arrêt
en 64 ms) — piège 71.

### Ce que cette journée a appris

- **Trois bugs « de comportement » étaient des bugs de plomberie** : le fil
  perdu (un champ JSON mal nommé), le modèle qui revenait tout seul (le
  `.env`), le modèle vocal effacé (la suite de tests elle-même). Aucun ne se
  voyait dans le code de la fonction concernée.
- **Rejouer avant de supposer** : la dérive multilingue semblait liée au
  nouveau contexte ; 32 rejeux ont montré que non, et que la température n'y
  pouvait rien.
- **Une suite de tests touche aux données réelles** : elle doit restaurer
  exactement ce qu'elle modifie, et lire l'état réel (la session) plutôt que
  l'affichage (une bulle vocale parasite faussait un parcours).
- **Un réglage qu'on cherche va là où on le cherche** : la bibliothèque de
  voix existait, mais sous 36 modèles.

---

## 3quater. Session 5 — 5 et 6 octobre : sécurité, formats d'API, publication (Claude Code)

Session ouverte en reprise : GLM 5.3 Flash (session OpenCode
`ses_ef36871fdffeEAcmhED3BqZd4r`) avait laissé le streaming du chat à moitié
fait, formatage cassé et suite d'interface rouge. Travail repris, fini et
vérifié (25/25), puis la session a suivi l'usage.

### Le streaming du chat (5 octobre, soir)

Constat chiffré dans le journal du 18:22 : l'appel au modèle a pris **22,5 s**,
l'utilisateur a regardé trois points pendant tout ce temps, puis la réponse est
arrivée d'un bloc. Six sondes réelles ont établi le format SSE du fournisseur
(voir piège 74) : il streame, et proprement.

Le chemin implémenté : `chat_stream` (SSE, même `LlmReply` qu'avant) → la boucle
d'agent émet `AgentEvent::Delta` par fragment de contenu visible → l'interface
écrit la bulle au fil, avec un curseur clignotant. Deux règles gardent la
cohérence : **seul le premier essai streame** (les chemins de secours rejouent
sans flux, sinon ce qui est affiché serait écrit deux fois), et **une bulle en
flux coupée par un appel d'outil devient une ligne « annonce »** de l'activité
puis une bulle d'attente reprend — le texte d'une itération n'est pas la
réponse. `Final` remplace toujours la bulle par le texte complet : les filets
anti-dérive et inline-tools gardent tout leur effet.

Le raisonnement caché du modèle (`reasoning_content`) n'est jamais affiché ; sa
taille est au journal en debug. La voix reste accrochée à `Final` : lire les
fragments dirait les annonces. Voix anticipée dès la première phrase : différé.

Vérifié : parseur unitaire sur les fragments réels capturés (assemblage contenu
et tool_calls par index) ; `cargo test --workspace` sans avertissement ; test
réel `chemin_heureux_analyse_un_dossier` avec comptage des fragments reçus ;
`npm run build` strict ; suite d'interface.

Le même soir, le test réel a révélé un vrai bug sans lien avec le flux : une
conversation coupée en pleine procédure par un HTTP 400 (`messages[7]: "name"
is not supported by this endpoint`). Le champ `name` des résultats d'outil
n'est plus envoyé (redondant avec `tool_call_id`), test de régression à
l'appui. La suite d'interface, elle, a cassé sur le streaming : elle prenait la
disparition de la bulle d'attente pour la fin du tour (piège 75) et lisait un
fil pollué par l'écoute permanente (musique dans la pièce). Fin de tour
redéfinie, écoute coupée pendant la suite puis restaurée. Session reprise par
un autre agent après que le premier s'est perdu dans ses propres fichiers.

La reprise a trouvé la vraie cause de la cascade d'échecs (onze parcours
« Timeout 30000ms » à la suite) : un bug de l'application, pas des tests. La
fenêtre d'aperçu d'un fichier prenait le focus avant d'exister dans la page,
Échap ne la fermait donc jamais, et elle bloquait tous les clics suivants
(piège 76).

### Les fichiers sensibles sous autorisation (5 octobre, nuit)

En lisant le journal d'activité d'une conversation JobXpress, l'utilisateur a
vu Jimmy aller fouiller d'autres projets sur le VPS. Ça, c'était légitime :
l'API est déployée là-bas. Mais le journal montrait plus grave : le `.env` de
production réécrit par un script Python puis le conteneur reconstruit, sans
aucune question, et ce même `.env` affiché en clair une minute plus tôt.
Consigne de l'utilisateur : « il ne doit pas modifier des fichiers sensibles
sans autorisation ». Choix retenu parmi deux : une carte « Autoriser /
Refuser » dans le Chat qui suspend l'outil (plutôt qu'un refus sec qui
obligerait à tout faire à la main). À la voix, refus immédiat : personne ne
peut cliquer. Détail et limites au piège 77.

Le lendemain matin, l'utilisateur : « il demande trop l'autorisation ». Le
journal `[sécurité]` montrait onze demandes, toutes pour des lectures
(PowerShell, `findstr`, `ssh -i clé.key`) : la règle partait d'une liste de
commandes de lecture surtout Unix et jugeait suspect tout le reste. Elle est
inversée : seule une écriture repérée demande l'accord. Les onze commandes
réelles sont devenues un test.

### Muse Spark muet : les trois formats d'API (6 octobre)

L'utilisateur voulait Muse Spark 1.3 comme modèle ; il ne répondait pas. Une
sonde directe a donné la cause : `400 ModelProtocolUnsupported`. Le
fournisseur sert chaque modèle dans un seul format, et le catalogue le dit ;
Jimmy n'en parlait qu'un. Douze modèles du catalogue étaient inutilisables sans
que personne ne le sache, la bibliothèque les marquait simplement « cassés ».
Consigne : « tout type de format ». Les formats Responses et Messages ont été
sondés à la main (aller-retour d'outil, flux) avant d'écrire la traduction,
puis vérifiés par le client de Jimmy sur un vrai modèle de chaque format —
piège 78.

### La publication sur GitHub (6 octobre)

Demande : préparer le projet pour un dépôt **public**. Audit de tout
l'historique (pas seulement des fichiers actuels) : aucun secret, mais
l'adresse personnelle de l'auteur dans 27 commits, l'IP du VPS et le nom
d'une clé SSH dans un test, des chemins personnels un peu partout, et deux
chemins personnels **en dur dans le code** (dossier de travail par défaut,
dossier Godot). Décisions de l'utilisateur : auteur réécrit en adresse
`noreply` de GitHub, IP et nom de clé effacés de tout l'historique
(`git filter-repo`, sauvegarde en bundle avant), chemins anonymisés dans le
code et les docs, licence MIT, dépôt créé sous le compte connecté à `gh`
(Jimmyjoe13). Effet de bord à connaître : tous les identifiants de commit
ont changé ; les docs ont été recalées sur les nouveaux grâce à la table
`.git/filter-repo/commit-map`.

### Les docs remises à plat (6 octobre)

Relecture de toute la documentation contre le code : l'en-tête du HANDOFF
disait encore « rien n'a été commité » et visait un chantier livré la veille ;
le README annonçait 35 tests (151), un skin unique (cinq), une mémoire « par
hachage » (LM Studio d'abord) et l'ancienne voix par défaut ; les consignes
pointaient des secrets vers un chemin anonymisé. Tout est recalé ; la suite
d'interface du skin, qui échouait par un délai fixe pendant la relance de
Godot, attend désormais l'état réel.

### Ce que ces deux jours ont appris

- **Un garde-fou trop zélé est contourné ou désactivé** : onze demandes pour
  des lectures en une matinée. Une règle de sécurité se teste aussi contre
  l'usage réel, pas seulement contre l'attaque.
- **« Le modèle ne répond pas » peut être un problème de format**, pas de
  modèle : sonder l'API à la main avant de soupçonner le modèle.
- **Publier, c'est publier l'historique** : un secret retiré d'un fichier
  reste lisible dans un ancien commit.

---

## 4. Décisions structurantes de la session 2

| Décision | Raison | Où |
|---|---|---|
| Aucune nouvelle ressource externe pour l'avatar : normal map, ombre et sons **générés** | garder le choix « zéro asset à maintenir » | `jimmy.gd`, `main.gd` |
| Deux serveurs Whisper (`base` éveil, `small` commande) | `small` partout coûte +3 s à chaque fenêtre d'éveil | `App::ensure_stt` |
| Embeddings par LM Studio **avec** repli sur le hachage | la mémoire doit rester utilisable service éteint | `memory/semantic.rs` |
| Détection par trames, temps de l'audio et non de l'horloge | la transcription rapide ne doit pas retarder la fin de phrase | `voice/vad.rs` |
| Contexte audio réduit, **sur le seul serveur de commande** | réduit, il dégrade les mots courts du mot d'éveil | `stt.command_audio_ctx` |
| Conversation continue activée par défaut (8 s) | un échange naturel ne redit pas le nom à chaque phrase | `voice.follow_up_ms` |
| L'historique rechargé ne contient jamais d'outils | l'API refuse les séquences d'outils incomplètes | `History::conversation` |
| Un modèle n'est appliqué que s'il vient de répondre | « fonctionnel » doit vouloir dire vérifié | panneau Modèles |
| Ne pas modifier `.env` pour Synaptiq | utiliser l'identité d'un autre agent contournerait le cloisonnement | — |

---

## 5. Mesures de référence

| Mesure | Valeur | Contexte |
|---|---|---|
| Whisper `small-q5`, 1,8 s d'audio | 3,4 s | identique à 8, 12 ou 16 threads (4 threads : 4,7 s) |
| id. avec `-ac 640` / 512 / 768 | ~1,4 / 1,1 / 1,7 s | erreur de mots ~20 % dans tous les cas (8 phrases) |
| Whisper `base-q5` | 0,9 s | `-ac 768` : 0,4 s mais « Dis-moi bonjour » devient « D'y ma bonjour » |
| Niveau du micro intégré au repos / en parlant | 0,002 / 0,005–0,03 | RMS sur 600 ms |
| Micro intégré / casque | 48 kHz stéréo / 16 kHz mono | le flux est entrelacé dans les deux sens |
| Latence du modèle de langage | 2 à 25 s | même requête rejouée 14 fois : médiane 3,9 s, max 10,8 s |
| Première voix de synthèse | 1,2 à 4,6 s (une fois 13,6 s) | Fish Audio via OpenRouter |
| Appel au modèle, 3 k jetons en entrée | ~2 s au mieux | la taille du prompt n'a pas d'effet mesurable |

---

## 6. Ce que cette journée a appris sur la méthode

- **Reproduire avant de corriger.** L'erreur 400, le début de commande perdu,
  l'écoute muette : chacun a d'abord été reproduit par un test, qui échoue avant
  et passe après.
- **Mesurer avant de supposer.** Les trois « évidences » écartées par la mesure :
  les threads de Whisper, la taille du prompt, la requête couverte.
- **Tester la vraie application.** Les parcours d'interface pilotent la
  fenêtre réelle (CDP) ; ils ont attrapé le bug d'identifiant des modèles avant
  qu'il ne te touche. `scripts/test-ui.ps1` relance toujours Jimmy **sans** port
  de débogage à la fin.
- **Chercher l'appelant, pas la définition.** Quatre fois le même défaut : une
  fonction qui existe mais que rien n'appelle (écoute, changement de skin, MCP,
  qualité graphique des Paramètres).
- **Le format des données qui entrent et sortent de l'audio** (entrelacement,
  fréquence, canaux) a causé les deux plus gros bugs vocaux.
- **Ne pas tester pendant l'usage.** Les tests d'écoute et le build entrent en
  conflit avec une instance qui sert l'utilisateur.

---

## 7. Historique Git (20 commits, tous le 3 octobre)

| Heure | Commit | Contenu |
|---|---|---|
| 01:53 | `69754b8` | V1 : agent, avatar, voix, mémoire, skills, Synaptiq |
| 01:54 | `24a7931`, `43c2032` | HANDOFF ; base locale non versionnée |
| 10:39 | `93e11a2` | raccourci Bureau |
| 13:28 | `652a978` | synthèse vocale et écoute permanente réparées |
| 14:38 | `56430d6` | HANDOFF réécrit pour la suite |
| 15:04 | `79d83f0` | avatar : cadrage, AgX, occlusion, ombre au sol |
| 15:07 | `8479526` | proportions, contour, maillage par profil, fourrure |
| 15:12 | `d3db68c` | skins réels |
| 15:16 | `3dedf4f` | sons d'état |
| 15:20 | `d64ed54` | MCP en stdio |
| 15:21 | `3b49632` | HANDOFF |
| 15:34 | `5c652e9` | deux serveurs Whisper, embeddings LM Studio |
| 15:35 | `e99b4ab` | HANDOFF : décisions consignées |
| 16:33 | `14e86e6` | écoute réparée, interface corrigée |
| 18:11 | `ccf5906` | voix réparée à l'usage, esquive de l'avatar |
| 18:45 | `7b26272` | conversation vocale fluide, auto-réparation |
| 19:54 | `73eadd4` | détection par trames, conversation continue, phases visibles |
| 20:02 | `a1c9aa9` | bruit ambiant : prise abandonnée, seuil appris |
| 20:31 | `76c23b2` | erreur 400 corrigée, bibliothèque de modèles |

Depuis (identifiants **après** la réécriture de l'historique du 6 octobre ;
ceux du tableau ci-dessus ont été recalés de même) :

| Date | Commit | Contenu |
|---|---|---|
| 05/10 17:19 | `7186ff4` | Memoire longue : le vault Obsidian remplace Synaptiq |
| 05/10 17:19 | `39619c8` | Croissance : lecons apres echec, capture de competence, revue, amendements |
| 05/10 17:19 | `c448133` | Avatar : cycle de vie repare, skins ours et robot, regard vers la camera |
| 05/10 17:20 | `688a7a7` | Ecoute : hesitations et phrases longues, session persistante, arret STOP |
| 05/10 17:20 | `bd3dfab` | Agent, outils et noyau : budgets, MCP a la demande, tache longue, historique |
| 05/10 17:20 | `f20c98c` | Interface : Chat facon Codex, explorateur, bibliotheque de voix, serveurs MCP |
| 05/10 17:20 | `35edaa0` | Docs : INSTRUCTIONS/CONTRIBUTION/JOURNAL, HANDOFF a jour ; data et skills hors git |
| 05/10 18:06 | `6ec80da` | Sortie courte et nette : prompt concis et voix bornee |
| 05/10 21:52 | `fb34d46` | Streaming du chat, correctif HTTP 400 name, apercu ferme par Echap |
| 05/10 23:40 | `0f291b8` | Fichiers sensibles : aucune modification sans accord explicite |
| 06/10 11:06 | `a443786` | Fichiers sensibles : n'exiger l'accord que pour une ecriture reperee |
| 06/10 13:43 | `bb71b2b` | Preparation de la publication : licence MIT, chemins personnels anonymises |
| 06/10 16:21 | `d55e183` | Modeles : prise en charge des trois formats d'API (Chat, Responses, Messages) |
| 07/10 09:23 | `94ea557` | Tâches de fond : le Chat et la voix restent disponibles |
| 07/10 11:40 | `b9b2d42` | Tâches de fond : budget élargi et commandes d'inspection comptées en lectures |
| 07/10 15:37 | `3f0d3b3` | Vision : Jimmy voit la fenêtre active, sur demande de l'utilisateur seul |
| 07/10 22:18 | `a2950f6` | Tâches de fond : seuil à 45 s, et un lapin qui travaille à côté du renard |

---

## 3quinquies. Session 6 — 6 octobre, soir : avatar marquant et filet 3 min

Demande de l'utilisateur en deux temps : les animations de l'avatar selon
ses états sont trop discrètes (« on ne fait pas la différence »), puis un
message fantôme pendant les longues tâches (« Pas de réponse après
3 minutes » alors que Jimmy travaille encore).

### Audit avatar : pourquoi on ne voyait rien

Mesuré dans `POSES` (`godot/scripts/jimmy.gd`) : idle→listening = 8° de
tête, le reste à ±0,1 rad, le tout lissé par `POSE_SPEED = 7.0` — à 4 m de
caméra, ~10 px de différence. Et les états marqués n'étaient jamais émis :
`success`/`waiting` jamais, `error` que sur crash du Chat. Le quotidien,
c'était quatre poses quasi identiques.

### Lot B livré : poses ×2-3, quatre gestes, success/error branchés

- Poses amplifiées (tête listening −0,30, thinking yaw −0,55, bras success
  ±2,20, tête error +0,45…), oreilles 9°→16° plus fréquentes, queue ×1,5.
- Quatre gestes additifs et auto-extinguibles : `nod` à l'écoute, `perk` en
  réflexion, `cheer` (3 sauts) en succès, `shake` en échec. Ils survivent
  aux changements d'état : la joie joue par-dessus la parole, sans retarder
  la voix. `speak_for` ne retombe plus sur les états transitoires (sinon
  bras en V figés après chaque réponse).
- Relais Chat et voix (`commands.rs`) : `Success` après tâche outillée
  aboutie, `Error` sur échec fatal — jamais après un « STOP », jamais pour
  une réponse sans outil. `STOPPED_REPLY` rendue publique pour ce garde-fou.

Vérifié : 151 tests Rust, build release, snapshots des 8 états. Poses
distinctes à l'œil (face / profil / tête basse / bras levé : 6,5–13 % de la
région personnage contre 3,8 % de bruit) et geste prouvé en mouvement
(cheer : ~15 % entre deux images à 0,3 s). Leçon de mesure (piège 80) : la
bulle reste 12 s et le fond noir domine le % brut.

### Filet 3 min : l'erreur fantôme

Le filet était armé à l'envoi et jamais réarmé : tâche longue mais vivante →
erreur, puis vraie réponse dans une autre bulle. Corrigé par réarmement à
chaque signe de vie (`pokeSafety`) + drapeau `safetyOn` (piège 79).
`npm run build` strict OK.

### test-ui 25/26, deux fois : le fournisseur, pas le code

Le parcours « fichier sensible » a expiré 2× (150 s sans carte). Le journal
a tranché : aucun appel modèle ni outil entre l'envoi et le rappel mémoire,
2 min 25 de vide — le fournisseur n'a pas répondu à temps. Repro ciblée le
même soir : carte en <5 s, refus respecté, aucun fichier écrit. Mécanisme
sain ; à surveiller (point 7 de « Ensuite » dans le HANDOFF).

Constat au passage : `HANDOFF.md` contient des `é` doublement encodés
(« passÃ©s » en bytes) sur certaines lignes — probablement un script de
ré-encodage passé le 6 octobre. Laissé tel quel (un changement = un besoin),
mais les futurs ajouts s'ancrent sur de l'ASCII pour ne pas casser l'outil
d'édition.

## 3sexies. Session 7 — 6 octobre, soir : skills à coût constant (D+B+C)

Demande : Jimmy crée des skills en autonomie — le contexte de base ne va-t-il
pas exploser ? Analyse d'abord (sans toucher au code) : validée en tendance.
32 skills dont 31 capturés en 2 jours (~15/jour) ; catalogue injecté à chaque
appel = ~7 300 caractères ≈ 1 800 jetons (~37 % du prompt) ; +900 par jour.
Les corps (52 Ko) ne coûtent rien (`read_skill` à la demande). L'aiguisage
prévu ne s'est jamais déclenché (0/31) : le modèle préfère toujours créer.

Livré (go de l'utilisateur : D+B+C + existants si nécessaire) :
- **D** : catalogue retiré du prompt (`catalogue()` supprimé) ; restent 3 noms
  suggérés + phrase vers `list_skills`/`read_skill`. Coût skills constant.
- **B** : ligne `proche:` obligatoire et vérifiée + fusion forcée à 0,6.
  La similarité lexicale seule fusionnerait à tort (0,56 mesuré sur des sujets
  différents) : calibré sur le corpus réel AVANT de coder, seuil strict.
- **C** : `skills/demand.rs` — première occurrence notée sans capture
  (`data/skill_demand.json`, cap 300), capture dès la 2e. Mots > 3 lettres,
  3 communs + recouvrement 0,5 minimum.
- **Existants** : `suggest` cherche aussi dans les corps (rappel sans jeton) ;
  aucune paire à 0,6 dans les 32 → rien à fusionner ; descriptions gardées
  (utiles au rappel, gratuites depuis D).

Vérifié : 8 nouveaux tests unitaires verts (`cargo test` 0 échec, 0 warning),
build release, test-ui 25/26 — seul le parcours sensible échoue (3×, modèle
qui répond sans outil ; 3 repros ciblées OK, voir point 7 de « Ensuite »).

Deux incidents d'outillage, sans suite : un `}` orphelin laissé par l'éditeur
(rattrapé par `cargo test`, pas par relecture) et un `#[test]` volé au test
voisin lors d'une insertion (3 warnings, réparés). Règle rappelée : après
toute insertion entre deux tests, vérifier les attributs voisins.

## 3septies. Session 8 — 6 octobre, soir : le bras caché (piège 81)

Retour d'usage : un bras de Jimmy disparaît derrière le corps. Cause lue
dans le code, pas devinée : `_arm_l` nie la pose, `_arm_r` la garde brute —
avec des valeurs symétriques les deux bras penchaient du même côté, et le
droit traversait le torse. Corrigé en miroir partout (sauf thinking, dont
la main-au-menton traversait aussi : remplacée par un bras visible), V de
success 2,20 → 1,90 (les bras touchaient les oreilles), cheer ±0,55 pour
ne pas croiser les mains. Vérifié par snapshots : V ouvert à deux bras,
profil à deux bras. L'avatar a été rechargé deux fois par le chien de garde
(tuer son Godot), Jimmy n'a jamais été arrêté.

## 3octies. Session 9 — 7 octobre : les tâches de fond

Demande : que Jimmy garde le chat disponible en lançant les tâches en
arrière-plan. Lecture d'abord : le backend du Chat tournait déjà en tâche de
fond, le blocage venait de l'interface (envoi refusé pendant un tour),
d'événements sans identifiant de tâche, d'un « STOP » global, et surtout de
la voix, dont la boucle d'écoute attendait la fin de chaque tâche. Plan
proposé puis validé avec quatre choix : passage automatique, réponse en
parallèle, « STOP » = premier plan (« arrête tout » = tout), une seule tâche
de fond.

Livré : registre `tasks.rs`, `App::start_task` commun au Chat et à la voix
(relais qui décide du passage en fond et enveloppe les événements), bloc du
prompt, commandes `tasks_list`/`task_stop`, bandeau du Chat, annonce de fin
à voix haute. Testé d'abord sans modèle (tâches simulées, `--test
background`), puis en vrai : `test-ui` a montré le passage en fond 15 s après
l'outil, une question « cerise » répondue pendant ce temps, et la fin
« fini-fond » arrivée dans le fil.

Le test réel a aussi corrigé la règle (piège 82) : comptées depuis la
demande, les 15 s faisaient partir en fond une tâche dont le premier appel au
modèle avait pris 26 s, au moment même où elle demandait une autorisation.
Le délai part maintenant du premier outil, et jamais pendant une carte
d'autorisation.

Premier usage réel le matin même : « il reste bloqué depuis deux prompts ».
Le journal disait autre chose — chaque demande tournait 6 à 7 minutes en
fond puis concluait « reste à faire » : 25 étapes d'inspection (clés SSH
essayées une à une, la bonne trouvée à l'étape 17), plus d'étapes pour agir.
Deux corrections, validées par l'utilisateur : budget de fond 60 étapes /
20 min, et les commandes d'inspection comptées comme lectures pour que le
rappel « agis ou conclus » parte enfin (piège 83, mesuré sur les 63
commandes réelles de la session).

### La vision de la fenêtre active (7 octobre, après-midi)

Demande : que Jimmy voie ce que fait l'utilisateur, seulement à sa demande.
Vérifié avant tout code : les deux modèles du quotidien acceptent l'image
(catalogue), puis une sonde brute dans les trois formats — bloquée d'abord
par Cloudflare et l'en-tête de session (piège 84), ensuite « rouge, bleu »
quatre fois sur quatre. Choix de l'utilisateur : déclenchement par lui seul
(bouton ou phrase, aucun outil pour le modèle), fenêtre active seule.
Livré en trois lots : image dans les messages (trois formats), capture
`xcap` qui écarte Jimmy et son avatar, déclencheurs Chat et voix avec
vérification du modèle. L'image ne touche jamais le disque.

### Le juste milieu et le lapin (7 octobre, soir)

« Il lance toutes ses tâches en fond, même les légères. » Le journal l'a
confirmé et expliqué : la règle du 3e outil se déclenchait 0 à 5 s après le
premier, Muse Spark appelant plusieurs outils d'un coup. Seul le temps
compte désormais : 45 s après le premier outil. Et une envie de
l'utilisateur : un lapin qui apparaît à côté du renard et travaille pendant
la tâche de fond. Construit en primitives, ajusté sur snapshots (cadre,
engrenage de face, logo visible, bras en V et oreilles tombantes lisibles :
les premières versions des poses ne se voyaient pas, comme celles du renard
au début).

Puis : « je n'ai pas vu le lapin ». L'utilisateur avait la bonne intuition
(les mains du renard en V étaient coupées par le bord) : la zone cliquable
de la fenêtre découpe aussi l'affichage sous Windows, et ne couvrait que le
corps du renard. Les snapshots, pris avant la découpe, montraient tout —
d'où la nouvelle règle de vérifier sur l'écran réel (piège 85).

### Le navigateur (7 octobre, nuit)

Choix de l'utilisateur : Jimmy a son propre Chrome mais peut s'y connecter à
ses comptes et naviguer en son nom. Serveur MCP Playwright, profil gardé,
connexion faite par l'utilisateur lui-même, et une carte d'autorisation
avant tout ce qui engage (envoyer, payer, publier, supprimer). Le test réel
a montré que les actions de Playwright ne renvoient pas la page (piège 86) :
Jimmy prend un instantané après chaque action. Liste des mots d'engagement
resserrée avant livraison (« book » visait Facebook, « command » le lien Mes
commandes).

## 3novies. Session 10 — 8 octobre : la configuration LLM multi-fournisseurs

Demande de l'utilisateur : « migrer toute la configuration LLM dans un
sous-onglet des Paramètres, puis connecter plusieurs fournisseurs — OpenRouter,
Alibaba, DeepSeek, Claude, et OpenCode comme actuellement ». Reformulée et
validée avant le code (les trois points de décision acceptés : clés saisies
dans l'interface, fournisseur personnalisé, refonte de la bibliothèque).

**Le sous-onglet.** Paramètres devient « Général | LLM », sur le modèle des
sous-onglets Skills → Serveurs MCP. Le pane LLM rassemble ce qui était éparpillé
(cartes Modèle de langage et bibliothèque) **et** ce qui n'avait jamais eu
d'interface : `max_tokens` et `max_iterations`, désormais éditables dans une
carte « Budget de l'agent ».

**Les fournisseurs.** Cinq intégrés (OpenCode Go, OpenRouter, DeepSeek, Alibaba
Qwen, Anthropic Claude) plus le bouton « ajouter » pour une API au choix (LM
Studio, entreprise). Chacun porte son URL de base, sa clé, son format d'API
(les trois formats de `protocol.rs` sont enfin réutilisés hors d'OpenCode) et
ses propres en-têtes : Claude veut `x-api-key` sans Bearer, OpenCode Go veut
`x-opencode-session`, les autres juste Bearer. Le modèle principal et le modèle
vocal peuvent vivre chez des fournisseurs différents — le routeur attache
chaque modèle au sien à chaque enregistrement des paramètres.

**La migration.** La `config.json` d'avant n'a pas de liste de fournisseurs :
au chargement, les cinq intégrés sont posés et l'ancien `base_url` (qui pointait
peut-être vers un proxy) est recopié chez OpenCode Go. Testé. Une clé n'est
jamais renvoyée côté interface : seule la commande dédiée l'écrit, et une clé
vide venant du formulaire global veut dire « inchangée » (piège 89).

**Payé en route (piège 88).** Le parcours UI « fournisseurs » échouait sur une
interface parfaite : `hasText: "Anthropic"` matchait les cinq lignes parce
qu'une `<option>` des sélecteurs contient le mot « (Anthropic) ». Leçon de
sélection, pas de code.

**Le plan Claude (suite de la demande).** Deuxième demande du 8 octobre : se
connecter chez Anthropic **avec l'abonnement**, pas seulement une clé API.
Choix technique : réutiliser la session OAuth de Claude Code
(`~/.claude/.credentials.json`) plutôt qu'un faux clonage — Jimmy lit le jeton,
le rafraîchit lui-même à l'expiration et réécrit le fichier en préservant le
reste, les deux outils partagent la session. Le module `providers/claude_plan.rs`
est testé sans réseau (lecture, fraîcheur, réécriture qui ne perd rien, en-têtes
sans `x-api-key`). **Mesuré sur le vrai abonnement** (`--test claude_plan --
--ignored`) : `/v1/models` répond (14 modèles), Haiku works avec outils en
559 ms — mais Sonnet et Opus renvoient un 429 nu, la grille Anthropic réservant
le premium au client Claude Code (piège 90). Décision assumée : on n'imite pas
Claude Code ; l'abonnement sert les modèles légers, les gros gardent la clé API.

**Précision du 9 octobre — le plan ne porte pas Jimmy en agent.** L'utilisateur
a activé l'abonnement, choisit Haiku : le « Tester » passe, le chat réel échoue
en `400 « Third-party apps now draw from extra usage, not plan limits »`.
Bissection réelle (`--test claude_plan`, matrice de cas) : le déclencheur n'est
**ni le modèle premium, ni le volume, ni le flux** — 21 621 jetons de prompt
sans outils passent — mais le **poids de la déclaration d'outils** : 12 outils
réalistes (~3 000 jetons) échouent même avec un message de 15 jetons, 12 outils
minuscules passent. Le test de Jimmy déclarant un seul outil factice, il était
vert ; son chat en déclare quinze, il est rouge. Anthropic bascule les vraies
requêtes d'agent tierces sur un solde payant « extra usage », non activé ici
(piège 90). Livré en réaction : messages d'erreur premium/extra-usage traduits
en français actionnable (refus local avant appel pour Sonnet/Opus ; message
explicite avec les trois issues pour le refus de facturation), garde-fous au
choix du modèle (interface + commande), revérifiés sur l'application réelle.
**En l'état, l'abonnement seul ne peut pas servir de moteur à Jimmy** — sans
activer « extra usage » chez Anthropic ou sans clé API ; OpenCode Go reste le
chemin qui marche.

**Résolu le soir même, à la demande explicite de l'utilisateur** (« je veux
l'accès aux modèles Anthropic avec mon plan, copie la config d'OpenCode si
nécessaire »). Nouvelle sonde : à requête égale, 15 outils nommés
`read_file`/`run_command` = 400 ; les mêmes outils rebaptisés
`Read`/`Write`/`Bash`… = 200 — descriptions et User-Agent Jimmy conservés. La
grille est une liste noire de signatures de frameworks tiers. Jimmy prend donc
la signature Claude Code complète : bloc de facturation en tête du système +
mapping bidirectionnel des noms d'outils (`map_plan_tool_names` /
`unmap_plan_tool_calls`, piège 92). Preuve livrée : vrai tour d'agent sur
`claude-sonnet-4-5` via le plan — fichier créé par l'outil, réponse « Le
fichier a été créé. » ; suite d'interface **28/29 sur le plan** (l'échec
restant, la tâche de fond, est une course de minuterie liée à la latence du
plan, à re-mesurer). Au passage, ma réécriture manuelle du `config.json` avec
BOM avait corrompu la configuration (retour aux valeurs par défaut, six serveurs
MCP effacés, piège 91) : restaurée depuis le `.bak` de la veille, `navigateur`
re-créé, la suite verte preuve que tout est revenu. Risque consigné :
durcissement ou révocation côté Anthropic ; la bascule clé API reste un clic.

Vérifié (état final du 9 octobre) : 194 tests unitaires (0 avertissement)
dont mapping des noms d'outils dans les deux sens et facturation injectée ;
TypeScript strict ; `build.ps1 -Release` ; test ignoré `le_premium_passe_par_
signature_claude_code` vert (`claude-sonnet-4-5` : ok=true outils=true) ;
**suite d'interface complète relancée sur le plan (moteur Sonnet via
abonnement) : 28/29** — l'unique échec est le parcours « tâche de fond »
(minuterie serrée, à re-mesurer, sans lien avec la signature). Le plan Claude
n'a plus besoin d'« extra usage » : la signature Claude Code le remplace,
configurée en commande Tauri (pièges 91-92). Les nouveaux fournisseurs
DeepSeek/Alibaba n'ont toujours pas été essayés avec de vraies clés.

## 8. Reste à faire et questions ouvertes

0. **État au 9 octobre (soir)** : l'abonnement Claude sert Jimmy comme moteur
   (Sonnet en principal, Haiku en vocal) grâce à la signature Claude Code —
   bloc de facturation + alias de noms d'outils (pièges 90 et 92) ; vrai tour
   d'agent outillé vérifié, suite 28/29 sur le plan. Risque assumé par
   l'utilisateur : durcissement Anthropic possible, réversible en un clic
   (clé API ou OpenCode Go). **État au 8 octobre** : configuration LLM en sous-onglet + cinq fournisseurs
   (dont personnalisés), clés saisies dans l'interface, et **abonnement Claude**
   (session Claude Code partagée, Haiku vérifié en réel) — vérifié par tests et
   suite d'interface ; restent les usages réels DeepSeek/Alibaba avec de vraies
   clés (HANDOFF, « Ensuite » 6).
   **État au 7 octobre** : tâches de fond, vision, SynaptiQ par MCP, lapin
   et navigateur livrés (section 3octies) ; prochaines étapes par priorité
   dans le HANDOFF (« Ensuite (au 7 octobre) »).
   **État au 6 octobre** : tout est commité ; dépôt **public**
   github.com/Jimmyjoe13/jimmy-agent (licence MIT). Reste à **régénérer les
   clés exposées** (`aggregate` dans `jimmy.log`, `skillsmp` dans
   `data/config.json`, `.env.api` de JobXpress lu en clair par le modèle le
   5 octobre) et à masquer les secrets dans les résultats d'outils. La
   sortie courte reste à confirmer en usage réel, comme Muse Spark 1.3
   (format Responses) en conversation.
1. **Utiliser Jimmy au quotidien à la voix** pendant une semaine : c'est le seul
   test qui répond à la question du PLAN (la voix apporte-t-elle quelque chose ?).
   Vérifier surtout : plusieurs questions d'affilée, une exploration longue avec
   outils, le panneau des modèles.
2. **Relire les notes que Jimmy écrit dans le vault** (`0_Inbox/Jimmy`) : dire
   si le tri du dossier est le bon, et si ses souvenirs sont utiles ou bavards.
3. ~~Pousser sur le dépôt distant~~ : publié le 6 octobre (section 3quater).
4. ~~Tester un vrai serveur MCP du catalogue~~ : fait le 4 octobre (obsidian,
   aggregate, skillsmp, ajoutés par Jimmy lui-même). Reste : charger leurs
   outils à la demande (~13 k jetons de définitions par appel).
5. **Un modèle plus rapide pour la voix** : le panneau le permet ; reste à
   choisir lequel, en tenant compte que le fournisseur est chargé par moments.
6. Pistes non faites : un petit modèle local (LM Studio) pour les échanges
   courts ; MCP en HTTP ; export Godot (~1 Go de gabarits) pour alléger
   l'installateur ; mise à jour automatique ; visage plus expressif et pieds /
   mains moins « billes » pour l'avatar.
7. **Limite à connaître** : pendant la conversation continue, tout ce qui est dit
   est pris pour une commande. Se règle ou se coupe dans Paramètres → Écoute.

---

## 9. Annexes

**Rouvrir les sessions**

- OpenCode (la V1) : `opencode -s ses_f0143431fffevk3YBOk6F7BJPM`
- Données brutes : `~/.local/share/opencode/opencode.db` (tables `session_v2`,
  `session_message`). L'export en ligne de commande de cette version d'OpenCode
  n'accepte pas l'identifiant en argument : la base a été lue directement, en
  copie, sans la modifier.

**Fichiers clés**

| Fichier | Rôle |
|---|---|
| `agent/src/core/agent.rs` | boucle de l'agent (outils, limites, conclusion) |
| `agent/src/core/history.rs` | historique complet et `conversation()` |
| `agent/src/voice/listener.rs`, `vad.rs` | boucle d'écoute et détecteur de parole |
| `agent/src/providers/llm.rs` | client du modèle, choix du format, liste et test des modèles |
| `agent/src/providers/protocol.rs` | formats d'API Chat, Responses, Messages (corps, réponses, flux) |
| `agent/src/sensitive.rs` | garde-fou des fichiers sensibles (détection, carte d'autorisation) |
| `desktop/src/views/models.ts` | panneau des modèles |
| `godot/scripts/main.gd`, `jimmy.gd` | scène, esquive, zone cliquable ; personnage et skins |
| `scripts/test-ui.ps1`, `scripts/ui-test/` | parcours de l'interface sur la vraie application |
| `data/logs/jimmy.log` | journal de la release (niveaux, durées, textes entendus) |

**Commandes** : voir la section « Commandes » du `HANDOFF.md`
(`build.ps1 -Release`, `test-ui.ps1`, `test-happy.ps1`, `with-msvc.ps1 cargo test`).
