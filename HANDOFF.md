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