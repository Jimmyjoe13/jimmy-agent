# HANDOFF

État du prototype au **3 octobre 2026**, fin de la session « avatar +
fonctionnalités ».

**Objectif de la prochaine session :** **utiliser Jimmy au quotidien** à la
voix, et vérifier en conditions réelles ce que les tests ne voient pas
(serveur MCP du catalogue, voix humaine à distance du micro intégré).
Tout ce qui suit est lu dans le code et vérifié, pas une liste d'idées.

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
| Rendu avatar (lot 1-2) | `/snapshot` sur les 3 profils, planches avant/après comparées |
| Skins | `/skin` renard → arctique → fennec → « licorne » refusé, rendu vérifié |
| Sons d'état | `cargo test --test audio sons_d_etat -- --ignored` : 3 sons en cache, « Oui ? » relu en 267 ms |
| MCP | `cargo test --test mcp` : serveur Node réel via `cmd /C`, noms normalisés, `isError` remonté |
| Tests | 37 unitaires + 3 MCP + 1 mémoire, tous verts (`cargo test --workspace`) |
| Deux whisper-server | `--test audio commande_transcrite -- --ignored` : phrase Fish Audio transcrite par `small` en 3 s |
| Mémoire sémantique | `--test memory_semantic -- --ignored` : « véhicule » retrouve « voiture » (0,50), le hachage ne trouve rien |
| **Écoute de bout en bout** | `--test audio ecoute_reconnait -- --ignored` : phrase injectée à RMS 0,008 → « Jimmy? » reconnu → « quelle heure est-il ? » → réponse de l'agent, 24 s |
| **Interface** | `scripts\test-ui.ps1` : 12 parcours sur la vraie application (CDP), 0 erreur JS |
| Tests | 43 unitaires (dont mixage micro et mot d'éveil phonétique) |

---

# Fait pendant cette session (commits 4ebac03 → 368dbfa)

## Avatar

- **Cadrage** : caméra à `(0, 1.92, 4.69)`, plongée 8°. Le renard occupait
  ~35 % de la hauteur, il est ~1,6× plus grand ; la bulle garde le haut.
- **Rendu** (`main.gd`) : tonemapper **AgX** + saturation 1,18 / contraste
  1,06 ; **SSAO** réglée à l'échelle du personnage (rayon 0,22), coupée en
  `low` ; `medium` rendu à pleine résolution (0,85 rendait flou).
- **Ancrage au sol** : tache de contact (tous profils) + **capteur d'ombre**
  `shadow_to_opacity` (medium/high). Voir piège 17 avant d'y toucher.
- **Proportions** (`jimmy.gd`) : `HEAD_Y = 0.30` (cou de girafe à 0,42),
  bassin, épaules, bras rapprochés (0,135), ventre enfoncé.
- **Contour** inverted hull (`next_pass`), sauf yeux et cônes (piège 18).
- **Tessellation** ×1 / ×1,5 / ×2 selon le profil : `set_detail` reconstruit
  le personnage à chaud. Les trois profils diffèrent enfin par la géométrie.
- **Fourrure** : `rim` + normal map générée par `NoiseTexture2D` — toujours
  **zéro asset** dans le dépôt.
- **Route `/snapshot`** `{"path": "..."}` : PNG du rendu, alpha compris. C'est
  l'outil de comparaison avant/après (pas besoin du premier plan, piège 16).

Leviers restants, par gain : visage plus expressif (sourcils, reflets dans
les yeux), mains et pieds moins « billes », bascule des ombres en `high`
vers des ombres plus douces (PCSS). TAA écarté : Jimmy bouge sans arrêt, il
traînerait ; le MSAA 4× suffit en `high`.

## Fonctions

| Sujet | État | Où |
|---|---|---|
| **Skins** | **Réel.** renard, arctique, fennec (`ear_scale`), appliqués à chaud. Avant : `set_skin` Rust jamais appelé, `/skin` faisait un `print`. | `jimmy.gd` `SKINS` + `providers::avatar::SKINS` (garder alignés) |
| **Sons d'état** | **Réel.** « Oui ? » / « C'est prêt. » / « Oups… » synthétisés avec la voix TTS courante, cache `data/audio/cues/`. Réglage `tts.cues`. | `agent/src/voice/cues.rs` |
| **MCP** | **Réel en stdio.** Outils exposés au modèle, `mcp_add_server`, connexion au démarrage. Le client existait mais n'était branché nulle part. | `agent/src/tools/mcp.rs`, `agent/src/mcp/mod.rs` |
| MCP HTTP | Non fait. Le stdio couvre presque tout le catalogue. | `McpRegistry::ensure` |
| **Mémoire sémantique** | **Réel.** Embeddings LM Studio (`localhost:1234`, modèle de SynaptiQ, 384 dim), table `memory_semantic`, réindexation au démarrage, seuil 0,35. LM Studio éteint → hachage, pause de 60 s avant de retenter. | `memory/semantic.rs`, `MemoryStore::recall_semantic` |
| **Deux whisper-server** | **Réel.** `base` (8178) pour le wake word, `small` (8179) pour la commande ; repli sur `base` si le second ne démarre pas. Réglable dans Paramètres. | `App::start_voice`, `App::transcribe_command` |
| Permissions | Globales. MCP passe par EXÉCUTION, cible `mcp:<serveur>` : un serveur précis peut être interdit par `deny_commands`. | `permissions.rs` |

**Comportement des sons, choisi exprès :** « Oui ? » ne joue que si
« Jimmy » est dit **seul** puis une pause. Le micro est alors purgé (sinon le
haut-parleur repasse dans la commande) et l'écoute accorde 3,5 s de grâce. Si
la commande suit directement le wake word, aucun son : il couperait la parole.
En vocal, réponse et erreur sont déjà dites à voix haute ; « C'est prêt. » et
« Oups » ne servent qu'au chat texte.

---

# Audit écoute + interface (3 octobre 2026, fin de journée)

## Pourquoi l'écoute ne marchait pas — quatre causes cumulées

1. **Capture micro fausse** (`voice/mod.rs`). Le flux cpal est entrelacé et à
   la fréquence du périphérique (ici 48 kHz **stéréo**). Il était stocké tel
   quel puis lu comme du mono 16 kHz : Whisper recevait un son ralenti, et la
   fenêtre de 2,4 s du mot d'éveil n'en contenait que 0,4 s. Désormais
   `Downmixer` convertit en mono 16 kHz **dans la callback** ; le tampon est
   toujours au format d'analyse.
2. **Seuil de VAD fixe trop haut** (0,012). Micro intégré mesuré : 0,002 au
   repos. VAD adaptatif : 3 × bruit de fond, entre 0,0035 et le réglage.
3. **« Jimmy » mal orthographié par Whisper** (« Guimmi »). Prompt Whisper
   « Jimmy, » sur chaque inférence + comparaison phonétique (`phonetic`) +
   interjections acceptées (« hé Jimmy »).
4. **Détection en cours de phrase** : la fenêtre ne contenait que « Jimmy »,
   le code concluait « Jimmy seul », jouait « Oui ? » et purgeait le micro…
   pendant que la commande était dite. On écoute maintenant la phrase jusqu'au
   silence avant de décider.

Plus : relancer l'écoute **tuait les serveurs whisper** (piège 24), plusieurs
boucles d'écoute pouvaient tourner sur le même micro (compteur de
génération), et l'écoute ne reprenait jamais au lancement
(`voice.listen_on_start`, mis à jour par les boutons).

## Interface — corrigé

- **Vues empilées** : `render()` faisait `content.append` → chaque navigation
  ajoutait une vue sous les précédentes (10 vues montées après un tour).
- Écouteurs d'événements jamais retirés (un de plus à chaque passage par le
  chat) ; remplacés par `ctx.onEvent` / `ctx.onCleanup`, libérés à la sortie.
- Journal d'activité du chat jamais inséré dans la page.
- Réponse : bulle « Jimmy réfléchit » + bouton occupé jusqu'à la réponse
  (avant : libéré au bout de 400 ms). Pastille d'état globale, retour à « prêt ».
- Libellés « Jimmy/Vous » en double dans les bulles ; titres en double.
- État de l'écoute deviné par la vue (retombait à « arrêtée ») → `voice_status`.
- Vue Voix : fil « Ce que Jimmy entend » (événement `heard`), vumètre en
  direct, état des deux serveurs ; « Enregistrer » marche sans écoute active.
- Toasts de succès affichés même en cas d'échec (`guard` sur des `void`).
- Onboarding : prénom pré-rempli « Jimmy », fermeture même si l'enregistrement
  échouait. Libellés anglais traduits (qualité, états, types de souvenirs).
- Logs de la release perdus (application sans console) → `data/logs/jimmy.log`.

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

**6. Un seul `whisper-server` par port.**
Deux serveurs tournent désormais : 8178 (wake word) et 8179 (commande). Une instance de Jimmy lancée en arrière-plan empêche le
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

**17. `shadow_to_opacity` : albedo noir = plan invisible.**
Lu dans `scene_forward_clustered.glsl` (Godot 4.5.1, l. 2054 et 2634) :
l'alpha est plafonné par `length(ambient_light * albedo)`. Albedo noir → alpha
0 partout. Et chaque lumière **sans ombre** qui touche le plan l'efface
(`alpha = min(alpha, 1 - attenuation)`). Recette qui marche : albedo blanc,
`metallic = 1` (annule l'ambiante dans la couleur, appliqué *après* l'alpha),
plan sur le calque 2, omni avec `light_cull_mask = 1`.

**18. Contour inverted hull sur un cône = éclats.**
`CylinderMesh` a des normales non lissées entre flanc et base : la coque
gonflée se fend en éclats visibles (oreilles). `_cone()` utilise donc
`_without_outline()`. Même règle pour tout futur `BoxMesh`.

**19. `with-msvc.ps1` et la redirection de stderr.**
Avec `$ErrorActionPreference = 'Stop'`, toute ligne écrite sur stderr par une
commande native (le « Compiling » de cargo, le message `vswhere` de vcvars)
devient une erreur bloquante **dès que l'appelant redirige** (`> log 2>&1`).
Corrigé : `'Continue'` juste avant l'appel natif. Seul `$LASTEXITCODE` fait foi.

**20. PowerShell avale le `--` de `cargo test -- --ignored`.**
Passé à un script `.ps1`, `--` est consommé comme fin de paramètres. Écrire
`'--'` entre guillemets : `.\scripts\with-msvc.ps1 cargo test --test audio x '--' --ignored`.

**21. Sous Windows, `npx`/`uvx` ne se lancent pas avec `Command::new`.**
Ce sont des `.cmd`. Le lanceur MCP passe par `cmd /D /C` sauf pour un `.exe`
explicite. Le test `--test mcp` lance `node` sans extension pour couvrir ce
chemin.

**22. Le piège 11 s'est répété trois fois.**
`set_skin` (Rust) jamais appelé, `McpRegistry` jamais instancié, qualité et
skin des Paramètres jamais poussés à Godot. Avant de dire « c'est en place »,
chercher **l'appelant**, pas seulement la définition.

**23. Un flux micro cpal est entrelacé, à la fréquence du périphérique.**
Ne jamais stocker `data` tel quel : mixer les canaux et rééchantillonner à la
capture (`Downmixer`). Le casque Jabra sort en 16 kHz mono, le micro intégré
en 48 kHz stéréo : les deux doivent marcher.

**24. Remplacer un client `Stt` tue son serveur.**
`kill_on_drop` : écraser `app.stt` détruit l'ancien client et son processus,
même si le nouveau venait de le juger « déjà actif ». `ensure_stt` garde le
client tant qu'il répond.

**25. Le micro intégré n'entend pas les haut-parleurs (annulation d'écho).**
Mesuré : 0,004 au maximum pendant la synthèse. Impossible de tester l'écoute
en faisant parler Jimmy. Tester par injection : `start_without_device` +
`inject` (test `ecoute_reconnait_jimmy_et_repond`).

**26. Le débogage distant de WebView2 ne doit jamais rester ouvert.**
`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222` donne le
contrôle total de l'interface à tout processus local. `test-ui.ps1` relance
toujours Jimmy sans ce port à la fin.

**27. Un `heredoc` bash avec une apostrophe dans un script Python casse.**
Pour patcher des fichiers contenant du français, écrire le script dans un
fichier (scratchpad) plutôt que `python3 - <<'EOF'`.

---

## Prochaines étapes

### Décisions prises (lot 6, 3 octobre 2026)

- **STT : deux serveurs** plutôt que `small` partout (+3 s sur chaque
  fenêtre de wake word) ou `base` partout (commandes moins bien transcrites).
  Coût : ~180 Mo de RAM.
- **Embeddings : LM Studio**, même modèle que SynaptiQ, avec repli sur le
  hachage. Ne pas retirer le hachage : c'est lui qui garde la mémoire
  utilisable quand LM Studio est éteint.

### Ensuite

3. **Utiliser Jimmy au quotidien une semaine** — le seul test qui compte.
4. Vérifier en conditions réelles un serveur MCP du catalogue (`npx -y
   @modelcontextprotocol/server-filesystem <dossier>`) et l'écoute à voix
   humaine, à distance du micro intégré (regarder le vumètre de la vue Voix).
5. Vue MCP dans l'interface (le statut expose déjà `mcp.servers` et
   `mcp.tools`).
6. Serveurs whisper orphelins : si Jimmy est tué brutalement, ses
   `whisper-server` survivent et sont réutilisés au lancement suivant (le
   contrôle de santé les trouve). Sans gravité, mais un orphelin lancé avec un
   autre modèle serait réutilisé tel quel.

### Différé

7. Export Godot (~1 Go de gabarits) pour que l'installateur n'installe pas le
   moteur complet.
8. Mise à jour automatique, quand il existera une distribution.

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
.\scripts\test-ui.ps1        # 12 parcours UI sur la vraie application (CDP)
.\scripts\with-msvc.ps1 cargo test -p jimmy-agent --test audio ecoute_reconnait '--' --ignored --nocapture
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