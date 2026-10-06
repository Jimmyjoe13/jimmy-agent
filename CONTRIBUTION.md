# CONTRIBUTION — règles pour tout agent qui touche à ce dépôt

Ce document s'adresse aux agents (Claude Code, OpenCode, Codex…) et à toute
personne qui modifie Jimmy. Il existe parce que des erreurs **déjà payées** se
sont répétées : une interface vide, un avatar tué par une course, un historique
qui cassait l'API, une fonction « en place » que rien n'appelait.

À lire avec `HANDOFF.md` (état technique et pièges numérotés, cités ici sous la
forme « piège N ») et `JOURNAL.md` (comment on est arrivé là).

---

## 0. Les sept règles qui évitent 90 % des régressions

1. **Lire avant d'écrire.** `HANDOFF.md` (état + pièges), puis le code concerné.
   Ne jamais se fier à ce qu'un document ou un résumé de session dit être « fait » :
   vérifier dans le code.
2. **Reproduire avant de corriger.** Un bug se reproduit d'abord par un test (ou
   une commande) qui échoue ; il est corrigé quand ce test passe. Pas de
   correction « à l'intuition ».
3. **Chercher l'appelant, pas la définition.** Une fonction qui existe n'est pas
   une fonction appelée (pièges 11 et 22 : écoute, skin, MCP, qualité des
   Paramètres, tous « écrits » et jamais branchés). Avant de dire « c'est en
   place » : `rg` de l'appelant.
4. **Vérifier avec la vraie application**, pas seulement `cargo build` : un
   build qui passe ne dit rien sur l'interface, l'audio ou l'avatar.
5. **Un changement = un besoin.** Pas de refactor, de renommage ou de « petite
   amélioration » glissés dans un correctif. Une amélioration repérée se signale
   en une ligne à la fin, elle ne se fait pas.
6. **Ne jamais déclarer « corrigé » sans l'avoir observé.** Dire ce qui a été
   vérifié, comment, et ce qui ne l'a pas été.
7. **Consigner ce qui est durable** (voir §9) : un piège non écrit sera repayé.

---

## 1. Compiler : toujours `build.ps1`, depuis PowerShell

```powershell
.\scripts\build.ps1 -Release        # LA commande pour le binaire utilisable
.\scripts\build.ps1                 # debug (attend le serveur Vite : pas un repli)
.\scripts\with-msvc.ps1 cargo test  # tests Rust
```

- **Jamais `cargo build --release` seul.** C'est la CLI Tauri (`tauri build`) qui
  embarque les assets du frontend. Sans elle, la fenêtre vise `localhost:1420` et
  affiche **« localhost a refusé de se connecter »** : interface vide, alors que
  l'avatar et la voix tournent (pièges 7 et 56). C'est exactement ce qui s'est
  produit à la fin de la session du 3 octobre.
- **Ne jamais compiler depuis Git Bash.** Le `link.exe` de Git (`/usr/bin/link`)
  passe devant celui de MSVC : « missing operand after ' ■' ». Utiliser l'outil
  PowerShell.
- `frontendDist` ne doit apparaître **qu'une fois** dans `tauri.conf.json`
  (piège 7).
- Un « exit code 1 » après un pipe vers `Select-Object -Last N` n'est pas un
  échec de build (piège 14). Rediriger vers un fichier, puis lire le fichier et
  `$LASTEXITCODE`.
- `--` passé à un `.ps1` est avalé : écrire `'--'` entre guillemets (piège 20).
- `with-msvc.ps1` : seul `$LASTEXITCODE` fait foi (piège 19).

## 2. Ne pas casser ce qui tourne

Jimmy est une application que l'utilisateur **utilise en direct**.

- **Vérifier qu'aucun Jimmy ne sert l'utilisateur** avant de compiler (l'exécutable
  est verrouillé), de lancer des tests d'écoute ou de relancer un service.
  `Get-Process jimmy,Godot*,whisper-server`.
- **Ne pas lancer les tests d'écoute pendant l'usage** : ils démarrent leurs
  propres `whisper-server` (ports 8178/8179) puis les tuent (piège 35). Un Jimmy
  relancé entre-temps les réutilise et devient muet.
- **Ne jamais tuer un processus que tu n'as pas lancé** sans l'avoir identifié
  (chemin, date de démarrage). Un Godot ou un whisper orphelin est normal
  (pièges 6, 52).
- **`kill_on_drop`** : écraser un client qui possède un processus le tue
  (piège 24). Ne jamais remplacer `app.stt` ou `app.godot` « pour être propre ».
- **Le débogage distant WebView2 ne reste jamais ouvert** (piège 26). Seul
  `test-ui.ps1` l'utilise et relance Jimmy sans à la fin.
- Après un test d'interface, remettre Jimmy dans l'état normal (sans port de
  débogage, un seul processus).

## 3. Les erreurs qui reviennent — à ne pas refaire

| Erreur déjà commise | Règle | Piège |
|---|---|---|
| Fonction jamais appelée (écoute, `set_skin`, MCP, qualité) | `rg` de l'appelant avant de conclure | 11, 22 |
| Binaire release sans interface | `build.ps1 -Release`, jamais `cargo build --release` | 7, 56 |
| Historique rechargé avec des messages `tool` → HTTP 400 | `History::conversation` ne renvoie **jamais** d'outils | 45 |
| Identifiant de modèle avec préfixe `opencode-go/` écrit en config | `id` = identifiant **court** (`space-bunny-free`) | 48 |
| Table ajoutée sans monter `SCHEMA_VERSION` | les bases existantes ne la reçoivent jamais | 30 |
| Audio entrelacé lu comme du mono | mixer à la capture, dupliquer par trame à la sortie | 23, 28 |
| Deux `start_avatar` concurrents | passer par le `Mutex` de lancement ; statut = `avatar_running()` | 50, 51 |
| Variable locale à la boucle d'écoute qui perd la session | ce qui doit survivre va sur `App` | 54 |
| Outil sans borne de temps qui gèle la conversation | chaque outil porte son budget (`search_files` 10 s) | 55 |
| `guard()` sur une action `void` | utiliser `ui::attempt` | 13 |
| `localhost` dans une URL locale (+2,3 s) | `127.0.0.1` | 47 |
| Normalisation du volume avant Whisper | dégrade la transcription, retirée | 32 |
| `to_lowercase()` global dans la recherche du vault | décale les extraits ; normaliser caractère par caractère | 53 |
| `{ok:true}` pris pour une preuve | vérifier l'état réel (fenêtre, processus, fichier) | 15 |

## 4. Règles de code

**Rust (`agent/`, `desktop/src-tauri/`)**

- `agent/` ne dépend **jamais** de Tauri : c'est ce qui le rend testable sans
  fenêtre. Tout ce qui touche à la fenêtre va dans `desktop/src-tauri/`.
- `State<'_, AppState>` explicite dans chaque commande Tauri, jamais un alias
  `State<'static, …>` (piège 3).
- Une commande Tauri ajoutée = trois endroits à garder alignés : la commande
  Rust, son enregistrement dans le `invoke_handler`, et `desktop/src/api.ts`.
  Puis **un appelant réel** dans l'interface (règle 3).
- Un événement `AgentEvent` renommé ou ajouté se met à jour **aussi** côté
  TypeScript (`chat.ts`, `api.ts`). Les champs sont en `camelCase`
  (`durationMs`) : `rename_all_fields = "camelCase"` est là pour ça.
- Les erreurs d'un **outil** se renvoient au modèle (il se corrige) ; elles ne
  font pas échouer l'agent. Les erreurs remontées à l'utilisateur tiennent en une
  phrase ; le détail va dans le journal.
- Aucun `unwrap()` sur une donnée externe (réseau, disque, modèle, audio).
- Tout nouvel outil porte son propre budget de temps et sa propre taille maximale
  de sortie (piège 55).
- Un nouveau réglage : valeur par défaut dans `config.rs`, lecture réelle
  (un réglage défini et jamais lu a déjà existé : `auto_learn_every`), champ dans
  l'interface, test.
- Commentaires fréquents, en français, qui expliquent le **pourquoi** (surtout
  pour un contournement : écrire quel piège il évite).

**TypeScript (`desktop/src/`)**

- `npm run build` (TypeScript strict) doit passer sans erreur ni avertissement.
- Un écouteur d'événement passe par `ctx.onEvent` / `ctx.onCleanup`, jamais
  `addEventListener` nu : sinon il s'empile à chaque navigation.
- Une vue **remplace** son contenu au rendu, elle ne l'ajoute pas (`content.append`
  empilait 10 vues).
- Un toast de succès ne s'affiche qu'après un succès constaté.
- Libellés d'interface en **français**.

**GDScript (`godot/`)**

- Godot ne fait que le rendu. Aucun asset ajouté : normal map, ombre et sons sont
  **générés** (décision structurante).
- Contour inverted hull interdit sur cônes et boîtes (piège 18). Avant de toucher
  à l'ombre au sol, lire le piège 17.
- Se tester avec la variante `_console.exe`, seule à afficher les `print()`
  (piège 4 : le *dossier* Godot porte le nom d'un exécutable).

**Scripts PowerShell**

- `.ps1` avec accents = **UTF-8 avec BOM** (piège 1).
- JSON avec accents envoyé à `curl` : écrire un fichier, puis `--data-binary @fichier`
  (piège 2).
- Pour patcher du français depuis un shell : écrire un script dans un fichier,
  pas de heredoc (piège 27).

## 5. Secrets

- **Aucun secret dans un prompt, un commit, un journal, une réponse.** Clés et
  cookies vivent dans `%USERPROFILE%\.secrets\<service>.json`, hors dépôt.
- Lecture ciblée d'une seule valeur ; ne jamais afficher le fichier entier.
- Un secret exposé : révoquer côté service d'abord, régénérer, nettoyer ensuite.
- `JIMMY_LLM_DUMP` enregistre les requêtes **sans** la clé : ne jamais l'élargir.
- `data/` contient la base, les journaux et les extraits audio de l'utilisateur :
  ne pas le committer, ne pas en recopier le contenu dans une réponse.

## 6. Mémoire, vault et identité

- La mémoire persistante de Jimmy est **le vault Obsidian**
  (`C:\Obsidian\Jimmy`), dossier d'écriture `0_Inbox/Jimmy` (piège 53). Synaptiq
  n'est plus branché : ne pas le réintroduire sans décision de l'utilisateur.
- Jimmy **lit** tout le vault mais n'**écrit** que dans son dossier. Ne jamais
  modifier, déplacer ou supprimer une note existante du vault.
- Ne pas confondre le second cerveau de l'agent et celui de l'utilisateur : les
  notes du vault sont rédigées pour un humain.

## 7. Vérifier : quoi lancer selon ce qu'on a touché

| Tu as touché… | Lance | Critère |
|---|---|---|
| Du Rust (logique pure) | `.\scripts\with-msvc.ps1 cargo test --workspace` | 0 échec, 0 avertissement |
| Le frontend | `npm run build` dans `desktop/` | strict, sans erreur |
| Une vue, une commande Tauri, un événement | `.\scripts\test-ui.ps1` | tous les parcours OK (26 au 6 octobre), **0 erreur JavaScript** |
| La boucle d'agent, les outils, le modèle | `.\scripts\test-happy.ps1 -SkipAudio` | réponse FR avec outils (un échec transitoire du fournisseur : relancer) |
| L'écoute, Whisper, la synthèse | tests `--ignored` de `agent/tests/audio.rs` | **Jimmy arrêté** (§2) |
| Le vault | `cargo test --test vault '--' --ignored` | notes trouvées, extraits justes |
| L'avatar | route `/snapshot`, `/state`, `_console.exe` | rendu comparé avant/après |
| Un déploiement ou une release | `build.ps1 -Release` **puis** `test-ui.ps1` | binaire lancé, interface visible |

Règles de vérification :

- **Un test qu'on écrit pour un bug doit échouer avant le correctif.** Sinon il ne
  prouve rien.
- **Vérifier la valeur réellement enregistrée**, pas seulement l'affichage (c'est
  ce qui a attrapé le bug du préfixe de modèle).
- **Tester la vraie application** : un build qui compile n'est pas une interface
  qui s'affiche.
- Une capture d'écran de l'utilisateur est une **donnée** : décrire ce qu'elle
  montre avant de conclure. « localhost a refusé de se connecter » veut dire
  « binaire mal compilé », pas « le code est faux ».
- Suite incomplète ou interrompue (mémoire saturée, timeout) : le dire, relancer
  par étapes. Ne pas conclure sur une suite à moitié exécutée.

## 8. Diagnostic : méthode

1. **Le journal d'abord** : `data/logs/jimmy.log` (niveaux, durées, outils,
   textes entendus). L'application n'a pas de console.
2. **L'état réel avant l'hypothèse** : processus vivants, port ouvert, date du
   binaire, contenu de `data/config.json`.
3. **Mesurer** avant de supposer : les « évidences » écartées par la mesure
   (threads Whisper, taille du prompt, requête couverte) ont coûté des heures.
   Ne pas refaire les mesures du `HANDOFF.md`.
4. **Du service vers le client** : API / processus, puis Rust, puis interface.
5. La latence du modèle de langage (2 à 25 s pour la même requête) **n'est pas
   un bug de Jimmy** (piège 49). Ne pas « l'optimiser » à l'aveugle.

## 9. Rendre compte et transmettre

- **Fin de run autonome (plus de ~3 appels d'outils)** : exactement trois lignes.

  ```
  Fait : <ce qui est vérifié, chemin de fichier inclus>
  Restant : <les étapes restantes, dans l'ordre>
  Bloqué : <ce qui empêche d'avancer, ou "rien">
  ```

  `Fait` ne liste que du vérifiable. Répondre à « c'est bon ? » avec ce bloc,
  sans relancer d'outils.
- **Contexte repris d'une autre session ou d'un autre agent** : distinguer
  explicitement ce qui est *repris* de ce qui a été *vérifié*. Un fait repris
  n'est pas un fait vérifié. Si l'agent précédent était un modèle plus faible,
  relire son diff avant de s'y appuyer.
- **Un piège découvert = une entrée numérotée dans `HANDOFF.md`** (symptôme,
  cause, règle), au numéro suivant. Ne pas renuméroter.
- **Décision structurante** : tableau « Décisions prises » du `HANDOFF.md`, avec
  la raison. Ne pas reproposer ce qui y est déjà tranché.
- Ne jamais écraser le `HANDOFF.md` : lire l'existant, **préserver** « Décisions
  prises » et « Pièges connus », ajouter.
- `JOURNAL.md` raconte l'histoire (chronologique) ; `HANDOFF.md` donne l'état.
  Ne pas mélanger.

## 10. Git

- **Rien n'est poussé sans demande explicite** de l'utilisateur. Aucun
  `git push`, aucun `--force`, aucun `reset --hard`.
- Un commit = un sujet, message en français, qui dit **ce qui a été corrigé et
  pourquoi**. Pas de fichier généré (`target/`, `desktop/dist/`, `data/`).
- Ne jamais committer un secret ni une capture contenant des données perso.
- Un BOM dans un message de commit vient d'un script de ré-encodage : le vérifier
  (piège 1).
- Avant de committer un grand lot en attente : relire le diff **fichier par
  fichier** et lancer la vérification du §7 correspondante.

## 11. Contrat avec l'utilisateur

- **Réponses en français, courtes, directes.** Pas de mise en forme excessive.
- **Demander avant** : suppression, changement d'architecture, push, choix
  technique important, changement de fournisseur ou de modèle par défaut.
- **Ne pas empiler les chantiers.** Si la demande en contient plusieurs, les
  nommer et les traiter un par un.
- **Une demande floue** : demander le référent (fichier, section, capture) plutôt
  que deviner. Une intention visuelle (« cohérent », « plus propre ») sans
  référent a déjà coûté six corrections d'affilée.
- **Reconnaître une erreur en une phrase** et la corriger. Pas de longue
  explication.
- Changement de configuration d'un modèle ou d'un fournisseur : vérifier le
  **format exact de l'identifiant**, puis faire un **test réel** avant de dire
  que c'est fait.

---

## Liste de contrôle avant de dire « c'est fait »

- [ ] J'ai relu `HANDOFF.md` et le code concerné, pas seulement un résumé.
- [ ] Le bug est reproduit par un test qui échouait avant mon correctif.
- [ ] Tout ce que j'ai ajouté a un **appelant réel** (`rg`).
- [ ] `cargo test --workspace` : 0 échec, 0 avertissement.
- [ ] `npm run build` : sans erreur.
- [ ] `build.ps1 -Release` (PowerShell) puis `test-ui.ps1` : 0 échec, 0 erreur JS.
- [ ] Jimmy tourne **une seule fois**, sans port de débogage, avatar visible.
- [ ] Aucun secret dans le diff, aucun fichier de `data/` ou `target/` ajouté.
- [ ] Le piège ou la décision est écrit dans `HANDOFF.md`.
- [ ] Mon compte rendu distingue le **vérifié** du **non vérifié**.
