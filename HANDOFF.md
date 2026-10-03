# HANDOFF

État du prototype au **3 octobre 2026**.

**Objectif de la prochaine session :** retravailler la **qualité graphique de
l'avatar**, puis les **fonctionnalités manquantes**. Les deux sections
« Priorité 1 » et « Priorité 2 » ci-dessous sont l'état réel, lu dans le code —
pas une liste d'idées.

---

## Ce qui est vérifié

| Domaine | Comment c'a été vérifié |
|---|---|
| Compilation complète | `scripts/build.ps1 -Release` → `code=0`, 0 avertissement |
| Tests unitaires | 35 tests, tous verts (`cargo test --workspace`) |
| Agent de bout en bout | `scripts/test-happy.ps1` — vrai modèle, outils réels, réponse FR |
| Synthèse vocale | `cargo test --test audio tts` → 225 280 octets PCM 44 100 Hz, **lu sans erreur** |
| Écoute permanente | `cargo test --test audio ecoute` → micro ouvert, `whisper-server` démarré |
| Avatar 3D | capture : renard rendu, fenêtre transparente sur le bureau |
| États de l'avatar | `POST /state` pour les 8 états, rendu vérifié |
| Bulle de dialogue | `POST /say` avec accent → texte affiché correctement |
| Clic sur l'avatar | `POST /ui/open` → fenêtre ouverte (handle visible, capture) |
| Fenêtre principale | masquée au démarrage **par conception** (`visible: false`), ouverte au clic |
| Installateur | `scripts/install.ps1 -NoBuild` → 8 vérifications, sortie 0 |
| Frontend | `npm run build` → TypeScript strict sans erreur |
| Godot | `--headless` → aucun script en erreur |
| Binaire release | contient `response_format`, **ne contient plus** l'ancien message MP3 |

---

# Priorité 1 — Qualité graphique de l'avatar

## État réel, mesuré dans le code

`godot/` pèse **0,03 Mo, 14 fichiers**. `godot/assets/` et `godot/skins/` sont
**des dossiers vides**. Il n'existe **aucun asset externe** : tout est
procédural. C'est un choix assumé (zéro fichier à maintenir), mais c'est aussi
la cause racine de la platitude visuelle.

**Géométrie** — primitives brutes, `godot/scripts/jimmy.gd:243-287` :
`_sphere`, `_capsule`, `_cone`, `_box`. La tessellation est **codée en dur et
basse** : capsules 16 segments / 6 anneaux, cônes 16 / 4, sphères 12 à 24
(`jimmy.gd:246-266`).

**Matériaux** — 7 `StandardMaterial3D` dans `_make_materials()`
(`jimmy.gd:224-241`) : couleur unie + `roughness` (0,30 à 0,95) +
`metallic_specular = 0.35`. **Aucune texture, aucune normal map, aucune
variation.** C'est la limite n°1 du rendu.

**Profils graphiques** — `godot/scripts/main.gd:15-19` ne pilotent que cinq
choses :

| | low | medium | high |
|---|---|---|---|
| `scaling_3d_scale` | 0,62 | **0,85** | 1,0 |
| `msaa_3d` | désactivé | 2× | 4× |
| ombres | non | oui | oui |
| glow | non | non | oui |
| `Engine.max_fps` | 30 | 60 | 60 |

Conséquence à assumer : **`high` n'est pas plus fin, seulement plus net et
plus lumineux.** Aucun preset ne touche la géométrie ni les matériaux. Et en
`medium`, la 3D est rendue à 0,85 puis agrandie — d'où le côté légèrement
flou en profil par défaut.

**Rendu** — `main.gd:91-97` : fond transparent, lumière ambiante couleur,
tonemapper **FILMIC**. **Pas d'occlusion ambiante, pas de SDFGI, pas
d'ajustements (contraste/saturation), pas de brouillard.**

**Éclairage** — 3 sources (`main.gd:104-126`) : une `DirectionalLight3D`
(clé, ombres) + deux `OmniLight3D` (contre-jour bleu, remplissage chaud). La
boucle de preset met `shadow_enabled` sur **les trois**
(`main.gd:332-334`), sans configurer quoi que ce soit pour les omni : coût
pur, aucun bénéfice.

**Pas de sol.** Le fond est transparent : l'ombre ne peut pas se poser. Elle ne
produit que de l'auto-ombrage. Le personnage flotte donc au-dessus du bureau
sans point de contact.

`use_taa = false` est câblé en dur (`main.gd:330`), alors que le MSAA est
justement faible sur un fond transparent.

## Leviers, classés par rapport gain / effort

1. **Occlusion ambiante** — un bloc de config dans `main.gd`. Pour un
   personnage fait de primitives empilées, c'est *le* levier : aujourd'hui le
   bras, la queue et les jambes lisent comme des autocollants flottants, sans
   contact. Le meilleur gain par ligne de code.
2. **Ombre de contact au sol** — une ellipse sombre sous les pieds. Puisqu'il
   n'y a pas de sol, aucune vraie ombre ne peut atterrir. C'est ce qui ancre le
   personnage sur le bureau.
3. **Tonemapper** — FILMIC désature et grise les aplats. Passer à ACES ou
   LINEAR + un ajustement de saturation. Deux lignes, effet couleur immédiat.
4. **Contour (inverted hull)** — le style qui rend un personnage procédural
   lisible au-dessus d'un bureau chargé.
5. **Ombres des omni** — soit les configurer, soit les exclure de la boucle
   `main.gd:332-334`. Aujourd'hui : coût sans retour.
6. **Tessellation liée au profil** — faible gain (le personnage fait ~150 px
   de large), mais elle rendrait l'affirmation « 3 profils » honnête.
7. **Matériaux** — la fourrure veut une normal map ou un shader de bruit, et
   de la variation de rugosité. **Ce serait le tout premier asset du projet** :
   à faire quand la structure d'assets sera décidée.
8. **TAA** — `use_taa` câblé en faux ; utile sur les bords alpha.

## Animation : ne pas repartir de zéro

Les 8 états sont dans `POSES` (`jimmy.gd:28-78`) et `_apply_pose`
(`jimmy.gd:169-219`) anime ~15 canaux : respiration, balancement, mâchoire,
clignement, suivi du curseur, remuage en cascade de la queue. Ajouter un état
ne demande **qu'une entrée dans `POSES`** — c'est le point d'extension prévu
par le PLAN. Ne pas réécrire ce système pour gagner en qualité graphique.

---

# Priorité 2 — Fonctionnalités

État réel, vérifié dans le code :

| Manque | État réel | Où l'attaquer |
|---|---|---|
| **Peaux** | **Inopérant.** `/skin` ne fait qu'`print` et ne reconstruit rien (`main.gd:237-241`). `skins/` est vide. | reconstruire la scène, pas seulement changer une variable |
| **Sons d'état** | **Aucun.** Pas de dossier `assets/` à la racine. | 3 fichiers : écoute, réponse, erreur |
| **MCP** | Config seule. `agent/src/mcp/mod.rs` (12 Ko), `mcp_servers: []`, types déclarés dans `api.ts:187,206`, **aucune commande, aucun transport HTTP** | transport HTTP + `mcp_add_server` |
| **Mémoire vectorielle** | `memory/embed.rs` = *hashing trick* (FNV, constante `0xcbf29ce484222325`). Rapproche les mots, pas les synonymes. | `MemoryStore` |
| **STT `small-q5`** | **Présent sur disque** (181 Mo) et **sélectionnable** (`stt.rs:25`), mais le défaut reste `base-q5` (57 Mo) | deux serveurs, ou rechargement de modèle |
| **Permissions** | Globales (LECTURE / MODIFICATION / EXÉCUTION / RÉSEAU). Pas de raffinement par outil ou par service | la structure de règles est déjà là |

Outils d'agent déjà en place : `cli.rs`, `fs.rs`, `knowledge.rs`, `net.rs`,
`skills.rs`.

---

## Pièges connus

**1. PowerShell 5.1 lit les `.ps1` en ANSI sans BOM.**
Tout fichier `.ps1` contenant des accents doit être écrit en **UTF-8 avec
BOM**. Sans BOM, un tiret cadratin (0xE2 0x80 0x94) est relu comme `"` (0x94 en
CP1252) et casse la chaîne. Symptôme : le script s'arrête silencieusement après
la première ligne contenant un tiret. *Les huit scripts de `scripts/` ont le
BOM — revérifié le 3 octobre 2026.* Un script de ré-encodage en ajoute un
second : c'est ainsi qu'un BOM s'est glissé dans un message de commit.

**2. `curl` et PowerShell cassent les accents dans un corps JSON.**
`curl.exe -d '{"text":"café"}'` produit un corps invalide. Écrire le JSON dans un
fichier UTF-8 et utiliser `--data-binary @fichier`.

**3. `tauri::State<'static, T>` casse la compilation.**
Avec un alias `type Shared = State<'static, AppState>`, toutes les commandes
échouent avec `__tauri_acl__ does not live long enough`. Écrire directement
`State<'_, AppState>` dans chaque signature.

**4. Le dossier Godot porte le nom de l'exécutable.**
`C:\Dev\Godot\Godot_v4.5.1-stable_win64.exe` est un **dossier**, pas un fichier.
`paths::find_godot_exe` explore donc les dossiers comme les fichiers. Ne pas
« corriger » ce chemin.

**5. Le modèle `base` transcrit « Jimmy » en « J'y mise ».**
Ce n'est pas un bug. `voice::KNOWN_PHRASES` liste les trois formes observées,
et un test unitaire (`pas_de_faux_positif_sur_jai`) garantit que « j'ai besoin »
ne réveille pas Jimmy.

**6. Un seul `whisper-server` à la fois.**
Il occupe le port 8178. Une instance de Jimmy lancée en arrière-plan empêche le
démarrage. Vérifier avec `Get-Process whisper`.

**7. `frontendDist` ne doit apparaître qu'une seule fois.**
Un doublon dans `tauri.conf.json` fait gagner `devUrl` au build release : la
fenêtre affiche « Impossible d'accéder à cette page ». Symptôme trompeur, car le
démarrage paraît correct et seule l'interface manque.

**8. L'avatar ne doit pas dépendre du frontend.**
Il est lancé dans `setup()` (Rust), pas dans la commande `bootstrap`. La fenêtre
peut mettre du temps à charger, ou échouer : Jimmy doit malgré tout être sur le
bureau.

**9. Fish Audio ne produit pas de WAV.**
`response_format: "wav"` renvoie **400**. Les deux formats acceptés sont `mp3`
et `pcm`. Jimmy demande `pcm` : le flux est déjà dans le format que la carte son
consomme, donc aucun décodeur MP3 à embarquer. `providers::tts::parse_rate` lit
la fréquence dans l'en-tête `audio/pcm;rate=44100;channels=1` ; si elle est
absente, elle vaut 0 et le lecteur ne rééchantillonne pas. Le MP3 n'est plus
supporté — `decode_audio` le refuse explicitement plutôt que de produire du
bruit.

**10. `build_output_stream` ne joue rien tant que `.play()` n'est pas appelé.**
Le piège le plus coûteux de la session dernière : le code construisait le flux,
puis attendait `done` dans une boucle. Sans `.play()`, la callback n'est jamais
invoquée, `done` ne passe jamais à `true`, et **la lecture bloque
indéfiniment** — sans message d'erreur. Le test TTS est resté bloqué plus de
15 minutes. Symptôme reconnaissable : le processus consomme un cœur et rien ne
se passe. Il y a maintenant un filet de sécurité : durée réelle majorée d'une
seconde, puis abandon avec `log::warn`.

**11. Une commande Tauri qui existe n'est pas une commande appelée.**
`voice_start` / `voice_stop` étaient implémentées côté Rust, exposées dans
`api.ts`… et jamais invoquées. Résultat : le micro n'était jamais ouvert,
`whisper-server` jamais lancé, et le wake word muet — sans la moindre erreur
visible. Vérifier avec `rg 'api\.voiceStart' desktop/src` : un résultat = un bug.

**12. `cargo test --test audio` doit utiliser les vrais chemins.**
Un `Paths` pointant vers un dossier temporaire ne trouve ni `whisper-server.exe`
ni les modèles : le test échoue sur « composant introuvable » alors que
l'installation est correcte. `app_reel()` cible le workspace ; `app_de_test()`
(dossier temporaire) ne sert qu'aux tests hors processus.

**13. `guard()` renvoie `undefined` pour une action `void` — dans les deux cas.**
`guard` retourne `T | undefined`. Quand `T` vaut `void`, impossible de distinguer
« réussi » de « échoué ». Le code
`if (!(await guard(() => recorder.start()))) return;` faisait donc **toujours**
sortir, et le bouton « Enregistrer » ne passait jamais à l'étape suivante : le
micro s'ouvrait, le texte ne s'affichait pas.

`ui::attempt` existe pour ça : il renvoie un booléen. **Règle : dès que la
commande Tauri ne retourne rien, utiliser `attempt`, jamais `guard`.**

**14. Un « exit code 1 » sans erreur n'est pas forcément une erreur.**
Les commandes de ce projet sont souvent pipées vers `Select-Object -Last N` ou
`-First N` : PowerShell ferme alors le pipeline en amont, la commande native est
tuée, et le code de sortie vaut 1 alors que la compilation a réussi. Redirecter
vers un fichier (`> log 2>&1`) puis lire le log, pour un verdict fiable.

**15. Une route qui répond `{ok:true}` n'a pas forcément fait le travail.**
`POST /ui/open` répond `{"ok":true}` même quand `window.show()` échoue — le
résultat est ignoré (`bridge.rs:28-30`). Toujours vérifier l'état réel de la
fenêtre, pas la réponse HTTP.

**16. Cliquer une fenêtre Tauri depuis un script est piégé, et c'est bien.**
`SetForegroundWindow` échoue depuis un processus de fond ; injecter un clic
alors que le focus n'est pas confirmé revient à cliquer dans la fenêtre du
derrière — ici la messagerie du travail. **Toujours vérifier
`GetForegroundWindow() == handle` avant tout clic simulé, et abandonner sinon.**
Pour capturer une fenêtre masquée, utiliser `PrintWindow(h, hdc, 2)` : il rend
la fenêtre même occultée, sans passer par le premier plan.

---

## Prochaines étapes

### Priorité 1 — l'avatar doit cesser de ressembler à un assemblage de primitives

1. **Occlusion ambiante + ombre de contact au sol.** Le plus gros gain
   perceptible, quasi tout dans `main.gd`. Reprendre `QUALITY_PRESETS`
   (`main.gd:15-19`) pour que l'AO se dégrade avec le profil.
2. **Tonemapper + saturation.** Abandonner FILMIC pour un rendu qui garde
   l'orange du renard.
3. **Contour** et **densité de maillage liée au profil**, pour que `high` soit
   réellement plus fin.
4. Décider si l'on introduit un premier asset (normal map de fourrure). C'est
   le moment de créer une vraie structure `godot/assets/`, aujourd'hui vide.

### Priorité 2 — les fonctions

5. **Sons d'état** : trois petits fichiers, fort effet sur le ressenti.
6. **Rendre le changement de peau réel** : `/skin` doit reconstruire la scène.
   C'est ce qui débloquera les skins sans toucher à Rust.
7. **MCP** : transport HTTP + `mcp_add_server`.
8. **Mémoire** : vrai modèle d'embeddings dans `embed.rs`.
9. **Brancher `small-q5`** par défaut pour la transcription de commande.

### À faire en parallèle

10. **Utiliser Jimmy au quotidien une semaine.** C'est le seul test qui compte
    pour la question du PLAN : est-ce que la voix apporte quelque chose ?

### Différé

11. Export Godot (~1 Go de gabarits) pour que l'installateur n'installe pas le
    moteur complet.
12. Mise à jour automatique, quand il existera une distribution.

---

## Commandes

```powershell
.\scripts\install.ps1          # vérifications
.\scripts\dev.ps1              # lancer Jimmy (développement)
.\scripts\build.ps1            # compiler (debug)
.\scripts\build.ps1 -Release   # release — c'est LA commande à utiliser
.\scripts\build.ps1 -Release -Bundles   # + installateur NSIS
.\scripts\test-happy.ps1       # test de bout en bout + chaîne audio
.\scripts\test-happy.ps1 -SkipAudio
.\scripts\shortcut.ps1         # raccourci Bureau
.\scripts\shortcut.ps1 -Autostart   # démarrage avec Windows
.\scripts\with-msvc.ps1 cargo test
```

**Toujours `build.ps1`, jamais `cargo build --release`** : c'est la CLI Tauri
qui embarque les assets du frontend dans le binaire.

### Tester l'avatar sans passer par Tauri

```powershell
& 'C:\Dev\Godot\Godot_v4.5.1-stable_win64.exe\Godot_v4.5.1-stable_win64_console.exe' `
  --path .\godot -- --quality=high
```

**Utiliser la variante `_console.exe`** : c'est la seule qui affiche les
`print()` de `main.gd` — `[godot/main] profil graphique : high`,
`[godot/bubble] affichée : …`, `[godot/http] POST /state`. Sans elle, tout le
diagnostic de l'avatar est invisible. Le nom du dossier porte `.exe` mais c'est
un dossier (piège 4).

Puis, dans une autre fenêtre :

```powershell
curl.exe -s -X POST http://127.0.0.1:8787/state -H "Content-Type: application/json" -d '{"state":"thinking"}'
curl.exe -s -X POST http://127.0.0.1:8787/say   -H "Content-Type: application/json" -d '{"text":"Bonjour"}'
curl.exe -s -X POST http://127.0.0.1:8787/quality -H "Content-Type: application/json" -d '{"level":"high"}'
```

---

## Lancement depuis le bureau

`scripts/shortcut.ps1` crée `Jimmy.lnk` sur le Bureau (OneDrive Desktop inclus).
Il cible `scripts/launcher.ps1`, qui démarre `target/release/jimmy.exe` en
arrière-plan — aucune fenêtre de console.

**Ce qui a été corrigé pour que cela marche :**

- **L'avatar ne démarrait pas en release.** Il était lancé depuis la commande
  `bootstrap`, c'est-à-dire par le JavaScript de l'interface. Or la fenêtre peut
  mettre du temps à charger, ou échouer : Jimmy n'apparaissait jamais sur le
  bureau. Le démarrage a été déplacé dans `setup()`, côté Rust. C'est le seul
  élément visible de l'application, il ne doit dépendre de rien d'autre.
- **`devUrl` écrasait les assets embarqués.** En release, la fenêtre tentait de
  charger `localhost:1420` et affichait « Impossible d'accéder à cette page ».
  `frontendDist` doit être déclaré **une seule fois** dans `tauri.conf.json` ;
  un doublon fait gagner `devUrl` et casse le build release.

**Le binaire debug n'est pas un repli** : il attend le serveur Vite. Le
lanceur affiche un message clair et propose de compiler.

**La fenêtre principale est masquée au démarrage, c'est voulu**
(`visible: false` dans `tauri.conf.json`). On passe par l'avatar. Le
raccourci Bureau ne montre donc qu'un bouton, pas l'interface.

---

## Ne pas y repasser — pièges déjà payés

- Un « exit code 1 » sur un build qui a réussi → piège 14.
- Le test happy path qui échoue une fois sur deux → échec transitoire du
  provider. Il n'a pas été rendu déterministe ; relancer suffit.
- La fenêtre qui ne revient pas au premier plan → ce n'est pas un bug, c'est
  `visible: false`. Utiliser `PrintWindow` pour la capturer.