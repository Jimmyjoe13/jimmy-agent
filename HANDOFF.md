# HANDOFF

État du prototype au **3 octobre 2026**.

---

## Ce qui est vérifié

| Domaine | Comment c'a été vérifié |
|---|---|
| Compilation complète | `scripts/build.ps1` — 0 erreur, 0 avertissement |
| Tests unitaires | 35 tests, tous verts |
| Agent de bout en bout | `scripts/test-happy.ps1` — vrai modèle, 4 outils, réponse en français |
| Avatar 3D | captures d'écran : renard rendu, fenêtre transparente sur le bureau |
| États de l'avatar | `POST /state` pour les 8 états, rendu vérifié |
| Bulle de dialogue | `POST /say` avec accent → texte affiché correctement |
| Clic sur l'avatar | `POST /ui/open` → fenêtre ouverte (capture) |
| Application Tauri | capture d'écran : onboarding affiché au premier lancement |
| Installateur | `scripts/install.ps1 -NoBuild` → 8 vérifications, sortie 0 |
| Frontend | `npm run build` → TypeScript strict sans erreur |
| Godot | `--headless` → aucun script en erreur |
| **Synthèse vocale** | `cargo test --test audio tts` → 225 280 octets PCM 44 100 Hz, **lu sans erreur** |
| **Écoute permanente** | `cargo test --test audio ecoute` → micro ouvert, `whisper-server` démarré |

---

## Pièges connus

**1. PowerShell 5.1 lit les `.ps1` en ANSI sans BOM.**
Tout fichier `.ps1` contenant des accents doit être écrit en **UTF-8 avec
BOM**. Sans BOM, un tiret cadratin (0xE2 0x80 0x94) est relu comme `"` (0x94 en
CP1252) et casse la chaîne. Symptôme : le script s'arrête silencieusement après
la première ligne contenant un tiret. *Les six scripts de `scripts/` ont le BOM.
Conserver cette propriété à chaque édition.*

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
consomme, donc aucun décodeur MP3 à embarquer (ni dépendance, ni licence).
`providers::tts::parse_rate` lit la fréquence dans l'en-tête
`audio/pcm;rate=44100;channels=1` ; si elle est absente, elle vaut 0 et le lecteur
ne rééchantillonne pas. Le MP3 n'est plus supporté — `decode_audio` le refuse
explicitement plutôt que de produire du bruit.

**10. `build_output_stream` ne joue rien tant que `.play()` n'est pas appelé.**
C'est le piège le plus coûteux de cette session : le code construisait le flux,
puis attendait `done` dans une boucle. Sans `.play()`, la callback n'est jamais
invoquée, `done` ne passe jamais à `true`, et **la lecture bloque
indéfiniment** — sans message d'erreur. Le test TTS reste bloqué plus de 15
minutes avant correction. Symptôme reconnaissable : le processus consomme un cœur
et rien ne se passe. Il y a maintenant un filet de sécurité : durée réelle
majorée d'une seconde, puis abandon avec `log::warn`.

**11. Une commande Tauri qui existe n'est pas une commande appelée.**
`voice_start` / `voice_stop` étaient implémentées côté Rust, exposées dans
`api.ts`… et jamais invoquées. Résultat : le micro n'était jamais ouvert,
`whisper-server` jamais lancé, et le wake word muet — sans la moindre erreur
visible. Une commande Rust sans appelant ne se voit pas. Vérifier avec
`rg 'api\.voiceStart' desktop/src` : un résultat = un bug.

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
micro s'ouvrait, le texte ne s'affichait pas. Même famille que le piège 11, mais
invisible et plus sournois : TypeScript n'y voit rien.

`ui::attempt` existe pour ça : il renvoie un booléen. **Règle : dès que la
commande Tauri ne retourne rien, utiliser `attempt`, jamais `guard`.**
`tts_preview`, `voice_start`, `avatar_start`, `avatar_quality`,
`avatar_stop`, `set_startup`, `save_*` sont tous concernés.

**14. Un « exit code 1 » sans erreur n'est pas forcément une erreur.**
Les commandes de ce projet sont souvent pipées vers `Select-Object -Last N` ou
`-First N` : PowerShell ferme alors le pipeline en amont, la commande native est
tuée, et le code de sortie vaut 1 alors que la compilation a réussi. Redirecter
vers un fichier (`> log 2>&1`) puis lire le log, pour un verdict fiable.

---

## Prochaines étapes, par valeur

### Le plus utile maintenant

1. **Utiliser Jimmy au quotidien une semaine.** C'est le seul test qui compte
   pour la question du PLAN : est-ce que la voix apporte quelque chose ?
   Noter les moments où l'on écrit au lieu de parler.

2. **Brancher le modèle `small-q5` pour la commande.** Le wake word reste sur
   `base` (latence), mais la transcription de la phrase gagnerait ~3 s de
   qualité sur le français. Il faut deux serveurs, ou un rechargement de modèle.

3. **Icônes et sons d'état.** Trois petits fichiers audio
   (`assets/sounds/`) : un pour l'écoute, un pour la réponse, un pour l'erreur.

### Structurant ensuite

4. **Vrai modèle d'embeddings** dans `memory/embed.rs`. Le hachage fonctionne
   mais ne rapproche pas les synonymes. Le point d'entrée est `MemoryStore`.

5. **Export Godot** (~1 Go de gabarits) pour que l'installateur n'installe pas
   le moteur complet.

6. **MCP : transport HTTP** et commande `mcp_add_server` pour que Jimmy
   installe lui-même un serveur pertinent.

### Différé

7. Peaux supplémentaires ; le chargeur est prêt (`--skin=`).
8. Mise à jour automatique, quand il existera une distribution.
9. Permissions par outil/appareil.

---

## Commandes

```powershell
.\scripts\install.ps1          # vérifications
.\scripts\dev.ps1              # lancer Jimmy (développement)
.\scripts\build.ps1            # compiler
.\scripts\build.ps1 -Release   # release + installateur NSIS
.\scripts\test-happy.ps1       # test de bout en bout
.\scripts\shortcut.ps1         # raccourci Bureau
.\scripts\with-msvc.ps1 cargo test
```

---

## Lancement depuis le bureau

`scripts/shortcut.ps1` crée `Jimmy.lnk` sur le Bureau (OneDriveDesktop inclus).
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