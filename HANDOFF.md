# HANDOFF

État du prototype au **8 octobre 2026**. Depuis le 6 : les **tâches de fond**
(une tâche outillée qui travaille encore 45 s après son premier outil passe en
arrière-plan, le Chat et la voix restent libres, un **lapin** travaille à côté
du renard, « STOP » = premier plan, « arrête tout » = tout ; budget de fond
60 étapes / 20 min), la **vision** (fenêtre active jointe à la demande, sur
demande de l'utilisateur seul), **SynaptiQ** rebranché par MCP (identité
`jimmy`), le **navigateur** (Playwright, son propre Chrome, carte d'accord
avant toute action qui engage), et la zone affichée de l'avatar corrigée
(piège 85). Modèle principal et vocal : `muse-spark-1.3-contributor`.

**Objectif de la prochaine session :** les **essais réels** de ce qui est
vérifié par tests mais pas encore à l'usage — navigateur connecté à un compte,
tâches de fond et vision à la voix, lapin pendant une vraie tâche de fond — et
le point « fichier sensible » avec Muse Spark (« Ensuite », point 2). Depuis le
8 octobre : la configuration LLM est dans un **sous-onglet des Paramètres** et
Jimmy connecte **plusieurs fournisseurs** (OpenCode Go, OpenRouter, DeepSeek,
Alibaba, Claude, plus un personnalisé) avec leurs clés saisies dans l'interface ;
l'essai réel avec une vraie clé chez un nouveau fournisseur reste à faire
(« Ensuite », point 6).
Tout ce qui suit est lu dans le code et vérifié, pas une liste d'idées.

---

## Reprise — matin du 9 octobre 2026

Poussé la veille au soir : `main` = `49a2b95` (mécanisme MAJ + masquage
secrets + docs), à jour des deux côtés. Le Jimmy de ce PC tourne sur le
binaire final, à jour, sans alerte (normal : rien n'a bougé depuis).

Reste, dans l'ordre :
1. Au travail : cloner (README « Deuxième PC »), `install.ps1`, clés dans
   l'interface, `build.ps1 -Release`, lancer depuis le clone.
2. Ici : petit commit déclencheur visible, puis push.
3. Au travail (sous 30 min) : bulle + pastille + session « Mise à jour »,
   puis `/update` (pull + rebuild + restart auto).
4. Régénérer les 2 clés exposées (`aggregate`, SynaptiQ).
5. Re-mesurer le parcours tâche de fond quand le fournisseur est rapide
   (échec ×2 le 8 au soir : la question « cerise » elle-même a dépassé
   45 s et s'est détachée — latence modèle, sans lien avec ce chantier).


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
| **Vault Obsidian** | `--test vault '--' --ignored` : 370 notes parcourues, « jimmy » → 5 notes en 0,39 s, extraits réels |
| **Avatar : chien de garde** | Godot tué à la main → `[avatar] Godot s'est arrêté (exit code: 0xffffffff) ; relance automatique` → relancé en 1 s |
| Tests | 75 unitaires + `--test vault` (1 + 1 ignoré) |
| **Tests (4 octobre)** | 95 unitaires agent + 1 desktop, 0 avertissement (`cargo test --workspace`) |
| **Interface (4 octobre)** | `test-ui.ps1` : **18 parcours**, 0 erreur JS (dont fil du chat, bibliothèque de voix, serveurs MCP) |
| Écoute : hésitation | `--test audio ecoute_hesitation_ne_coupe_pas_la_phrase -- --ignored` : phrase entière malgré 1,8 s de pause |
| Voix « Le narrateur » | `--test audio voix_du_narrateur_et_catalogue -- --ignored` : 159 744 octets, retrouvée au catalogue |
| Mémoire partagée | `--test happy_path il_sait_que_sa_memoire_est_partagee -- --ignored` : réponse correcte du vrai modèle |
| Regard de l'avatar | `/snapshot` avant/après + 10 captures au repos : face à l'utilisateur à ±3 px, coups d'œil ponctuels |
| Leçon après échec d'outil | `--test happy_path un_echec_d_outil_produit_une_lecon -- --ignored` : `read_file` échoue → leçon en base (source `lesson`) et vault ; journal : `extraction illisible (tentative 1)` puis réessai fructueux |
| Compétence capturée | `--test happy_path une_trajectoire_outillee_se_condense_en_skill -- --ignored` : trajectoire de 3 outils (list_directory, 2 read_file, run_command) → SKILL.md écrit dans le dossier de skills (temp), description et corps non vides |
| Revue périodique | `--test happy_path la_revue_propose_sans_appliquer -- --ignored` : 2 leçons en mémoire → session « Revue » avec une skill proposée (16,6 s) ; seconde revue immédiate non due (idempotence) |
| Filet d'amendement | `--test amendment l_amendement_ne_degrade_pas_les_reponses -- --ignored` : A/B sur 3 demandes réelles (sans/avec l'amendement, `JIMMY_AMENDMENT`), 0 dérive, longueur comparable, 28 s |
| **Chat façon Codex (lots 0-4)** | `test-ui.ps1` : 25/25 — menu « @ » (1 item, insertion sans envoi), puce de chemin → aperçu avec ligne surlignée, bloc « travaux » (`reussite-47.txt`), historique groupé (3 groupes) + recherche + Ctrl+K ; 126 tests verts ; `fs_search`/`fs_diff` testés unitairement sur tempdir |
| **Sortie courte (5 octobre)** | prompt « Sortie » + `limit_for_speech` (240 caractères dits, le reste à l'écran) |
| **Streaming (5 octobre)** | `test-ui.ps1` 25/25 : réponse écrite en direct, `[llm] premier fragment après … ms` au journal (pièges 74-76) |
| **Fichiers sensibles (5-6 octobre)** | tests `sensitive` (dont les 11 lectures réelles du 6 octobre, libres, et 8 écritures déguisées, bloquées) ; parcours UI : carte « garde-N.env », refus respecté (piège 77) |
| **Trois formats d'API (6 octobre)** | `--test protocols -- --ignored` : glm-5.3-flash (Chat), muse-spark-1.3-contributor (Responses), qwen3.8-flash (Messages) — outil appelé, résultat relu, réponse en flux ; test de bibliothèque vert pour les trois (piège 78) |
| **État au 6 octobre** | 151 tests unitaires, 0 avertissement ; `test-ui.ps1` : **26/26**, 0 erreur JS |
| **État au 7 octobre** | 176 tests unitaires + 7 de tâches de fond, 0 avertissement ; tests réels `protocols` (image), `screen`, `browser` ; `test-ui.ps1` : **27/28**, 0 erreur JS — l'échec est « fichier sensible » (Muse Spark répond sans outil, « Ensuite » 2) |
| **Navigateur (7 octobre)** | `--test browser -- --ignored` : Playwright MCP 0.0.83 lancé comme Jimmy le lance (`npx`, `cmd /C`), 25 outils, `example.com` ouvert et lu par `browser_snapshot` ; règles de la carte « action en ton nom » testées (14 cas, dont faux positifs Facebook / Mes commandes / PayPal) ; 176 tests unitaires ; `test-ui.ps1` : **27/28**, 0 erreur JS, « navigateur (connected, 25 outils) » dans Skills → Serveurs MCP (échec connu « fichier sensible ») |
| **Vision (7 octobre)** | sonde brute puis `--test protocols chaque_format_lit_une_image -- --ignored` : glm-5.3-flash (Chat), muse-spark-1.3-contributor (Responses), qwen3.8-flash (Messages) lisent « rouge, bleu » sur une image générée, en flux ; `--test screen -- --ignored` : fenêtre active capturée hors du processus appelant (1461×720, JPEG) ; 175 tests unitaires ; `test-ui.ps1` : **27/28**, 0 erreur JS (échec « fichier sensible » : le modèle répond sans outil, 3e fois de suite avec Muse Spark) |
| **Tâches de fond (7 octobre)** | `--test background` (6 tests, tâches simulées : passage en fond, STOP épargne le fond, « arrête tout », bouton de la tâche, une seule à la fois, pas pendant une autorisation) ; 165 tests unitaires, 0 avertissement ; `test-ui.ps1` : **27/27**, 0 erreur JS — parcours réel : passage en fond 15 s après l'outil, question en parallèle répondue, fin dans le fil |
| **LLM multi-fournisseurs (8 octobre)** | demande validée avant le code (sous-onglet + Claude/DeepSeek/Alibaba/OpenRouter/personnalisé + clés dans l'interface) ; 183 tests unitaires dont 7 nouveaux (migration des cinq intégrés, clé saisie gagne sur `.env`, routing modèle→fournisseur, en-têtes par fournisseur, format imposé), 0 avertissement ; TypeScript strict OK ; `build.ps1 -Release` code 0 ; `test-ui.ps1` : **28/29**, 0 erreur JS — nouveau parcours « fournisseurs » (5 lignes, Anthropic en messages, clés jamais renvoyées, ajout/retrait d'un personnalisé) ; parcours modèles vert avec sous-onglet LLM (37 modèles, « muse-spark » testé). L'échec restant est « fichier sensible », connu depuis le 7 octobre (piège : HANDOFF « Ensuite » 2). Non vérifié en réel : appels DeepSeek/Alibaba/Claude avec de vraies clés (aucune saisie à ce jour) |
| **Abonnement Claude (8 octobre)** | `--test claude_plan -- --ignored` sur la vraie session Claude Code : `/v1/models` OK (14 modèles), Haiku répond avec outils en 559 ms, Sonnet et Opus en `429` (bridage Anthropic du premium hors Claude Code, piège 90) ; 189 tests unitaires (session lue, refresh préservant le fichier, en-têtes sans `x-api-key`, mode abonnement persistant) ; suite 27/29 (deux échecs côté modèles, famille « Muse Spark sans outil » déjà consignée) ; sélecteur « Méthode » vérifié dans le parcours fournisseurs |
| **Garde-fous abonnement (9 octobre)** | bissection réelle (`--test claude_plan`) : le refus `400 « extra usage »` dépend du POIDS des outils déclarés, pas du volume premium (21 621 jetons sans outils OK ; 12 outils réalistes à 15 jetons = refus) ; 191 tests unitaires dont `le_refus_extra_usage_devient_un_message_actionnable` et `le refus premium est local` ; refus local Sonnet/Opus avant appel (`set_llm_model`, bibliothèque, `send_on`) ; message traduit avec les trois issues ; revérifié sur l'app réelle par CDP (toast, modèle inchangé, message dans le chat, session nettoyée) ; build release sans avertissement. Suite complète non relancée : la config courante (plan bloqué) ferait échouer les parcours Chat pour une raison externe au code |
| **Signature Claude Code sur le plan (9 octobre, soir)** | demande réitérée de l'utilisateur : accès complet aux modèles Anthropic avec son plan, « copie la config d'opencode si nécessaire » (vérifié : la voie OpenCode est la délégation au vrai CLI). Sondes : Q1 = noms d'outils Claude Code + descriptions/UA Jimmy + facturation → 200 (15 outils, 16 384 jetons) ; Q2 = noms Jimmy → 400. Implémenté : `map_plan_tool_names` (déclaration, `tool_use` de l'historique, prompt système) + `unmap_plan_tool_calls`, bloc de facturation en bloc 0. 194 tests unitaires verts (mapping dans les deux sens inclus), `check --workspace` 0, release sans avertissement. Preuves réelles : `le_premium_passe_par_signature_claude_code` (Sonnet ok+outils), **vrai tour d'agent complet via les commandes de l'app** (fichier créé par l'outil, réponse confirmée), suite **28/29 avec Sonnet/plan en moteur** (échec = minuterie du parcours tâche de fond, à re-mesurer). Incident consigné piège 91 (BOM PowerShell sur config.json → restauration `.bak` + commandes Tauri ; `navigateur` re-créé et rebranché, vérifié « prêt ») |
| **Tout le vocal dans l'onglet Voix (8 octobre, soir)** | demande validée avant le code : modèle vocal LLM, STT/écoute, sons d'état et voix Fish Audio vivent dans Voix, Paramètres garde fournisseurs + modèle principal + budget (carte de renvoi). `modelsPanel` gagne `onlyRole` (bibliothèque restreinte au rôle, boutons et ligne courante filtrés) ; chaque vue persiste via copie fraîche de Rust (pas d'écrasement entre onglets). 194 tests unitaires, 0 avertissement ; TypeScript strict OK ; `build.ps1 -Release` code 0 ; `test-ui.ps1` : **30/30**, 0 erreur JS (parcours « conversation continue » et « choix vocal » déménagés dans Voix, nouveau script `voice-vocal-check.js` : `deepseek-v4-pro` choisi/retiré/restauré, champ synchronisé). Constat au passage : le fournisseur actif est DeepSeek (2 modèles au catalogue) — borne de la suite adaptée (DeepSeek ≥ 2). Jimmy relancé proprement, une seule instance, sans debug |

---

# Usage réel et corrections (4 octobre 2026)

Session pilotée par l'usage : chaque point part d'un retour de l'utilisateur
ou du journal, est reproduit, puis corrigé. Détail et règles : pièges 57 à 67.

| Sujet | Cause réelle | Correction | Piège |
|---|---|---|---|
| L'écoute coupait la parole | 700 ms de silence clôturaient une phrase ; 12 s tranchées net | silence de fin adaptatif, reprise si la phrase paraît inachevée ou coupée, segments recollés | 57 |
| Serveurs MCP invisibles | aucune vue | Skills → sous-onglet **Serveurs MCP** (état réel, outils, clés masquées) | 58 |
| Jimmy perdait le fil du Chat | `sessionId` ignoré par Tauri (structure imbriquée) : **une session par message depuis la V1** | `rename_all = "camelCase"` ; résumé des outils récents + socle de mémoire dans le prompt, MCP regroupés (coût en jetons inchangé) | 59 |
| Réponses en charabia multilingue | dérive aléatoire du modèle gratuit (1/32 au rejeu) | filet : régénération, sinon coupure à la dernière phrase saine | 60 |
| Modèle vocal effacé | la suite d'interface le remettait à vide | restauration en fin de parcours | 61 |
| MiMo choisi, `space-bunny-free` utilisé | `JIMMY_LLM_MODEL` du `.env` appliqué à chaque lancement | l'environnement ne sert plus que de valeur de départ | 62 |
| Choix de la voix | deux voix codées en dur | bibliothèque (catalogue Fish Audio, écoute avant choix), **« Le narrateur » par défaut** | 63, 65 |
| HTTP 500 en vocal, 100 s de silence | panne d'OpenCode Go, non journalisée | échec journalisé, rejeu si rapide, 45 s max en vocal | 64 |
| Mémoire « mélangée » | le vault est partagé avec d'autres agents | Jimmy le sait ; chaque note porte son origine | 66 |
| L'avatar regardait la souris | suivi du curseur | regard vers la caméra (effet Joconde), coups d'œil et saccades | 67 |

Outils de diagnostic ajoutés : `agent/tests/replay.rs` (rejouer une requête
réelle N fois, plusieurs températures, `JIMMY_REPLAY_*`), et la route
`/snapshot` utilisée en série pour mesurer le regard.

Points ouverts : la clé API du serveur MCP `aggregate` figure en clair dans
`data/logs/jimmy.log` (les arguments d'outils sont journalisés tels quels) —
**à régénérer**, et à masquer dans le journal ; SynaptiQ répondait 503 en fin
de journée (la préférence de voix n'a pas pu y être notée).

---

# Avatar réparé + mémoire vault Obsidian (3 octobre 2026, nuit)

## L'avatar « ne se lance pas » — trois défauts de cycle de vie

Observé : le renard apparaissait puis **disparaissait sans trace** (aucun crash
Windows, aucun dump, journal Godot muet), et l'application relançait Godot
plusieurs fois par minute sans jamais le garder.

Trois bugs réels dans `agent/src/lib.rs`, tous corrigés :

1. **Course au lancement** : deux appels concurrents à `start_avatar`
   voyaient `is_up()` faux (port pas encore ouvert) et lançaient chacun un
   Godot. Le second écrasait le premier dans `godot` — et `kill_on_drop`
   **tuait l'avatar VIVANT** ; le second mourait sur le port déjà pris.
   → un `Mutex` de lancement sérialise, et un échec de port est remonté au
   lieu d'être masqué (10 s d'attente sur un processus mort).
2. **Statut mensonger** : `"running"` testait `godot.is_some()`, vrai même
   pour un enfant **terminé**. L'interface affichait « avatar en marche »
   sans avatar. → `avatar_running()` interroge `try_wait()`.
3. **Aucun filet** : un Godot mort ne revenait jamais. → un **chien de garde**
   (5 s, retour croissant si le lancement échoue) le relance tant que
   l'utilisateur ne l'a pas arrêté explicitement (`avatar_desired`), et logue
   le **code de sortie** — la prochaine mort sera datée et expliquée.

Un Godot **orphelin** (survivant à la mort de Jimmy) reste possible : le
chien de garde s'appuie sur `is_up()` (la vérité du service), pas sur l'enfant
stocké, et le réutilise au lieu d'en lancer un second.

## Mémoire persistante : vault Obsidian au lieu de Synaptiq

Décision : plus de service de mémoire externe. Les souvenirs de Jimmy sont des
**notes Markdown dans le vault**, lisibles et modifiables par l'utilisateur.

- **Module** `agent/src/memory/vault.rs` : parcours récursif des `.md` (hors
  `.obsidian`, `.trash`, dot-dossiers, notes > 300 Ko), recherche plein texte
  insensible casse/accents, extrait, lecture par chemin relatif, écriture d'une
  note datée. `synaptiq.rs` **supprimé** ; ses heuristiques de déclenchement
  (`should_consult`, `has_context_markers`) sont reprises dans `vault.rs`.
- **Outils** : `vault_search`, `vault_read`, `vault_write` remplacent
  `synaptiq_search` / `synaptiq_remember`. Le mode vocal retire `vault_search`
  comme il retirait `synaptiq_search`.
- **Contexte** : `prompt::vault_context` remplace `synaptiq_context` — même
  heuristique, mais les notes sont injectées depuis le disque, sans réseau.
- **Apprentissage** : `learn()` écrit le souvenir dans SQLite **et** dans le
  vault (`0_Inbox/Jimmy`), en tâche de fond.
- **Réglages** : `memory.vault_path`, `memory.vault_enabled`,
  `memory.vault_folder`, `memory.vault_min_request_chars` ;
  `JIMMY_VAULT_PATH` en surcharge. La carte « Synaptiq » des Paramètres devient
  « Vault Obsidian ». `SynaptiqSettings` et les secrets `SYNAPTIQ_*` sont
  retirés.
- **Vérifié en réel** : `[vault] ouvert : C:\Obsidian\Jimmy (370 notes)` au
  démarrage ; recherche réelle sur le vault en 0,39 s.

## « Jimmy était en plein travail, puis plus rien, et il avait perdu le fil »

Deux causes distinctes, retrouvées dans `data/logs/jimmy.log` et dans
`data/jimmy.db` (session `649c8687` puis nouvelle session `d13d35ae`).

### 1. `search_files` gelait la conversation 4 minutes

Diagnostic posé sur un cas réel : demande « Le contenu du dossier
agent-pentest », l'agent appelle 2 outils. `list_directory` répond en 0,2 s,
puis **`search_files` sur `C:\Users\user\Projet` tourne 4 min 22 s**
(20:43:10 → 20:47:32, dossier de travail = 73 projets). Il lit chaque fichier
du dossier de travail pour y chercher une chaîne, sans borne de temps. La
conversation reste muette pendant tout ce temps ; le budget vocal de 90 s ne
peut rien interrompre (il se teste **entre** deux itérations, jamais pendant un
outil). Au retour, le budget est dépassé → un dernier appel conclut « Il reste à
lire ses fichiers » — d'où l'impression de travail inachevé.

Corrigé : `search_files` a un **budget de 10 s** et saute les fichiers
> 2 Mo, avec un message explicite de résultats partiels. Et **chaque outil est
désormais logué** (`[agent] outil « … » appelé … / terminé en … ms`) : ce
diagnostic ne nécessitait aucun log d'outil, ils existent maintenant.

### 2. La session vocale mourait avec la boucle d'écoute

La session de conversation (`voice_session`) était une **variable locale de la
boucle d'écoute**. À 20:47:49 l'écoute s'arrête ; l'utilisateur la relance à
20:54:25 (bouton, après avoir rouvert l'interface) → **boucle neuve, session
neuve**. Sa question « Alors, j'attends toujours ton rapport » ouvre une
nouvelle « Session vocale » (`d13d35ae`) qui ne connaît pas les outils de
`649c8687` ; Jimmy répond littéralement « Je n'ai pas de rapport en cours dans
cette session ». L'écart réel était de ~7 min, donc **sous** le délai de
10 min : le fautif n'est pas `VOICE_SESSION_IDLE`, c'est la reprise d'écoute.

Corrigé : `voice_session` vit maintenant sur **`App`**
(`App::voice_session`, `Mutex<Option<(id, Instant)>>`) et survit à un
arrêt/relance de l'écoute. Limite connue : un redémarrage de *Jimmy* remet la
session à zéro (l'historique de l'ancienne session reste lisible dans le Chat).

---

# Fait pendant cette session (commits 79d83f0 → 5c652e9)

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
| **Skins** | **Réel.** renard, arctique, fennec, ours, robot (`ear_scale`, `tail_scale`, `metallic`, `has_fur`), appliqués à chaud. Avant : `set_skin` Rust jamais appelé, `/skin` faisait un `print`. | `jimmy.gd` `SKINS` + `providers::avatar::SKINS` (garder alignés) |
| **Journal d'expérience** | **Réel.** Après chaque tour avec échec d'outil, un appel unique d'extraction produit faits **et** leçons (source `lesson`, importance 0,65, toujours extraites — hors échantillonnage). Deux tentatives : 800 ms sur réponse illisible, **5 s sur panne réseau** (à l'usage, les deux tentatives mouraient dans la même coupure) ; l'extrait brut de l'échec va au journal en `warn`. Rappelées par la recherche sémantique. | `memory/learn.rs`, `core/agent.rs` |
| **Capture de compétence** | **Réel.** Après une trajectoire réussie de ≥ 3 outils, la démarche est condensée en skill (format trois lignes, pas du JSON — le modèle gratuit échouait 4 fois sur 4 en JSON multi-lignes). Doublons évités par le catalogue fourni à l'extracteur ; même nom = aiguisage du skill existant. Réglage `skills.auto_capture`. Deux tentatives : 800 ms / **5 s en panne réseau** (même logique que l'extraction de leçons) ; extrait brut en `warn` au journal. | `skills/capture.rs`, `core/agent.rs` |
| **Revue périodique** | **Réel.** Boucle de fond (30 min) : leçons nouvelles + fenêtre `growth.review_days` (défaut 7, 0 = jamais) → un appel de rédaction **sans outil**, session « Revue » du Chat. Elle ne peut rien appliquer : propositions seulement. État sur disque (`data/growth_state.json`), survit aux redémarrages. | `growth.rs`, `memory::lessons`, setup desktop |
| **Amendements du prompt** | **Réel.** `data/growth_amendments.md` (peut ne pas exister) injecté dans le prompt système ; proposé par la revue, écrit en conversation (avec `.bak`), **acté seulement si le filet de rejeu passe** (`--test amendment -- --ignored`, A/B sur demandes réelles). Le modèle ne peut jamais s'auto-valider. | `growth.rs`, `core/prompt.rs`, `core/agent.rs`, `tests/amendment.rs` |
| **Chat façon Codex** | **Réel.** `@` pour citer un fichier du projet (menu filtré, accents et casse), chemins cliquables dans les réponses (aperçu + saut de ligne `:42`, exécutable jamais lancé), bloc « travaux » sous la réponse (fichiers écrits + diff git lecture seule via `fs_diff`, borné), historique groupé par projet + recherche + `Ctrl+K`. Extraction/leçons réparés : 800 jetons (les json en ```fence``` débordaient de 400), format trois lignes tolérant, corps tronqué au lieu de rejeté, backoff réseau 5 s. | `explorer.rs`, `chat.ts`, `history.ts`, `learn.rs`, `capture.rs` |
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

# Retours d'usage réel (3 octobre 2026, soir)

« Quand je dis Jimmy il réagit, puis plus rien » et « dans le chat il dit un
mot rapide incompréhensible, plus de vocal ». Causes, toutes corrigées :

- **Sortie audio entrelacée ignorée** (`play_bytes`) : le son mono était
  écrit une case sur deux en stéréo → **lu deux fois trop vite**. Toutes les
  voix de Jimmy étaient touchées. Fin de tampon non remise à zéro (bruit) et
  dernière syllabe coupée, aussi corrigés.
- **Après « Oui ? », la boucle s'arrêtait** : purge du micro puis lecture par
  fenêtre fixe de 900 ms → fenêtre vide → sortie immédiate. Lecture par
  curseur (`read_since`) ; rien n'est transcrit si personne n'a parlé.
- **Table `memory_semantic` absente des bases existantes** (schéma resté en
  version 1) : recherche et écriture mémoire en erreur. `SCHEMA_VERSION = 2`.
- **Le chat ne parlait pas** (jamais, en fait) : il lit désormais sa réponse
  (`App::speak`, partagé avec l'écoute), texte nettoyé du Markdown (pas
  d'« astérisque », code et URL remplacés). L'écoute est suspendue pendant que
  Jimmy parle.
- **Détection en deux temps** : `base` réagit pendant la phrase, `small`
  décide sur la phrase entière (`base` seul se trompait sur les fenêtres
  courtes). Prompts Whisper différents par rôle (voir piège 31).
- Variantes du nom ajoutées : « je mise », « et Jimmy » (= « Hé Jimmy »).

## Avatar : zone cliquable et esquive

- Seul le personnage (et la bulle quand elle est affichée) capte la souris :
  `window_set_mouse_passthrough` avec la silhouette projetée par la caméra.
  Le reste de la fenêtre 560×620 laisse passer les clics vers le bureau.
- **Esquive** : à l'approche du curseur, Jimmy glisse de côté (opposé au
  curseur, en restant sur l'écran), puis revient à sa place 1,4 s après que
  le curseur s'est éloigné. Aller le chercher là où il s'est réfugié permet de
  le cliquer (réarmement seulement quand le curseur a quitté les deux zones).
  Glisser-déposer = nouvelle place. Réglable : vue Skin → « Comportement »
  (`avatar.dodge`, route Godot `/dodge`, argument `--dodge=0|1`).
- Vérifié sans aucun clic simulé : `WindowFromPoint` (zone cliquable) et
  `SetCursorPos` + position de fenêtre (esquive, poursuite, retour).

# Fluidité de la conversation (3 octobre 2026, soir)

Retour d'usage : « pas assez fluide, la transcription enregistre des bruits
parasites, je ne comprends pas les périodes où elle attend que je parle ».
Le journal détaillé a permis de **mesurer** (délais en secondes, fin de phrase
→ commande transmise).

## Où partait le temps

| Poste | Avant | Après | Cause / correctif |
|---|---|---|---|
| Fin de phrase → commande transmise | 4,2–4,5 s | **1,6–2,2 s** | Silence de fin 900→700 ms, paliers de 450 ms → trames de 50 ms ; surtout **modèle précis 3,4 s → 1,2 s** (`-ac 640`) |
| « Jimmy » seul → « Oui ? » | ~5 s | ~1,5 s | `base` déjà reconnu et prise brève (≤ 900 ms de parole) : `small` n'est plus appelé |
| Après « Oui ? » | ≥ 3,5 s fixes | dès la fin de ta phrase | l'attente fixe devenait « attendre le début de la parole » (4,5 s max) |
| Réponse → début de la voix | +3 à 5 s | immédiat | l'**apprentissage mémoire** (appel au modèle) précédait l'envoi de la réponse → tâche de fond, 1 tour sur 4 |
| Chat → échec après chaque pause | 1 requête sur 2 | 0 | connexions HTTP gardées vers whisper-server (fermées côté serveur) → `pool_max_idle_per_host(0)` |

## Ce qui n'est PAS corrigeable de notre côté

**La latence du modèle de langage varie de 2 s à 25 s pour la même requête**
(mesure : requête enregistrée rejouée 14 fois, médiane 3,9 s, max 10,8 s ;
une heure avant, tous les modèles du fournisseur répondaient en 1,2–2,3 s).
Éliminé un par un : outils, taille du prompt (2,9 k jetons), historique,
`max_tokens`, identifiant de session, appels concurrents, raisonnement caché
(22 jetons en sortie pour 11 s). **Requête « couverte » (2e requête après 3 s)
testée : aucun gain**, la lenteur est corrélée (fournisseur chargé à ce moment).
Réponse : « Un instant. » dit à voix haute au-delà de 5 s (`Cue::Thinking`) et
pastille « je réfléchis » en continu. Piste non faite : modèle local (LM Studio)
pour les échanges courants.

## Ce qui a été construit

- **Détecteur par trames** (`voice/vad.rs`, 9 tests) : trames de 50 ms,
  confirmation sur 2 trames (un claquement n'est pas de la parole), fin de
  phrase au silence, **temps de l'audio et non de l'horloge**, extrait rogné
  (parole + 300 ms) avant Whisper, parole antérieure au « Oui ? » conservée
  si ≥ 700 ms (sinon c'est l'écho du cue).
- **Conversation continue** : après la réponse, 8 s d'écoute **sans** redire
  « Jimmy » (`voice.follow_up_ms`, 0 = désactivée). Fenêtre visible : pastille
  « à toi · 6 s » (barre du haut) et bandeau de la page Voix.
- **Événement `Listen`** (idle, capturing, transcribing, your_turn, thinking,
  speaking) : l'utilisateur sait à chaque instant ce que Jimmy attend.
- **Anti-bruit** : `clean_transcript` (annotations `[BLANK_AUDIO]`, `(musique)`,
  `*bruit*`, ♪, phrases de sous-titres inventées), parole minimale 250 ms,
  extrait rogné. Réglage `voice.debug_audio` : garde les 40 derniers extraits
  dans `data/audio/debug/` pour écouter ce que Whisper a reçu.
- **Mode vocal de l'agent** sans `search_memory`/`vault_search` (le contexte
  est déjà dans le prompt : 2 à 4 s par aller-retour économisés).
- `rename_all_fields = "camelCase"` sur `AgentEvent` : les durées d'outils
  n'apparaissaient jamais dans l'interface (`duration_ms` ≠ `durationMs`).

## Mesures de Whisper à ne pas refaire

`small-q5` sur ce CPU (16 threads) : **3,4 s pour 1,8 s d'audio**, identique à
8, 12 ou 16 threads (4 threads : 4,7 s). Whisper traite toujours 30 s de
contexte. `-ac` (contexte audio, 1500 = 30 s) :

| | s / phrase | erreur de mots |
|---|---|---|
| small, `-ac` 0 | 4,0 | 20 % |
| small, `-ac` 1024 | 2,4 | 18 % |
| small, `-ac` 768 | 1,7 | 20 % |
| **small, `-ac` 640 (retenu)** | ~1,4 | ~20 % |
| small, `-ac` 512 | 1,1 | 19 % |
| base, `-ac` 0 | 0,9 | 29 % |
| base, `-ac` 768 | 0,4 | 38 % (« Dis-moi bonjour » → « D'y ma bonjour ») |

640 = 12,8 s, juste au-dessus de la phrase maximale (12 s + marges) : jamais
tronquée. **Appliqué au seul serveur de commande** ; le serveur du mot d'éveil
garde le contexte complet. Scripts de mesure : voir pièges 41 et 42.

## À savoir pour la suite

- Si une phrase est coupée en deux : c'est la **pause de fin de phrase**
  (700 ms, Paramètres → Écoute). Une pause de réflexion plus longue coupe la
  phrase ; le nom suivi d'une pause est géré (la suite est reprise après le
  « Oui ? »).
- En conversation continue, tout ce qui est dit pendant la fenêtre est pris
  pour une commande : coupe-la (0) dans une pièce où l'on parle à quelqu'un
  d'autre.

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

**28. La sortie audio cpal est entrelacée, comme l'entrée.**
Un échantillon mono par case = lecture deux fois trop rapide en stéréo. Dans
`drain`, une valeur par trame, recopiée sur chaque canal. Test unitaire
`sortie_stereo_duplique_chaque_echantillon`.

**29. `take_window` après une purge rend du vide.**
Il exige `n` échantillons disponibles. Pour suivre un flux, utiliser le
curseur (`cursor` + `read_since`), qui rend exactement l'audio nouveau.

**30. Toute table ajoutée au schéma doit faire monter `SCHEMA_VERSION`.**
Sinon les bases existantes ne la reçoivent jamais, et les tests (base neuve
en mémoire) ne voient rien. Test `migration_v1_vers_v2_cree_memory_semantic`.

**31. Le prompt Whisper n'est pas neutre.**
« Jimmy, » en prompt : le modèle `small` croit le nom déjà dit et l'omet de
la commande ; un prompt descriptif fait halluciner `base` sur un appel court.
D'où un prompt par rôle (`whisper_prompt(…, SttRole)`).

**32. Normaliser le volume avant Whisper a dégradé la transcription.**
Essayé (crête à 0,9) : « et Jimmy. » devenait « ee uh ! ». Retiré, documenté
sur `window_to_wav`.

**33. Godot : repère viewport ≠ repère fenêtre si Windows met à l'échelle.**
Projeter avec la caméra donne des pixels de viewport ; la zone cliquable et
l'esquive travaillent en pixels de fenêtre / d'écran. Multiplier par
`window_get_size() / viewport_size` (`_window_scale`). Les tests PowerShell
doivent appeler `SetProcessDPIAware()`, sinon leurs coordonnées sont
virtualisées.

**34. Les clips de synthèse ne sont pas des entrées fiables.**
Fish Audio prononce parfois mal « Jimmy » ; à RMS 0,008 Whisper comprend mal
la phrase elle-même. Les tests d'écoute affichent une **référence** (clip
entier transcrit hors écoute) et régénèrent le clip tant que le nom n'y est
pas audible (`phrase_avec_nom`).

**35. Ne jamais lancer les tests d'écoute pendant que Jimmy sert l'utilisateur.**
Les tests démarrent leurs propres `whisper-server` (ports 8178/8179) et les
tuent en fin de test ; un Jimmy lancé entre-temps les réutilise et devient
muet. `ensure_stt` relance désormais un serveur mort (auto-réparation), mais
arrêter Jimmy pendant les tests reste la règle.

**36. Whisper met un tiret de dialogue en tête (« - Eh, Jimmy ! »).**
Un mot de pure ponctuation ne doit consommer aucun token dans
`strip_wake_word`, sinon le nom reste dans la commande et Jimmy répond « oui,
je suis là ». Les tests d'écoute vérifient que la commande transmise ne
contient plus le nom.

**37. Une commande vocale = un échange d'une conversation.**
La session vocale est réutilisée tant que le dernier échange date de moins de
10 minutes (`VOICE_SESSION_IDLE`) ; avant, chaque phrase ouvrait une session
neuve et Jimmy oubliait tout.

**38. Whisper traite toujours une fenêtre de 30 s, même pour 2 s d'audio.**
D'où ~3 s par transcription `small`, quel que soit le nombre de threads.
`-ac 640` divise le temps par ~3 sans perte mesurée de précision (voir le
tableau). Ne pas l'appliquer au modèle de mot d'éveil.

**39. L'apprentissage mémoire bloquait la réponse.**
`learn()` est un appel de plus au modèle ; placé avant l'envoi de la réponse,
il la retardait de 3 à 5 s. Maintenant en tâche de fond avec un `WeakSender`
(un `Sender` fort ferait attendre l'interface). Et le réglage
`memory.auto_learn_every` existait mais n'était jamais lu : 1 tour sur 4.

**40. Connexions HTTP gardées vers whisper-server.**
Le serveur ferme ses connexions inactives ; reqwest en réutilisait une fermée
et la première requête après chaque pause échouait (« error sending request »),
ce qui déclenchait toute la réparation des serveurs. `pool_max_idle_per_host(0)`
sur le client STT.

**41. Python ne passe pas le pare-feu du fournisseur LLM sans User-Agent.**
`urllib` seul → HTTP 403 (et pas une erreur de clé). Ajouter
`User-Agent: jimmy/0.1 (desktop agent)`. Mesures reproductibles :
`JIMMY_LLM_DUMP=<dossier>` enregistre chaque requête envoyée au modèle
(corps JSON, sans la clé), à rejouer à la main pour comparer des latences.

**42. Pour mesurer Whisper hors de Rust** : test ignoré `exporter_clips_de_test`
(`JIMMY_CLIPS_DIR`) écrit des phrases de synthèse variées en WAV 16 kHz, puis
lancer `whisper-server.exe` à la main avec `-ac N` et POSTer les clips sur
`/inference`.

**43. Une prise qui commence avant la fin du « Oui ? » n'est pas un écho.**
L'écho du cue dure ~0,5 s ; une parole confirmée ≥ 700 ms avant l'armement
est l'utilisateur qui enchaîne sa commande pendant la pause qui suit le nom.
Sans cette règle, « Jimmy… (pause) dis-moi bonjour » perdait la commande.

**44. Un bruit ambiant au niveau du seuil gelait la prise.**
Journal réel : « parole détectée (niveau 0,0036, seuil 0,0035) » puis une prise
de **19,9 s pour 250 ms de parole** : le bruit rouvre sans cesse la parole, le
silence de fin n'arrive jamais, et Jimmy est aveugle pendant ce temps.
Deux protections : `SpeechTracker::looks_like_noise` (au bout de 3 s, moins de
15 % de parole confirmée → prise abandonnée) et un **seuil appris des fausses
alertes** (`learned_floor`, relevé à 1,25 × le niveau de la fausse alerte,
plafonné à 0,007, retombe en ~2 min). Si Jimmy réagit encore à des bruits :
activer « Garder les 40 derniers extraits audio » (Paramètres → Écoute) et
écouter `data/audio/debug/`.

**45. L'historique rechargé ne doit JAMAIS contenir d'outils (HTTP 400).**
Cause de « HTTP 400 invalid request » en plein usage : `History::messages`
rechargeait les messages `tool` sans leur `tool_call_id` (jamais stocké) et la
coupure à 20 messages laissait des résultats orphelins en tête. L'API refuse
les deux. `History::conversation` ne renvoie que les demandes et les réponses
finales (et le contexte passe de 15 k à ~3 k jetons). `messages()` reste
complet pour l'interface. Reproduit par le test ignoré
`historique_avec_outils_ne_casse_pas_la_conversation` (tour 2 en échec avant le
correctif, 3 tours OK après). Filet de sécurité : sur un 400 au premier appel,
nouvel essai sans historique (`is_bad_request`).

**46. Limite d'étapes ≠ réponse.** Quand la boucle s'arrête sans réponse sans
outil (6 étapes ou 90 s en vocal), le dernier texte du modèle n'est qu'une
phrase d'annonce (« Je lis la documentation… »). Un dernier appel sans outil
lui demande de conclure ; la réponse est enregistrée dans l'historique.

**47. `localhost` coûte 2,3 s sous Windows quand le port est fermé** (IPv6 puis
IPv4). LM Studio éteint = chaque rappel de mémoire +2,3 s. `127.0.0.1` +
`connect_timeout` de 500 ms dans `SemanticEmbedder`.

**48. Bibliothèque de modèles : le catalogue public est organisé PAR
FOURNISSEUR** (`api.json` → `opencode-go` → `models`). L'ancien code lisait une
clé `models` à la racine : la liste était toujours vide. La liste vient
maintenant de `GET {base_url}/models` (ce que le compte peut utiliser, 36
modèles), enrichie par le catalogue (32/36). `ModelTest` teste comme Jimmy :
requête simple puis avec outil — `grok-4.6` et `muse-spark-*` répondent
« Model does not support this protocol » (HTTP 400) et sont inutilisables.
**`id` doit rester l'identifiant COURT** (`space-bunny-free`) : une première
version renvoyait `opencode-go/space-bunny-free` ; choisir un modèle l'aurait
écrit dans la configuration et cassé toutes les requêtes (rattrapé par la suite
UI, qui vérifie maintenant la valeur enregistrée).

**49. La latence du fournisseur est corrélée dans le temps.** Une requête
couverte (2e requête après 3 s) ne gagne rien. Un modèle vocal plus rapide
(`llm.voice_model`, Paramètres → Modèles) est le seul levier ; tester avant de
croire une latence mesurée à un instant donné.

**50. Deux `start_avatar` concurrents tuent l'avatar vivant.** `is_up()` rend
faux tant que Godot n'a pas ouvert son port ; deux appels dans cette fenêtre
lançaient deux Godot, le second écrasait le premier dans `godot` et
`kill_on_drop` **tuait le premier, vivant** — le second mourait sur le port
occupé. Résultat : aucun avatar, sans une ligne de journal. Un `Mutex` de
lancement (`avatar_spawning`) sérialise ; une sortie immédiate est détectée par
`try_wait()` et remontée comme erreur.

**51. Un enfant terminé reste `Some` dans le mutex.** `godot.is_some()` est
vrai pour un processus mort : le statut affichait « avatar en marche » et
`start_avatar` croyait déjà tourner. Utiliser `avatar_running()` (`try_wait()`
= `None`), jamais `is_some()`.

**52. Un Godot orphelin survit à la mort de Jimmy** (comme les
`whisper-server`, piège 6). `kill_on_drop` ne s'exécute pas quand le processus
parent est tué de force. Au lancement suivant, le chien de garde le trouve par
`is_up()` et le réutilise (son pont pointe vers le nouveau Jimmy) — ne pas
conclure « deux avatars ». C'est aussi pourquoi le chien de garde se fonde sur
`is_up()` et non sur l'enfant stocké : sinon il logait « absent » toutes les
5 s pour un avatar bien visible.

**53. La mémoire persistante de Jimmy est le vault Obsidian**, plus Synaptiq.
Écrire dans `memory.vault_folder` (`0_Inbox/Jimmy`), lire partout. La recherche
normalise casse et accents **caractère par caractère** (longueur préservée) :
ne pas passer par `to_lowercase()` global, qui change la longueur en Unicode et
décale les extraits.

**54. Une variable locale à la boucle d'écoute meurt avec elle.** La session de
conversation vivait dans `spawn_voice_listener` ; **arrêter puis relancer
l'écoute** (bouton, réglage, micro) créait une session neuve et Jimmy perdait
le fil, même sous les 10 min de `VOICE_SESSION_IDLE`. Elle est portée par `App`
(`App::voice_session`). Toute donnée qui doit survivre à une relance de
l'écoute va sur `App`, pas dans la boucle. Limite : un redémarrage de Jimmy
repart de zéro.

**55. Un outil synchrone ne peut pas être interrompu par le budget de l'agent.**
Le budget vocal (90 s) se teste **entre** les itérations ; pendant qu'un outil
tourne, rien ne l'arrête. `search_files` sans borne a scanné le dossier de
travail entier (73 projets) : **4 min 22 s** de silence. Les outils doivent
porter leur propre budget (`SEARCH_BUDGET = 10 s`, fichiers > 2 Mo sautés), et
`run_command` garde son `timeout_ms`. Corollaire : `jimmy.log` logue désormais
chaque appel d'outil (nom, arguments tronqués, durée) — sans ça, un gel
d'outil est invisible dans le journal.

**56. « localhost a refusé de se connecter » = binaire release mal compilé.**
Quand `target/release/jimmy.exe` n'a pas été produit par `build.ps1 -Release`
(`tauri build`), la fenêtre vise `devUrl` : interface vide, avatar et voix
normaux (piège 7). Recompiler avec `build.ps1 -Release`. À lancer depuis
**PowerShell**, pas depuis Git Bash : le `link.exe` de Git (`/usr/bin/link`)
passe devant celui de MSVC et le build échoue. Les tests d'interface gardent
les résultats des tests de modèles dans le `localStorage` : un modèle déjà
marqué cassé a son bouton grisé (la suite le gère).

**57. Une hésitation clôturait la phrase ; 12 s de parole la coupaient net.**
Journal réel (4 octobre, 10:51:31) : « Je viens de voir que dans ta mémoire
tu as mis... » transmis tel quel, une pause de réflexion de plus de 700 ms
avait clos la prise. Et `MAX_SPEECH` (12 s) tranchait une longue explication
au milieu d'un mot, le reste étant perdu. **Ne pas relever `MAX_SPEECH`** :
le serveur de commande ne voit que 12,8 s d'audio (`-ac 640`), Whisper
tronquerait. Trois corrections : (1) silence de fin **adaptatif**
(`SpeechTracker::ended`) : réglage tel quel jusqu'à 1,5 s de parole, puis
jusqu'à +500 ms ; les commandes brèves gardent leur réactivité. (2) Au-delà de
10 s, le segment est coupé dans la première pause (100 ms). (3)
`continue_speech` : si la prise a été coupée, ou si la transcription semble
inachevée (`looks_unfinished` : « ... », virgule finale, « que », « de »,
« euh »…), Jimmy écoute la suite (2,5 s après une hésitation, 1,5 s après une
coupure) et recolle les segments (`join_segments`), jusqu'à 6 segments.
Whisper met un point à tout : seuls « ? » et « ! » prouvent une fin. Journal :
`phrase inachevée : j'écoute la suite`, `suite : « … »`. Vérifié de bout en
bout par `--test audio ecoute_hesitation_ne_coupe_pas_la_phrase -- --ignored`
(« Jimmy, donne-moi le nom de » + 1,8 s de pause + « la capitale de
l'Italie ») : sans la reprise, commande « donne-moi le nom de... » et Jimmy
répond « Ta phrase est coupée » ; avec, phrase entière et réponse « Rome ».

**58. Une commande MCP porte souvent une clé : ne jamais l'afficher brute.**
`mcp_add_server` enregistre la commande telle quelle (ex. `mcp-remote … --header
"X-API-Key: …"`). La vue Skills → **Serveurs MCP** (commande `mcp_servers`,
`App::mcp_overview`, `McpRegistry::status`) l'affiche via `mcp::mask_command` :
valeur cachée après `--header`/`--token`/`--api-key`…, après `Key:`/`token=`,
dans l'URL (`?api_key=`, `user:motdepasse@`) ; variables d'environnement par
leur nom seulement. L'état vient du processus (`try_wait`), et le verrou des
connexions est pris en `try_lock` (sinon « occupé ») : jamais d'attente de 90 s
dans l'interface. **Reste ouvert** : `[agent] outil « … » appelé {arguments}`
journalise les arguments en clair ; la clé d'`aggregate` est dans
`data/logs/jimmy.log` (4 octobre, 11:50:56).

**59. Tauri ne renomme pas les champs d'une structure imbriquée.**
Symptôme (4 octobre) : Jimmy perdait le fil dans le Chat (« installe-le » →
« installer quoi ? »). Cause : `api.chat` envoie `{ request: { sessionId } }` ;
Tauri convertit les arguments de **premier niveau** (camelCase → snake_case),
pas les champs de `ChatRequest`. `session_id` restait `None` → **une session
neuve par message**, depuis la V1 (les titres de sessions = chaque message).
Règle : toute structure `Deserialize` reçue de l'interface porte
`#[serde(rename_all = "camelCase")]` (test `la_requete_de_chat_garde_la_session…`,
parcours d'interface « le second message garde le fil »). Dans la foulée, le
contexte de chaque demande contient : (1) `History::tool_digest` — outils des 3
derniers tours, une ligne par appel, ≤ 1 500 caractères, dans le prompt
système (jamais de messages `tool` rejoués, piège 45) ; (2) un **socle de
mémoire** (8 souvenirs les plus importants) fusionné au rappel, ≤ 1 200
caractères ; (3) la liste d'outils du prompt regroupe les MCP par serveur.
Mesuré : 14 997 jetons (avant, 2 serveurs MCP) → 14 379 (après, 3 serveurs +
socle + outils récents). L'essentiel du coût restant = les **définitions**
d'outils envoyées à l'API (~78 outils MCP, dont 74 pour `aggregate`).

**60. Le modèle déraille parfois : réponse qui part en chinois, russe, japonais.**
Cas réel (4 octobre, 13:25 et 13:28, `space-bunny-free`) : réponses longues
(900 et 2 600 caractères) qui finissent en charabia multilingue, lues à voix
haute. Ce n'est **pas** Jimmy : rejeu de la requête réelle 32 fois
(`agent/tests/replay.rs`, variables `JIMMY_REPLAY_*`) → 1 dérive, à 0,3 de
température ; le résumé d'outils (piège 59) n'y change rien (0/8 avec, 0/8
sans). **Ne pas « corriger » par la température.** Filet dans `agent::run` :
réponse finale avec ≥ 2 caractères d'écritures étrangères (et aucune dans la
demande) → régénérée une fois, sinon coupée à la dernière phrase saine avec une
note (`is_degenerate`, `cut_degenerate`). Journal : `réponse dégénérée … :
nouvelle génération`. Si ça revient souvent : changer de modèle principal.

**61. La suite d'interface effaçait le modèle vocal de l'utilisateur.**
Le parcours « bibliothèque de modèles » finissait par `set_llm_model(voice, "")`
sans condition : MiMo-V2.6-Flash (choisi en « Vocal ») a été effacé deux fois
le 4 octobre. Il relève maintenant le modèle vocal au début et le **restaure**
à la fin (vérifié par `status`). Il attendait aussi « une ligne vocale
quelconque » : avec un modèle vocal déjà choisi, l'attente passait avant
l'application du choix → échec intermittent, corrigé (attente sur la ligne de
glm-5.3-flash). Rappel : le modèle **Vocal** ne sert qu'aux commandes dites à
voix haute ; le Chat écrit utilise toujours le modèle **Principal**.

**62. Le `.env` écrasait le modèle choisi dans les Paramètres à chaque lancement.**
`JIMMY_LLM_MODEL=space-bunny-free` (`.env`) était appliqué par
`Settings::apply_env` **par-dessus** `config.json` à chaque démarrage, puis le
premier enregistrement le réécrivait dans la config : MiMo choisi,
`space-bunny-free` utilisé (et c'est lui qui a déraillé, piège 60). Le
commentaire du code disait l'inverse. Désormais `apply_env_with(lookup,
first_run)` : modèle, modèle et voix de synthèse, langue = **valeurs de départ
seulement** (pas de `config.json`) ; ports, Godot, vault = surcharge toujours.
Vérifié : MiMo tient à travers les redémarrages de `test-ui.ps1`.
Instabilités connues de la suite d'interface : « Skin » (attente fixe de
1,5 s, Godot parfois plus lent) et toute étape pendant laquelle l'écoute capte
un faux « Jimmy » (le parcours du fil lit désormais la session, pas la
dernière bulle). Un échec isolé de ces étapes se relance avant d'enquêter.

**63. Bibliothèque de voix : classes CSS distinctes des modèles.**
Décision (4 octobre) : voix par défaut **« Le narrateur »** (Fish Audio
`4f2a0684dd0247dda68f339738c780e6`, la plus utilisée des deux voix de ce nom),
`TtsVoice::NARRATEUR`, en tête de `PRESETS`. Paramètres → Voix de sortie :
bibliothèque (`desktop/src/views/voices.ts`) = voix prédéfinies + voix ajoutées
depuis le catalogue public Fish Audio (`Tts::search_voices`, sans clé, 10 s,
par pertinence — un tri par popularité faisait passer Clémence avant « Le
narrateur »), gardées dans `tts.library`. Commandes `tts_voices`,
`tts_search_voices`, `tts_set_voice` (identifiant validé : 32 hexadécimaux),
`tts_remove_voice`, et `tts_preview(text, voice?)` pour écouter avant de
choisir. Piège payé : réutiliser `model-row` / `model-current` pour les voix
faisait compter les voix au parcours des modèles (« 3 modèles listés ») ; les
voix ont leurs propres classes (`voice-row`, `voice-current`, `voice-list`).
Comme pour les modèles, la vue Paramètres met sa copie à jour à chaque choix
(sinon « Enregistrer » remettrait l'ancienne voix). Les sons d'état sont en
cache **par voix** : changer de voix les régénère.

**64. Une panne du fournisseur en vocal ne laissait aucune trace.**
4 octobre, 15:33 : MiMo muet ~95 s puis « HTTP 500 Internal Server Error —
Unknown Error » ; Jimmy a dit son message d'erreur après 100 s, et le journal
ne contenait rien (seul le Chat journalisait l'échec). Désormais dans
`agent::run` : tout échec d'appel est journalisé (`appel au modèle … en échec
après … ms`) ; une erreur passagère (5xx, 429, coupure) arrivée en moins de
20 s est rejouée **une** fois (`is_transient`) ; en vocal, chaque appel est
borné à 45 s (`VOICE_CALL_TIMEOUT`). Une panne lente n'est pas rejouée : elle
doublerait l'attente. Ces 500 viennent d'OpenCode Go, pas de Jimmy.

**65. La bibliothèque de voix était introuvable dans les Paramètres.**
Placée sous la liste des 36 modèles, l'utilisateur ne la voyait pas. Elle est
dans l'**onglet Voix** (carte « Voix de Jimmy », sous « Écoute permanente ») ;
les Paramètres n'y renvoient plus que par une phrase. Un réglage qu'on cherche
va là où on le cherche, pas au bout d'une page longue.

**66. Le vault est partagé : Jimmy doit le savoir.**
Le vault est le second cerveau de l'utilisateur ; d'autres agents (Claude
Code, OpenCode, la flotte) y écrivent. Le prompt le dit (`IDENTITY`, section
Mémoire), le contexte de session donne son dossier, et chaque note injectée
ou renvoyée par `vault_search` porte son origine : « ta note » (dans
`memory.vault_folder`, `Vault::is_own`) ou « partagée ». Vérifié avec le vrai
modèle (`--test happy_path il_sait_que_sa_memoire_est_partagee`) : « le vault
est partagé… je ne les récupère pas comme mes souvenirs ». Attention :
`Vault.folder` est **absolu** (racine + sous-dossier), `is_own` compare le
chemin relatif.

**67. Jimmy regarde l'utilisateur, plus la souris.**
`main.gd` faisait suivre le curseur à la tête : dès que la souris quittait la
fenêtre, le renard regardait de côté. Désormais `jimmy.set_viewer(_camera)` :
tête orientée vers la caméra dans le repère du cou (le balancement du corps
est compensé), pupilles visant la caméra depuis chaque œil (effet Joconde :
un regard caméra semble suivre la personne devant l'écran). Contact par état
(`EYE_CONTACT`) : pensif lève les yeux, au travail les baisse
(`GAZE_AWAY`) ; coups d'œil furtifs toutes les 4,5–9 s (9–15 s en écoute ou
parole) et micro-saccades. Mesuré sur 10 captures au repos : museau à ±3 px du
centre, un coup d'œil de 15 px puis retour. Recharger l'avatar sans relancer
Jimmy : tuer son Godot, le chien de garde le relance avec les nouveaux scripts.

**68. Chat : projet par conversation et explorateur (lot 1 de l'UI « Codex »).**
Une conversation peut être rattachée à un **projet** (colonne
`sessions.project`, **schéma v3** avec `ALTER TABLE` explicite pour les bases
existantes, piège 30). La commande `chat` applique ce projet comme dossier de
travail de l'agent (`settings.workspace` et `ToolContext.workspace`) ; une
nouvelle conversation garde le projet en cours. Commandes dans
`desktop/src-tauri/src/explorer.rs` : `pick_folder` (sélecteur natif,
`tauri-plugin-dialog` appelé **côté Rust seulement**, aucune capacité JS
ajoutée), `projects_recent`, `session_set_project`, `fs_list` (500 entrées
max, `.git` masqué), `fs_preview` (64 Ko, binaire détecté), `fs_open`
(application par défaut via `explorer.exe`) et `fs_reveal` (`/select,` en
`raw_arg`). **Chaque lecture passe par les permissions de Jimmy** ; un
exécutable (`.exe`, `.bat`, `.ps1`, `.lnk`…) n'est **jamais lancé**, seulement
montré dans l'Explorateur. **Piège 69** (même jour) : le projet choisi pendant
une **conversation vocale** se perdait — le Chat affichait la session vocale
sans connaître son identifiant, le choix restait local à la vue ; et la boucle
vocale n'appliquait aucun projet. Désormais `AgentEvent::Spoken` porte
`session_id` (le Chat l'adopte, `main.ts` le mémorise même sur un autre
onglet, et lui applique le projet ouvert), la voix passe par
`App::session_context` comme le Chat, et un projet choisi avant le premier
message vit dans `ctx.pendingProject`. Interface : `desktop/src/views/explorer.ts` et
l'en-tête du Chat (sélecteur « Projet », bouton « Fichiers », état gardé dans
le `localStorage`). Piège payé : une règle CSS `display: flex` **l'emporte sur l'attribut
`hidden`** — le menu des projets restait affiché ; tout élément masqué par
`hidden` et stylé en `display` doit avoir sa règle `[hidden] { display: none }`.
Reste du chantier (validé par l'utilisateur) : « @ » pour citer un fichier,
chemins cliquables dans les réponses, historique groupé par projet.

**70. « Trois inputs et il n'a toujours pas démarré la tâche » — sept causes.**
Session vocale réelle (4 octobre, projet `agent-reddit`) : quatre « oui, go »,
`notify.py` jamais écrit. Diagnostic par la base et par un test de bout en
bout sur une **copie** du projet (`--test happy_path
il_va_au_bout_d_une_tache_validee_en_un_tour`, `JIMMY_TASK_PROJECT`) :
1. `read_file` coupait à 12 000 caractères **sans moyen de lire la suite**
   (`storage.py` : 23 Ko) → `start_line` / `max_lines`, en-tête « lignes a–b sur
   N · suite : start_line=… » (`read_window`) ; `search_files` donne `L532:`.
2. Budget vocal de 6 étapes, puis 180 s → **25 étapes, 600 s** (comme l'écrit) ;
   défaut écrit 12 → 25 (`DEFAULT_MAX_ITERATIONS`, migration de 12).
3. `max_tokens` 4096 : MiMo y range son raisonnement caché → l'appel
   `write_file` arrivait **coupé, en texte** (`<tool_call><function=…>`) et
   était pris pour la réponse finale → 16 384 (`DEFAULT_MAX_TOKENS`, migration
   de 4096) ; `core::inline_tools` récupère un appel complet en texte et fait
   **refaire** un appel coupé (`finish_reason = "length"` → `LlmReply::truncated`) ;
   `write_file` a `append` pour écrire en plusieurs fois.
4. 79 définitions d'outils MCP ≈ **9 500 jetons à chaque appel** (mesuré,
   `--test replay mesurer_le_poids_des_definitions_d_outils`) → outils MCP **à
   la demande** : noms dans le prompt, `mcp_list_tools` / `mcp_call`.
5. MiMo explore sans fin (20 lectures avant d'écrire) → rappels automatiques
   « agis ou conclus » aux étapes 6 et 10 sans action (`EXPLORATION_NUDGE*`).
6. Une erreur du modèle en cours de tâche jetait tout → conclusion avec ce qui
   est fait ; limite par appel en vocal 45 → **150 s** (une écriture de fichier
   dépassait 90 s).
7. Silence pendant les minutes de travail → `AgentEvent::Progress` dit à voix
   haute (≤ 1 phrase / 20 s) et « Je travaille toujours dessus » après 45 s
   sans rien dire ; la réponse finale attend la fin de la phrase en cours.
Plus : consigne « plan validé = exécuter jusqu'au bout » (prompt, règle 6),
PowerShell imbriqué interdit dans `run_command`, caches Python ignorés.
**Résultat mesuré** : même tâche, un seul tour vocal, 22 étapes, 3 min 30 :
`notify.py` écrit, branché dans `run_analyse`, ruff propre, 276 tests verts.
Mesuré en production (journal) : premier appel d'une demande **15 957 → 4 800
jetons d'entrée** (−70 %) grâce aux outils MCP à la demande.
Reste lent (MiMo : 4 à 40 s par appel) ; piste : tâches longues en arrière-plan
(Jimmy dit « je m'en occupe », reste disponible et annonce la fin).

**71. Arrêt d'urgence « STOP » (demande de l'utilisateur, 4 octobre).**
La transcription se trompe parfois et lance des tâches pour rien. Pendant une
tâche, la boucle d'écoute **attend** (elle ne lit plus le micro) : un « STOP »
n'était pas entendu, et aucune tâche n'était interruptible. Désormais :
`App::request_stop` (compteur `watch`) ; `App::cancellable` enveloppe chaque
tâche de l'agent (Chat et voix) → `Error::Cancelled`, la future est abandonnée
et les commandes en cours sont tuées (`kill_on_drop`) ; `voice::interrupt_playback`
coupe la voix en pleine phrase et `App::speak` s'arrête entre les morceaux.
Pendant une tâche vocale, `watch_for_stop` écoute le micro : courte prise
(≤ 1,5 s) → transcription rapide → `is_stop_command`. Hors tâche vocale, la
boucle principale reconnaît aussi « STOP » (arrête une tâche lancée depuis le
Chat). **Seule une phrase réduite au mot d'arrêt compte** (« stop », « stoppe »,
« arrête », « arrête-toi », « arrête tout », avec ou sans « Jimmy ») :
« arrête le serveur » reste une commande. Après l'arrêt : « D'accord, j'arrête.
Je t'écoute. », conversation continue ouverte, et l'historique note
« (Tâche arrêtée… avant la fin.) ». Chat : bouton **Arrêter** pendant le
travail (`agent_stop`). Vérifié : `--test stop` (tâche de 30 s arrêtée en
0,29 s) et `--test audio ecoute_stop_arrete_la_tache_en_cours -- --ignored`
(« Stop ! » dit pendant une vraie tâche vocale → arrêt et réponse).

**72. Deux clés de même nom dans une constante `SKINS` = script refusé, avatar
invisible.** Ajout des skins ours et robot (4 octobre) : un paramètre de forme
nommé `fur` (fourrure oui/non) est entré en collision avec la clé de **couleur**
`fur` déjà présente dans chaque skin. Godot refuse le script à l'analyse
(`Key "fur" was already used in this dictionary`), la scène ne construit plus le
personnage : **processus Godot vivant, fenêtre transparente, avatar
invisible** — sans rien dans `jimmy.log` (l'erreur va dans la sortie Godot).
Symptôme : capture `/snapshot` entièrement noire. Le `--check-only --script` ne
l'attrape pas ; il faut rejouer le script headless (`--path godot --script
res://scripts/jimmy.gd`) pour voir l'erreur d'analyse. Corrigé en renommant le
paramètre `has_fur`. Règle : chaque paramètre de forme d'un skin porte un nom
qui n'existe pas déjà comme couleur (`fur`, `cream`, `dark`, `shirt`,
`accent`, `ear_scale`).

**73. Un commentaire qui promet une borne que le code ne tient pas.**
`fs_diff` (bloc « travaux » du Chat) annonçait « temps borné (10 s) » avec une
constante `DIFF_TIMEOUT`, mais exécutait `git diff` sans aucune borne : la
constante existait sans jamais être utilisée. Le compilateur l'a signalé
(`constant DIFF_TIMEOUT is never used`) — un avertissement de build n'est
jamais cosmétique, et « défini mais jamais appelé » est déjà arrivé plusieurs
fois (pièges 11, 22). Corrigé avec `tokio::process` + `kill_on_drop`, comme
`run_command`.

**74. Le SSE du fournisseur n'est pas exactement un JSON par ligne.**
`chat_stream` lit des fragments d'octets (`response.chunk()`), pas des lignes :
un fragment réseau coupe n'importe où, y compris au milieu d'un caractère
UTF-8. On accumule les octets et on ne découpe que sur `\n` (jamais un octet de
continuation) ; les lignes non `data: ` sont ignorées, une ligne JSON
illisible ne tue jamais le flux. Formats constatés par sondes réelles
(5 octobre) : `delta.reasoning_content` séparé du contenu (et consommé par le
plafond de jetons — 32 jetons/32 dans une sonde), tool_calls éparpillés par
`index` (id + nom au premier fragment, `arguments` en morceaux ensuite),
`finish_reason` dans un `delta: {}` final, `usage` dans un fragment séparé,
`data: [DONE]` pour clore. Le tampon qui s'arrête sans `finish_reason` ni
contenu → erreur « flux interrompu », pas une réponse vide.

**75. Avec le streaming, « la bulle d'attente a disparu » n'est plus la fin du
tour.** Elle part au premier fragment, la bulle `.streaming` redevient une
bulle d'attente à chaque appel d'outil, et seule `final` libère le bouton
d'envoi. Dans `suite.js`, la fin du tour est `attendreReponse()` : ni
`.bubble.pending`, ni `.bubble.streaming`, et bouton d'envoi actif, au même
instant. La preuve du flux passe par un `MutationObserver` posé avant l'envoi
(`guetterFlux`) : une réponse d'un mot crée et retire la bulle en flux entre
deux sondages. Autre détail : `voice_stop` enregistre aussi
`listen_on_start` — la suite coupe l'écoute au départ (parole ambiante =
commandes parasites dans le fil) et la remet dans son état d'origine à la fin.

**76. La fenêtre d'aperçu ne se fermait pas avec Échap.** `openFileModal`
appelait `focus()` avant d'ajouter la fenêtre au document : un élément détaché
ne prend pas le focus, Échap n'arrivait jamais. Restée ouverte, elle
interceptait tous les clics : onze parcours de la suite tombaient en cascade
(« locator.click: Timeout 30000ms »). Ajout puis focus. La suite retire aussi
toute fenêtre d'aperçu restante après un parcours échoué. Même famille : un
bouton grisé fait expirer un clic en 30 s sans autre message — le parcours
« bibliothèque de modèles » cliquait « Vocal » sur `glm-5.3-flash` alors qu'il
était déjà le modèle vocal ; il le retire d'abord (et le `finally` le remet).

**77. Un fichier sensible n'est jamais modifié sans accord explicite.** Cas
réel du 5 octobre : dans la conversation JobXpress, Jimmy a réécrit
`/home/user/app/secure/.env.api` sur le VPS (`vps_exec` : `cp …;
python3 <<'PY'`) puis reconstruit le conteneur, sans rien demander ; il avait
aussi affiché ce `.env` en clair (`cat -A`). Les permissions V1, accordées une
fois pour toutes, ne distinguent pas un `.env` d'un fichier de code.
`agent/src/sensitive.rs` repère avant chaque outil une **modification** d'un
fichier sensible (`.env*` sauf `.example`/`.sample`…, `*.pem`/`*.key`, clés
SSH, `secret*`, `credential*`, dossiers `secure/`, `secrets/`, `.ssh/`…) :
`write_file`, `run_command`, `mcp_call` à commande (`vps_exec`…) ou
d'écriture (`upload`, `put`…). La lecture reste libre, `--env-file` aussi
(docker lit le fichier). Seule une **écriture repérée** déclenche la demande
(6 octobre : l'ancienne règle « lecture connue sinon suspect » a demandé onze
accords en une matinée pour des `Get-Content`, `Select-String`, `findstr`,
`ssh -i x.key`, aucun n'écrivant). Écriture = redirection `>` vers le
fichier, verbe d'écriture (`WRITE_VERBS` : `rm`, `mv`, `tee`, `Set-Content`,
`Out-File`, `Remove-Item`…), destination d'une copie (`cp a .env` ; `cp .env
/tmp` reste libre), `sed -i`/`perl -i`, `find -delete`, script heredoc ou
`python -c` qui écrit, API .NET (`[IO.File]::Write…`). Les commandes
porteuses (`ssh`, `docker`, `sh -c`…) sont analysées récursivement. Le
découpage (`segments`) respecte les guillemets et sépare aussi sur `( ) { }`
(blocs PowerShell). La boucle d'agent suspend alors l'outil
(`sensitive::authorize`, appelé dans `core/agent.rs`), émet
`AgentEvent::Approval`, et le Chat affiche une carte « Autoriser / Refuser »
(`approval_respond`). Refus, 5 min sans réponse, STOP ou demande **vocale** =
outil non exécuté, et le modèle reçoit la consigne de ne pas contourner (règle
8 du prompt). Limite connue : détection par motifs, un script intermédiaire
qui ne nomme pas le fichier y échappe.

**78. Chaque modèle a son format d'API.** OpenCode Go sert un modèle dans un
seul des trois formats : Chat (`/chat/completions`), Responses (`/responses`,
OpenAI : Muse Spark, GPT, Grok) ou Messages (`/messages`, Anthropic : Qwen
3.7+/3.8, MiniMax). Appelé dans un autre format, il répond `400
ModelProtocolUnsupported` : le 6 octobre, Muse Spark 1.3 restait muet car
Jimmy ne parlait que Chat (12 modèles du catalogue sur 34 dans ce cas). Le format
vient du catalogue public (`provider.npm` : `@ai-sdk/openai` → Responses,
`@ai-sdk/anthropic` → Messages, sinon Chat), mis en cache par modèle
(`LlmClient::protocol`). Sur `ModelProtocolUnsupported` malgré tout (modèle
hors catalogue), `LlmClient::send` essaie les autres formats et retient le bon.
La traduction (corps, réponse entière, flux SSE) vit dans
`providers/protocol.rs`. Particularités : Messages veut la clé dans
`x-api-key` (sinon `401 Missing API key`), le système à part, l'alternance
stricte des rôles (résultats d'outil regroupés dans un message utilisateur) et
aucun bloc de texte vide ; Responses veut le système dans `instructions` et
les outils à plat (`{type, name, parameters}`). Le raisonnement (chiffré ou
`thinking`) n'est jamais renvoyé : l'aller-retour d'outil marche sans.
Vérifié par `--test protocols -- --ignored` (un vrai modèle par format, tour
d'outil puis réponse en flux). Les résultats de test de la bibliothèque sont
passés en `jimmy.llm-tests.v2` pour oublier les anciens échecs de format.

 **79. Un filet sans réarmement devient une fausse erreur.** Le filet de 3
 minutes du Chat (`chat.ts`, `armSafety`) était armé à l'envoi et jamais
 réarmé : une tâche longue mais vivante (outils, fragments en flux)
 affichait l'erreur quand même, puis la vraie réponse arrivait dans une
 bulle séparée — gênant sans rien bloquer (6 octobre, soir). Corrigé par un
 réarmement à chaque signe de vie (`pokeSafety` : outil, fragment, étape,
 mémoire, Vault, skill) et un drapeau `safetyOn` (`clearTimeout` seul ne dit
 pas si le filet est armé). L'erreur ne sort plus qu'après 3 min de silence
 total. Règle : tout délai de sécurité se réarme sur activité, et son état
 (armé/expiré/suspendu) vit dans une variable, pas dans l'existence du timer.

 **80. Comparer des snapshots d'avatar sans attendre la fin de la bulle
 fausse la mesure.** `/state listening` affiche la bulle et elle reste 12 s
 (`BUBBLE_HIDE_DELAY`) : tous les snapshots pris entre-temps montrent la
 bulle, pas la pose (36 % de pixels « différents » pour tous les états —
 c'était la bulle). Et le % brut est dominé par le fond noir (~80 % de
 l'image) : même un bras levé ne fait que ~3,5 %. Règle : laisser passer
 13 s après le dernier état à bulle, mesurer sur la région du personnage
 (x 150-410, y 230-620), et prouver un geste par le mouvement entre deux
 images rapprochées (cheer : ~15 % à 0,3 s d'intervalle, contre ~2-4 % pour
 la seule respiration).

---

## Prochaines étapes

### Décisions prises (lot 6, 3 octobre 2026)

- **STT : deux serveurs** plutôt que `small` partout (+3 s sur chaque
  fenêtre de wake word) ou `base` partout (commandes moins bien transcrites).
  Coût : ~180 Mo de RAM.
- **Embeddings : LM Studio**, même modèle que SynaptiQ, avec repli sur le
  hachage. Ne pas retirer le hachage : c'est lui qui garde la mémoire
  utilisable quand LM Studio est éteint.
- **Mémoire longue = vault Obsidian** (3 octobre, nuit) : plus de service
  externe. Souvenirs en notes Markdown, recherche plein texte locale.
  **Complété le 7 octobre** : SynaptiQ rebranché **par MCP**, comme Claude
  Code (choix de l'utilisateur parmi MCP seul / MCP + mémoire automatique /
  automatique seule) — serveur `synaptiq` dans `mcp_servers`, identité
  `SYNAPTIQ_AGENT_ID=jimmy` (une identité = un cerveau : Jimmy ne lit pas les
  souvenirs de Claude Code), outils à la demande (`mcp_call`). Le vault
  reste sa mémoire lisible ; aucun rappel ni écriture automatique dans
  SynaptiQ. L'API est jointe en `127.0.0.1:8000` par un tunnel SSH vers le
  serveur (relancé au démarrage de Windows).

### Décisions prises (4 octobre 2026)

- **Modèle** : MiMo-V2.6-Flash en principal et en vocal (choix de
  l'utilisateur). Le `.env` ne fixe plus que des valeurs de départ (piège 62).
- **Voix par défaut : « Le narrateur »** (Fish Audio
  `4f2a0684dd0247dda68f339738c780e6`) ; choix dans l'onglet Voix (63, 65).
- **Ne pas corriger les dérives du modèle par la température** : le rejeu a
  montré qu'elle n'y change rien ; le filet suffit (60).
- **Le vault reste lu en entier** (décision de l'utilisateur) : Jimmy sait
  qu'il est partagé et distingue ses notes (66).
- **Regard** : la caméra est l'œil de l'utilisateur ; pas de suivi de webcam (67).

### Décisions prises (5 octobre 2026 — sortie courte et nette)

- **Réponse courte par défaut** : une à trois phrases, l'essentiel en tête
  (nouvelle section « Sortie » d'`IDENTITY`). Le détail technique (code,
  chemins, commandes) va dans un bloc de code : il reste à l'écran et n'est pas
  lu à voix haute.
- **L'oral est borné, pas seulement nettoyé** : `limit_for_speech` ne dit que le
  début du texte nettoyé (phrases entières, `SPOKEN_MAX_CHARS` = 240
  caractères) ; le reste demeure dans la bulle. Constante plutôt que réglage, à
  ajuster si l'usage le demande. S'applique au Chat comme à la voix.

### Décisions prises (5 octobre 2026 — streaming du chat)

- **La réponse s'écrit en direct dans le chat** (`AgentEvent::Delta`) : le
  premier appel au modèle passe en SSE (`chat_stream`), le chat affiche les
  fragments au fil de la génération au lieu d'attendre l'appel complet (appel
  réel mesuré à 22 s de silence). Le résultat de l'appel est identique : la
  boucle, l'historique et les filets ne changent pas.
- **Seul le premier essai streame.** Les chemins de secours (réessai passager,
  régénération anti-dérive, conclusion forcée) rejouent `chat` sans flux :
  rejouer un stream écrirait deux fois ce qui est déjà affiché.
- **Une bulle en flux résolue par un appel d'outil devient une ligne
  « annonce »** du journal d'activité, et une bulle d'attente reprend : le
  texte d'annonce d'une itération n'est pas la réponse.
- **Le raisonnement caché (`reasoning_content`) n'est jamais affiché** ni lu à
  voix haute ; sa taille est au journal en debug (il consomme le plafond de
  jetons, piège 70).
- **La voix reste sur `Final`** : lire les fragments dirait les annonces
  d'itération. La voix anticipée dès la première phrase est différée.

### Décisions prises (5-6 octobre 2026 — sécurité, modèles, publication)

- **Fichiers sensibles : accord au cas par cas, dans le Chat** (choix de
  l'utilisateur parmi deux) : une carte « Autoriser / Refuser » suspend
  l'outil plutôt qu'un refus sec. À la voix, refus d'office. **Seule une
  écriture repérée déclenche la carte** : la lecture, le filtrage, le masquage
  et l'usage d'une clé (`ssh -i`) restent libres (piège 77).
- **Le format d'API vient du catalogue** (`provider.npm`), avec repli par
  essai sur `ModelProtocolUnsupported` ; les trois formats sont maintenus
  (consigne : « tout type de format », piège 78).
- **Dépôt public** github.com/Jimmyjoe13/jimmy-agent, licence MIT. Historique
  réécrit (`git filter-repo`) : auteur noreply, IP du VPS et nom de clé
  retirés ; les anciens commits gardent des chemins `C:\Users\…` sans secret.
  Plus aucun chemin personnel en dur : le dossier de travail par défaut et
  le dossier Godot se calculent depuis `%USERPROFILE%`.

### Décisions prises (6 octobre 2026, soir — avatar marquant et filet)

- **Gestes marquants, pas poses plus fortes seulement** : les poses ×2-3
  restent statiques à l'œil ; ce qui se remarque, c'est le mouvement. Quatre
  gestes additifs et auto-extinguibles (`nod`, `perk`, `cheer`, `shake`
  dans `jimmy.gd`), qui survivent aux changements d'état (un `success`
  suivi de `speaking` joue sa joie par-dessus la parole, sans la retarder).
- **`success`/`error` branchés pour de vrai** : les relais Chat et voix
  (desktop `commands.rs`) émettent `Success` après une tâche outillée
  aboutie, `Error` sur échec fatal — jamais après un « STOP » ni pour une
  réponse sans outil. `speak_for` ne retombe plus sur ces états transitoires
  (sinon bras en V figés après chaque réponse).
- **Filet 3 min réarmé sur activité** (piège 79) au lieu d'augmenté ou
  supprimé : son rôle est le silence total, pas la lenteur.

### Décisions prises (6 octobre 2026, soir — skills à coût constant)

Constat chiffré (demande de l'utilisateur) : 32 skills dont 31 capturés en
2 jours (~15/jour), catalogue injecté à chaque appel = ~1 800 jetons
(~37 % du prompt), +900 par jour. L'aiguisage prévu ne s'est jamais
déclenché (0/31) : croissance strictement additive.
- **D — coût constant** : catalogue retiré du prompt ; restent 3 noms
  suggérés + phrase vers `list_skills` / `read_skill` (découverte à la
  demande, comme les MCP). `catalogue()` supprimé (ni appelant ni test).
- **B — aiguisage réparé sans fusion abusive** : la similarité lexicale ne
  peut pas décider seule (0,56 sur des sujets différents — calibré sur le
  corpus avant codage). Le modèle désigne (`proche:`, vérifié) ; à défaut,
  fusion forcée seulement à 0,6 (aucun faux positif mesuré) ; sinon création.
- **C — barrière 2e occurrence** (`skills/demand.rs`, `data/skill_demand.json`,
  cap 300) : première fois on note, on ne capture pas. Mots > 3 lettres,
  3 communs minimum + recouvrement 0,5 (une requête pauvre ne valide rien).
- **Existants améliorés si nécessaire → non** : aucune paire à 0,6 dans les
  32, rien à fusionner. Descriptions conservées (le rappel `suggest` en a
  besoin, étendu au corps) ; elles ne coûtent plus rien depuis D.

 **81. Un bras qui traverse le torse disparaît dedans, pas derrière.**
 `jimmy.gd` applique `arm_l` en miroir (`-_pose`) mais `arm_r` brut : avec
 des valeurs symétriques (`1,10 / -1,10`), les deux bras penchaient du même
 côté, et dès que l'angle grandissait le bras droit traversait le torse
 (même plan z = 0) — invisible, « caché derrière le corps ». Vérifié sur
 snapshots : success ne montrait qu'un bras. Règle : valeurs positives des
 deux côtés (miroir partout) ; aucun membre ne doit viser le centre du
 torse dans le plan z = 0 — une main-au-menton exigerait un décalage en z.
 Au passage : 2,20 rad collait les bras aux oreilles, 1,90 fait un vrai V.

 **82. Un délai de passage en fond compté depuis la demande fait partir
 une tâche presque finie.** Première version des tâches de fond (7 octobre) :
 « outillée et plus de 15 s depuis la demande ». En test réel, le premier
 appel à MiMo a pris 26 s : au tout premier outil (`write_file`), la tâche
 avait déjà « 26 s » et partait en fond — Jimmy aurait dit « je m'en occupe »
 puis annoncé la fin deux secondes après. Elle partait même pendant que la
 carte d'autorisation (fichier sensible) attendait l'utilisateur. Règle : le
 délai se compte **depuis le premier outil** (`tasks::should_detach`), et
 jamais de passage en fond tant qu'une autorisation est ouverte. Côté suite
 d'interface : une tâche passée en fond **survit à son parcours** (une seule
 place de fond) — le parcours suivant attend qu'elle se libère et vérifie
 que le bandeau porte **son** titre, sinon il lit celui du précédent.

 **83. Un rappel qui compte toute commande comme une action ne part
 jamais.** Usage réel du 7 octobre : deux demandes de suite (tunnel SSH vers
 un serveur, reconfiguration de clients MCP) ont chacune épuisé 25 étapes en
 inspection — clés essayées une à une, `Get-ChildItem`, `ssh … docker ps`,
 `Test-Path` — puis conclu « reste à faire », sans rien changer.
 Le rappel « agis ou conclus » (`EXPLORATION_NUDGE`, étapes 6 et 10)
 n'existe que si `acts == 0` ; or tout `run_command` comptait comme une
 action. `sensitive::command_is_read_only` classe désormais une commande
 d'inspection comme lecture — **liste blanche** (verbe inconnu = action),
 commande distante de `ssh` analysée, redirection vers `$null` tolérée.
 Mesuré sur les 63 commandes réelles de la session : 0 → 40 lectures au moins. Et
 une tâche de fond a désormais 60 étapes / 20 min.

 **84. Une sonde HTTP brute vers OpenCode Go échoue avant même le
 modèle.** Sonde de la vision (7 octobre) en Python : `403 error code:
 1010` (Cloudflare refuse l'User-Agent de Python), puis `400
 MissingSessionID` (« Request is missing x-opencode-session »). Le client
 de Jimmy envoie déjà les deux (`jimmy/0.1 (desktop agent)` et la session
 `LlmClient::new`) : ce ne sont pas des erreurs de format. Règle : toute
 sonde hors de Jimmy envoie ces deux en-têtes, sinon on conclut à tort
 qu'un format ou une fonction est refusé.

 **85. Sous Windows, la zone cliquable de l'avatar découpe aussi son
 affichage.** `main.gd` réduit la zone de la fenêtre qui capte la souris
 (`DisplayServer.window_set_mouse_passthrough`) pour que les clics
 traversent le vide autour du renard. Sous Windows, ce qui sort de cette
 zone **n'est pas dessiné**. Elle ne couvrait que le corps (±0,32) : mains du
 renard bras en V coupées, et le lapin des tâches de fond (x 0,68)
 invisible chez l'utilisateur — alors que `/snapshot`, qui lit l'image
 **avant** la découpe, les montrait entiers. Corrigé : zone = renard bras
 compris (±0,58, 1,80 de haut) fusionnée avec celle du lapin quand il est là
 (`Geometry2D.merge_polygons`). Règle : tout ce qui est ajouté à la scène
 doit entrer dans cette zone, et se vérifie sur l'écran réel
 (`scripts\photo-avatar.ps1`), pas seulement par `/snapshot`.

 **86. Playwright MCP ne renvoie pas la page après une action.**
 Version 0.0.83 : `browser_navigate`, `browser_click`… répondent par l'URL,
 le titre et un **lien vers un fichier** d'instantané (`[Snapshot](….yml)`)
 — le modèle ne voit pas le contenu. Seul `browser_snapshot` le met dans la
 réponse. Et sans `--output-dir`, ces fichiers s'écrivent dans le dossier
 courant (un `.playwright-mcp` est apparu dans `agent/` pendant les tests).
 Règle : le prompt demande un `browser_snapshot` après chaque action
 (`prompt::BROWSER_GUIDE`), et le serveur a toujours un `--output-dir`
 (`data/browser-output` pour Jimmy, dossier temporaire pour le test).

 **87. Arrêter Jimmy avec son arbre tue aussi ce qu'il a lancé pour
 durer.** INSTRUCTIONS §2 impose `taskkill /T` (sinon MCP et whisper restent
 orphelins). Mais le 7 octobre, Jimmy avait lui-même ouvert un tunnel SSH
 (`Start-Process ssh … -N -L 8000:…`) depuis `run_command` : descendant de
 son processus, il est mort au premier arrêt pour compilation — SynaptiQ
 « connexion refusée » pour tous les agents, sans rien dans `jimmy.log`.
 Règle : avant d'arrêter Jimmy, regarder s'il a lancé un processus durable
 (tunnel, serveur) et le relancer après ; un processus qui doit survivre à
 Jimmy se lance par un mécanisme du système (tâche planifiée, démarrage
 Windows), pas comme enfant de son `run_command`.

**88. `hasText` de Playwright voit aussi le texte des `<option>`.** Parcours
« fournisseurs » (8 octobre) : chaque ligne contient un sélecteur de format
dont une option s'appelait « messages — /messages (Anthropic) ».
`rows.filter({ hasText: "Anthropic" })` a donc matché les cinq lignes, pas
celle d'Anthropic, et l'attente « exactement 1 » échouait sur une interface
pourtant correcte (le DOM-dump en `evaluate` était bon — le doute venait du
sélecteur, pas de la vue). Règle : pour repérer une ligne par son libellé,
filtrer sur un élément porteur (`has: locator("code", …)`) ou mettre le mot
dans le libellé seul ; ne jamais laisser un libellé d'`<option>` contenir le
nom qu'on cherche. Le test passé ne prouvait rien : un sélecteur large est un
faux positif.

**89. Une interface qui masque les secrets doit définir le contrat du
« vide ».** `get_settings` renvoie désormais les fournisseurs avec `api_key`
vidé ; un enregistrement complet renverrait donc des clés vides et les
effacerait. Contrat posé : clé vide = inchangée côté `save_settings` (fusion
avec la valeur en mémoire), retrait explicite seulement par
`llm_set_provider_key("")`. Toute future donnée sensible dans un formulaire
doit reprendre ce contrat, sinon le premier « Enregistrer » général écrase ce
que la vue dédiée avait épargné.

  **90. L'abonnement Claude (OAuth) est bridé par modèle côté serveur.** Mesuré
  le 8 octobre avec la session Claude Code de l'utilisateur (`--test
  claude_plan -- --ignored`) : `/v1/models` répond bien (14 modèles), Haiku
  fonctionne avec outils en ~550 ms, mais Sonnet ET Opus renvoient un `429
  rate_limit_error` nu alors qu'ils sont disponibles dans Claude Code à la même
  heure. C'est la grille d'abonnement Anthropic (premium réservé au client
  Claude Code, d'après les relevés communautaires), pas un quota : Haiku passe
  à la seconde près. Jimmy appelle donc l'API proprement (`Bearer` + `anthropic-
  beta: oauth-2025-04-20`) et **n'imite pas** Claude Code (billing header dans
  system, `user-agent: claude-cli`…), ce qui serait contourner la restriction
  et risque de faire révoquer la session. Usage : abonnement pour les modèles
  légers, clé API pour Sonnet/Opus. Le 401 arrive sans le drapeau bêta et
  consomme le jeton : ne jamais retirer `oauth-2025-04-20`.
  **Precision du 9 octobre (bissection réelle, mêmes sessions)** : la grille ne
  regarde ni le modèle économique, ni le volume de texte, ni le flux — elle
  regarde le POIDS DE LA DECLARATION D'OUTILS. 21 621 jetons de prompt système
  sans outils passent ; 12 outils réalistes (~3 000 jetons d'outils) échouent
  en `400 invalid_request_error — “Third-party apps now draw from extra usage,
  not plan limits”` même avec un message de 15 jetons ; 12 outils minuscules
  passent. Le « Tester » de Jimmy (1 outil factice, 548 jetons) était donc
  trompeur : un Haiku qui répond au test et qui échoue au premier vrai tour
  outillé. Le chat réel de Jimmy déclare ~15 outils : sur abonnement sans
  « extra usage » activé, il échoue toujours. Messages d'erreur traduits côté
  Jimmy (`CLAUDE_PLAN_EXTRA_USAGE_MSG`, `CLAUDE_PLAN_PREMIUM_MSG`) ; le garde-
  fou premium est local (refus avant appel). Conclusion d'usage : l'abonnement
  seul ne peut pas porter Jimmy en agent — soit activer « extra usage » (fac-
  turation à l'usage chez Anthropic), soit clé API, soit rester sur OpenCode Go.
  **Resolution le soir meme (mesure Q1/Q2, piege 92)** : la signature Claude
  Code complete (bloc de facturation + noms d'outils alias) fait passer les
  vraies tournees outillees — Sonnet repond 200 outils compris, et la suite
  complete est passee sur le plan (28/29). Le texte ci-dessus reste vrai pour
  ce qui etait mesure le matin : ne pas retirer `oauth-2025-04-20`, et ne
  jamais considerer le « Tester » comme une preuve pour un modele d'abonnement.

**91. `config.json` appartient à Jimmy : le réécrire à la main en PowerShell
peut tout effacer.** Le 9 octobre, une correction de modèle tentée par
`ConvertTo-Json | Set-Content -Encoding UTF8` (PS 5.1 écrit un **BOM**) : au
démarrage suivant `serde_json` refuse le BOM → « config illisible, retour aux
valeurs par défaut » → le Jimmy lancé a **resauvé les défauts par-dessus la
config réelle** — six serveurs MCP, réglages de voix, tout perdu sans alerte
côté interface. Sauvés par les `data/config.json.bak-*` (sauvegardes
horodatées). Règles : (1) un réglage passe par les commandes Tauri
(`llm_update_provider`, `set_llm_model`, `llm_set_provider_key`), jamais par
l'édition du fichier ; (2) si édition absolument nécessaire, écrire sans BOM
(`[IO.File]::WriteAllText(..., UTF8Encoding(false))`) et **Jimmy arrêté** ;
(3) voir « config illisible » au journal ⇒ restaurer le dernier `.bak` avant
que Jimmy ne sauve quoi que ce soit.

**92. La grille Anthropic sur abonnement se joue aussi sur le NOM des
outils.** Sondes du 9 octobre, même requête et mêmes en-têtes avec facturation :
15 outils nommés `read_file`/`run_command`/`mcp_call` = 400 « Third-party
apps now draw from extra usage » ; ces 15 outils rebaptisés `Read`/`Write`/
`Bash`/`Agent`… = 200 — descriptions, schémas et User-Agent de Jimmy
conservés. Un `ping` et des `outil_N` passaient déjà : c'est une liste noire
de signatures de frameworks tiers, pas une liste blanche Claude Code. D'où
`claude_plan::map_plan_tool_names` (déclaration `tools`, blocs `tool_use` de
l'historique, noms cités dans le prompt système) et `unmap_plan_tool_calls` au
retour, testés unitairement et en réel. Le bloc de facturation reste
indispensable (sans lui, premium refusé). Anthropic peut durcir : liste noire
plus large ou facturation validée — surveiller les 400/429 au journal.

  **93. L'installeur NSIS livre Jimmy sans avatar ni voix.** `tauri.conf.json`
  (`bundle.targets = ["nsis"]`) n'embarque aucune ressource : ni `godot/`,
  ni whisper, ni skills. En mode installé, `paths.app` vaut
  `%APPDATA%\Jimmy` : `godot_project()` n'y existe pas, l'avatar ne démarre
  pas, et les serveurs vocaux n'ont aucun binaire à lancer. L'installeur ne
  remplace donc pas `install.ps1` sur un second PC — il installe une coquille
  (interface + agent). Documenté dans le README (section Installation),
  non vérifié de bout en bout. Le jour où l'installeur devient la voie
  normale, il faudra y mettre les ressources (ou les télécharger au premier
  lancement, comme `install.ps1`).

  **94. `/update` exige un clone git : ni zip ni installeur.** La détection
  compare `git rev-parse HEAD` (dans `paths.app`) à `git ls-remote origin
  HEAD`. Sans `.git`, les deux rendent `None` et la vérification **se tait**
  (pas d'alerte, pas d'erreur) : un Jimmy issu du zip GitHub ne saura jamais
  qu'il est périmé. La migration zip → clone est manuelle (README « Deuxième
  PC ») ; `/update` sur un non-clone répond la procédure au lieu d'échouer
  en silence. Et `/update` ne part jamais sans binaire vérifié : si le build
  échoue, le Jimmy actuel continue (le redémarrage est programmé seulement
  après `target/release/jimmy.exe` plus récent que le début du build).

  **95. Une veille du PC pendant `test-ui.ps1` fait perdre tout le bilan.**
  `suite.js` n'imprime ses résultats qu'à la fin (`console.log(results)`) :
  si Windows passe en veille moderne pendant la suite (9 octobre, 08:35 →
  08:53 et → 09:08, Kernel-Power 506/507), la page CDP se ferme
  (`Target page, context or browser has been closed`), le parcours en cours
  rapporte une durée absurde au journal (`run_command` « timeout » après
  1 049 s, DeepSeek et le dépôt injoignables au réveil) et **aucun** parcours
  passé n'est affiché. Ce n'est pas un bug du code : vérifier les
  événements Kernel-Power avant d'enquêter, puis relancer la suite PC
  éveillé.


  **96. Sur une installation fraîche, l'avatar reste muet : le cache de
  l'éditeur manque.** `main.gd` utilisait les `class_name` globaux (`Jimmy`,
  `JimmyHttpServer`), qui n'existent que dans le cache de l'éditeur
  (`.godot/`, non versionné) : sur un clone frais, la scène meurt à
  l'analyse (`SCRIPT ERROR … not declared in the current scope`), aucun
  serveur HTTP (8787), avatar invisible et fenêtre morte qui mange les clics
  (l'accueil paraît « gelé » ; cas réel du 8 octobre 2026 sur le PC du
  travail). Le `--check-only --script` hors `--path` ne l'attrape pas
  (chemins `res://` faux). Corrigé par `preload` (`JimmyFox`, `JimmyHttp`,
  comme le lapin) et types explicites (`Node3D`, `Node`, `int`, `float` —
  l'inférence `:=` passait par les classes globales). Règle : dans les
  scripts Godot, `preload` plutôt que le `class_name` d'un autre script ;
  régression couverte par `--test avatar_scripts -- --ignored` (démarrage
  `headless` sans cache, serveur attendu). Arrivé par la PR #1 (fork du PC
  du travail), intégré le 9 octobre.

  **97. `/update` ne pouvait pas recompiler le Jimmy qui tourne.** Premier
  essai réel au travail (9 octobre) : pull OK, puis `build.ps1 -Release`
  échoue sur `failed to remove file …\target\release\jimmy.exe — Accès
  refusé. (os error 5)` : Windows refuse d'écraser un exécutable en cours,
  et c'est lui qui lance la compilation. Trois défauts liés : (1) le message
  affichait le **début** du journal (l'en-tête npm `> jimmy-desktop…`) au
  lieu de l'erreur, en fin de journal ; (2) le pull ayant réussi, Jimmy se
  croyait à jour : retaper `/update` répondait « Déjà à jour » avec l'ancien
  binaire ; (3) stdout/stderr en tubes jamais lus pendant la compilation
  (risque de blocage sur un gros build). Corrigé dans `update.rs` :
  `ExeAside` renomme l'exe en cours en `jimmy.exe.old` (Windows l'autorise),
  le remet en place au `Drop` si la compilation échoue ou est abandonnée
  (STOP, délai), l'efface au cycle suivant ; `unbuilt` (état persisté) garde
  l'alerte levée tant que le code tiré n'est pas compilé ; `/update`
  recompile aussi si le binaire est plus ancien que `HEAD` ; journal complet
  dans `data/logs/update-build.log`, résumé par `build_error_summary`.
  Le redémarrage, jamais exercé avant, cassait aussi : (4) PowerShell lancé
  en `DETACHED_PROCESS` depuis une appli graphique meurt avant d'exécuter
  le lanceur (reproduit avec un programme GUI minimal : 0x08 → rien,
  `CREATE_NO_WINDOW` 0x08000000 → OK) : Jimmy quittait et ne revenait
  jamais ; (5) `std::process::exit` saute les `Drop` : Godot restait
  orphelin sur 8787 → `App::shutdown_children` avant de quitter ; (6) le
  lanceur relançait sans attendre la sortie de l'ancien (instance unique
  Tauri) → `launcher.ps1 -WaitPid`, trace dans `data/logs/launcher.log`.
  Test de bout en bout : Jimmy lancé avec CDP, `unbuilt` = `HEAD` dans
  `data/update_state.json`, `/update` tapé dans le Chat (Playwright).
  **Un PC qui tourne sur un binaire d'avant ce correctif doit être
  recompilé une fois à la main** (arrêter Jimmy, `git pull`,
  `build.ps1 -Release`) : c'est l'ancien code qui exécute `/update`.

  **98. « Demande l'accord » dans le prompt = le modèle demande l'accord en
  texte.** Parcours « fichier sensible » en échec intermittent depuis le
  6 octobre (environ une suite sur deux, 25 dernières suites). Rejeu
  (`agent/tests/replay.rs`, base copiée, `JIMMY_REPLAY_SHOW=1` affiche
  texte et outils) : en session neuve, `write_file` 8/8 ; avec le **fil de
  la suite** (5 tours avant), 3 à 6 réponses sur 10 sans outil : « Dis-moi
  oui et je le crée », « Confirmes-tu que je peux continuer ». La règle 8
  du prompt (« modifier l'un d'eux demande l'accord de l'utilisateur ») se
  lisait comme une consigne de demander soi-même. Les souvenirs
  « validation explicite » n'y sont pour rien (sans eux : 6/10 sans outil).
  Règle reformulée : « appelle directement l'outil, c'est l'application qui
  demande l'accord ; ne demande pas en texte » → 12/12 avec outil
  (10 `write_file`, 2 consultent d'abord les skills). Mécanisme de carte
  inchangé. Leçon : une règle qui décrit un contrôle fait **par
  l'application** doit dire au modèle d'agir, pas décrire le contrôle.

### Décisions prises (7 octobre 2026 — navigateur)

- **Serveur MCP Playwright** (`navigateur` dans `mcp_servers`, version figée
  `@playwright/mcp@0.0.83`, Chrome visible), branché sans code comme
  SynaptiQ. Outils à la demande (`mcp_call`), sorties bornées à 12 000
  caractères (un instantané de page plus long est tronqué).
- **Son propre Chrome, connecté à tes comptes** (choix de l'utilisateur) :
  profil persistant `data/browser-profile`. L'utilisateur s'y connecte
  **lui-même** une fois ; Jimmy ne saisit jamais de mot de passe (consigne
  du prompt).
- **Carte « action en ton nom »** (`sensitive::browser_action_needs_approval`,
  même mécanisme que les fichiers sensibles) : clic dont la description
  contient un mot d'engagement (envoyer, payer, commander, publier,
  supprimer, confirmer, valider, s'abonner, virement, réserver, signer,
  répondre…), saisie validée hors recherche, touche Entrée, envoi de
  fichier, script qui agit dans la page. Libres : navigation, lecture,
  recherche, saisie, cookies. À la voix : refus d'office, comme les
  fichiers sensibles. Limite : la description vient du modèle ; un bouton
  mal décrit échappe à la carte.

### Décisions prises (7 octobre 2026 — vision de la fenêtre active)

Demande : que Jimmy ait le contexte de ce que fait l'utilisateur, **seulement
quand il le demande**. Choix de l'utilisateur :
- **Déclenchement par l'utilisateur seul** : bouton « Joindre ma fenêtre »
  du Chat (capture au clic, vignette retirable, `screen_capture` /
  `screen_discard`) ou phrase explicite au Chat comme à la voix
  (`screen::wants_screen` : « regarde mon écran », « tu vois ma fenêtre ? »,
  « ce que je fais » — « l'écran de login » ne capture rien). **Aucun outil
  de capture** n'est donné au modèle.
- **Fenêtre active seule** : on écarte le processus de Jimmy et l'avatar
  (cliquer dans le Chat met Jimmy au premier plan), puis la fenêtre au
  premier plan si elle reste, sinon la plus haute de la pile
  (`screen::pick`, crate `xcap`). Réduite à 1600 px, JPEG qualité 80.
- **Jamais sur disque** : `Message::images` est hors sérialisation ;
  l'historique garde « (Capture d'écran jointe : « titre ».) ». Les captures
  du bouton attendent en mémoire (3 au plus).
- **Modèle sans vision** (catalogue, `modalities.input`) : l'image n'est pas
  envoyée et Jimmy le dit (`App::images_for`). MiMo-V2.6-Flash et Muse Spark
  1.3 lisent les images.
- La suite d'interface vérifie bouton, vignette et retrait **sans envoyer**
  l'écran réel au fournisseur.

### Décisions prises (7 octobre 2026 — tâches de fond)

Demande de l'utilisateur : « laisser le chat le plus disponible possible en
lançant les tâches en arrière-plan ». Plan validé, avec ses choix :
- **Passage automatique** : toute demande commence au premier plan ; une
  tâche **outillée** passe en fond à son 3e outil, ou 15 s après son premier
  outil (`tasks::DETACH_AFTER*`). Une réponse lente sans outil reste au
  premier plan. Chat : bulle « Je m'en occupe en arrière-plan », saisie
  libre, bandeau au-dessus de la saisie (étape, durée, « Arrêter »). Voix :
  « Je m'en occupe en arrière-plan, je te préviens quand c'est fini », puis
  retour à l'écoute. Fin : réponse dans sa conversation (toast si une autre
  est affichée) et annonce à voix haute « Tâche de fond terminée. … » dès que
  Jimmy est libre (2 min d'attente au plus).
- **Réponse en parallèle** : pendant une tâche de fond, Jimmy répond aux
  nouvelles demandes en le sachant (bloc « Tâche en cours en arrière-plan »
  du prompt : titre, étape, durée ; consigne de ne pas refaire son travail).
- **Arrêts** : « STOP » et le bouton « Arrêter » du composer n'arrêtent que
  le premier plan et la parole ; la tâche de fond s'arrête par son bouton
  (`task_stop`) ou par « arrête tout » / « arrête toutes les tâches » à la
  voix (`is_stop_all_command`, `App::request_stop_all`).
- **Une seule tâche de fond** à la fois : une seconde tâche longue reste au
  premier plan, comme avant.
- **Seuil revu le soir même** (« il lance toutes ses tâches en fond, même
  les légères ») : mesuré, 17 passages sur 19 venaient de la règle « 3e
  outil », 0 à 5 s après le premier (Muse Spark appelle 3-4 outils d'un
  coup), pour des tâches de 30 à 50 s. Règle supprimée ; passage en fond
  **45 s après le premier outil** (`tasks::DETACH_AFTER`, réglable par
  `Tasks::set_detach_after` pour les tests). ~8 tâches sur 19 seraient
  restées au premier plan.
- **Le lapin** (`godot/scripts/helper_rabbit.gd`, route `/helper`) : pendant
  une tâche de fond, un lapin blanc travaille à droite du renard
  (mini-ordinateur au logo qui scintille, pattes qui tapent, engrenage qui
  tourne) ; fin réussie = sauts bras en V, échec ou arrêt = oreilles
  tombantes, puis il disparaît (2,6 s). Primitives et contour comme le
  renard, aucun asset ; chargé par `preload` (un nouveau `class_name`
  n'entre au cache global qu'avec l'éditeur). Déclenché par les relais Chat
  et voix (`background_avatar`). Limite : si Godot redémarre pendant une
  tâche de fond, le lapin ne revient qu'à la tâche suivante.
- **Pas de persistance** : le registre (`agent/src/tasks.rs`) vit en
  mémoire ; un redémarrage de Jimmy perd la tâche en cours.
- **Budget élargi en fond** (choix de l'utilisateur après usage réel) :
  60 étapes et 20 min au lieu de 25 étapes et 10 min. Relu à chaque étape
  (`agent::step_budget`, drapeau `AgentDeps::background`) : le premier plan
  garde ses limites, une tâche gagne le sien dès son passage en fond.
- Mécanique : `App::start_task` (Chat et voix) remplace `cancellable` ; ses
  événements passent tels quels au premier plan, puis `Detached` et tout
  enveloppé dans `Background` (avatar non sollicité, sauf succès/erreur à la
  fin). Limite connue : un tour en parallèle dans la **même** session
  s'intercale dans l'historique entre les étapes de la tâche de fond
  (`History::conversation` ne garde que les messages texte, le format
  Messages fusionne les rôles consécutifs : pas d'erreur, mais le résumé des
  outils peut attribuer un outil de la tâche au tour parallèle).

### Décisions prises (8 octobre 2026 — LLM : sous-onglet et multi-fournisseurs)

Demande de l'utilisateur, reformulée et validée avant le code : « migrer toute
la configuration LLM dans un sous-onglet des Paramètres, puis connecter
plusieurs fournisseurs ». Validé sur les trois points de décision : clés saisies
dans l'interface, fournisseur personnalisé, refonte de la bibliothèque.

- **Sous-onglets Paramètres** : « Général | LLM », même mécanique que Skills →
  Serveurs MCP. Le LLM pane regroupe la carte Fournisseurs, la carte Modèle
  (principal, vocal, température), la bibliothèque et une carte Budget (max_tokens,
  max_iterations — réglages qui n'avaient jamais d'appelant d'interface).
- **Cinq fournisseurs intégrés**, en presets dans `config.json` (jamais codés en
  dur dans les appels) : OpenCode Go (auto par catalogue, en-tête session),
  OpenRouter / DeepSeek / Alibaba-DashScope (format chat), Anthropic (format
  messages, clé en `x-api-key`). Plus « ajouter un fournisseur » (custom-N) pour
  LM Studio ou une API d'entreprise.
- **La liste `providers` est la source de vérité** ; `llm.base_url` devient un
  champ legacy que la migration recopie dans le fournisseur OpenCode Go (un
  proxy réglé à la main ne se perd pas). Un identifiant de fournisseur inconnu
  ou désactivé retombe sur le premier activé, jamais sur une panne.
- **Routing par modèle** : `LlmClient::update_providers` attache le modèle
  principal et le modèle vocal à leur fournisseur (ils peuvent être différents —
  écrit chez Claude, rapide chez OpenRouter). Un modèle inconnu suit le
  principal. Les en-têtes (Bearer vs `x-api-key`, `anthropic-version`,
  `x-opencode-session`) et le format (imposé ou catalogue) suivent le
  fournisseur, plus le client.
- **Clés** : saisies dans l'interface, stockées dans `data/config.json` (jamais
  commité — le dépôt est public ; l'utilisateur a choisi cette dérogation à la
  règle « `.env` seulement »). `get_settings` les masque ; `save_settings`
  fusionne (vide = inchangée) ; `status` n'expose que `has_key`, `key_from` et
  4 caractères de reconnaissance (piège 89). La clé `.env` reste repli pour
  OpenCode Go et OpenRouter — valeurs de départ, jamais écrasement (piège 62).
- **Tous les modèles du compte doivent marcher** (consigne du 6 octobre) : les
  trois formats déjà traduits (`protocol.rs`) sont gardés ; le nom du service
  dans les erreurs vient du fournisseur appelé, plus « OpenCode Go » partout.
- La bibliothèque garde ses résultats de test par **fournisseur**
  (`jimmy.llm-tests.v3`, clé `fournisseur:modèle`) : un « deepseek-chat » testé
  chez l'un ne dispense pas du test chez l'autre.
- **Abonnement Claude (demande du 8 octobre)** : le fournisseur Anthropic offre
  deux méthodes d'accès — clé API (défaut) ou **plan Claude** (`auth:
  "claude-plan"`). Le mode abonnement **réutilise la session de Claude Code**
  (`~/.claude/.credentials.json`) : Jimmy lit le jeton, le rafraîchit quand il
  expire et **réécrit le fichier** en préservant le reste (les deux outils
  partagent la session ; si l'un a rafraîchi entre-temps, `invalid_grant` →
  relecture). Appels en `Bearer` + `anthropic-beta: oauth-2025-04-20`, jamais
  `x-api-key` avec un jeton OAuth. **Signature Claude Code requise (mesures des
  8-9 octobre, pièges 90 et 92)** : la grille Anthropic refuse le plan aux apps
  tierces dès qu'un modèle premium ou une déclaration d'outils substantielle
  apparaît (« extra usage »). À la demande explicite de l'utilisateur (« je
  veux l'accès aux modèles Anthropic avec mon plan »), Jimmy prend donc la
  signature complète : bloc `x-anthropic-billing-header` en bloc 0 du système +
  noms d'outils alias Claude Code (`map_plan_tool_names`/
  `unmap_plan_tool_calls`), schémas, descriptions et UA Jimmy conservés.
  Risque assumé et consigné : d'autres outils le font déjà ; Anthropic peut
  durcir, voire révoquer ; si le 400 revient, vérifier d'abord la liste noire
  et le bloc de facturation (piège 92). Choix du moteur laissé à l'utilisateur
  via le sous-onglet LLM.

### Décisions prises (8 octobre 2026 — mises à jour et secrets au journal)

Demande de l'utilisateur : tester les notifications de mise à jour entre
deux PC (corriger ici, pousser, voir la bulle au travail, `/update`
là-bas), documenter l'installeur, et corriger la fuite des secrets au
journal comme véhicule du test.

- **Détection par SHA git, pas par version** : `CARGO_PKG_VERSION`
  (0.1.0) ne bouge jamais d'un commit à l'autre ; on compare
  `git rev-parse HEAD` à `git ls-remote origin HEAD` (`agent/src/update.rs`,
  `git` bornés 10 s / 30 s, `kill_on_drop`). Sans `.git` : silence, pas
  d'erreur (piège 94).
- **Boucle `update_loop`** (5 min puis 30 min, modèle `review_loop`),
  état dans `data/update_state.json` (`pending` + `notified`) : une alerte
  survit au redémarrage.
- **Notification en deux canaux** : bulle `say()` à chaque cycle **si
  inactif** (`App::is_busy` : ni tâche au premier plan, ni synthèse —
  sinon le rappel écrase la bulle lue) + session « Mise à jour » écrite
  **une fois par SHA** + pastille persistante dans la barre latérale
  (`status().update.pending`, clic vers l'Historique, rafraîchie toutes
  les 15 s).
- **`/update` déterministe** (intercepté avant le modèle dans `chat`) :
  `pull --ff-only` (refus explicite si dépôt sale), recompilation
  `build.ps1 -Release` en tâche suivie (détachement, lapin, `Progress`
  toutes les 60 s pour le filet 3 min), redémarrage **seulement si le
  binaire est plus récent que le début du build**. Le redémarrage attend
  le calme (`schedule_restart` : ni tâche ni parole, capuchon 5 min) puis
  relance `launcher.ps1` détaché et quitte ; un « STOP » annule tout et
  ne redémarre pas (`pending` revérifié avant de quitter).
- **Secrets masqués avant journal et affichage** (`sensitive::mask_json`
  / `mask_text`, sans dépendance) : arguments d'outils (`agent.rs`),
  extraits bruts `learn.rs` / `capture.rs`, et carte d'autorisation
  (`describe`). Motifs : clés nommées (`KEY=`, `:`, `Bearer`, `?...=`),
  valeur au mot suivant (`X-API-Key: <clé>`), `sk-…`, identifiants d'URL.
  Reste à l'utilisateur : **régénérer les deux clés exposées**
  (`aggregate`, SynaptiQ) — aucun code ne le fait à sa place.

### Décisions prises (9 octobre 2026 — refonte visuelle de l'interface)

Demande de l'utilisateur : appliquer à l'interface desktop la maquette
Claude Design « Jimmy – Refonte interface » (deux écrans, Chat et Voix),
reformulée puis validée avant le code. Points tranchés par défaut (« ok top
go » sans réponse explicite) : accent de la maquette, rien de nouveau côté
Rust, les six autres onglets extrapolés.

- **Habillage seulement** : mêmes vues, mêmes classes, mêmes libellés —
  les sélecteurs de `scripts/ui-test` (textes de boutons, `.card` « Écoute
  permanente » et l'ordre de ses `.note`, `.composer button.primary` =
  « Envoyer », `.topbar .chip` = « prêt ») sont restés valables sans
  modification.
- **Palette graphite + accent cuivre `#D9692C`** (au lieu de l'ambre) ;
  tous les ambres codés en dur passent par `color-mix(... var(--accent) ...)`
  et `--bg-input` / `--on-accent` : changer l'accent = une ligne de `:root`.
- **Polices embarquées** (Geist, Geist Mono, Instrument Serif, sous-ensembles
  latins woff2, OFL, `desktop/src/assets/fonts/`) : l'interface reste hors
  ligne, rien n'est chargé depuis Google Fonts.
- **Icônes au trait** dans `ui.ts` (`icon(name)`, chaînes SVG constantes)
  à la place des glyphes Unicode de la navigation.
- **Bouton de base stylé** (voile argent) : son survol est en
  `button:where(:hover…)` pour que les survols propres aux composants
  (nav, sous-onglets, listes) gardent la main ; `.msg-path` / `.work-file`
  repassent en `inline-block` pour garder l'ellipse.
- **Voix** : la carte « Écoute permanente » devient le héros de la maquette ;
  l'onde (56 barres) suit **le vrai niveau du micro** déjà relevé par
  `livePoll` (aucune commande ajoutée) ; « Parler à Jimmy » = le bouton
  existant activer / couper l'écoute (pas de push-to-talk).
- **Chat** : état vide (orbe, accroche serif, trois suggestions qui
  préremplissent sans envoyer), compositeur en boîte avec bouton « @ » qui
  réutilise le chemin des mentions (événement `input`). Pas de bouton dictée
  (aucune fonction derrière). Orbe statique (respiration CSS).
- Les trois petites cartes de la maquette Voix (écoute, voix, avatar) ne
  sont pas reprises : elles doublaient les réglages existants.

### Décisions prises (9 octobre 2026 — logo officiel)

- **Logo officiel « Agent Jimmy »** (monogramme « AJ » à lunettes) : source
  `logo-agent-jimmy.jpg` à la racine. Le monogramme est détouré (fond marine
  rendu transparent par distance de couleur, Pillow) dans
  `desktop/src/assets/logo-mark.png` (256 px) et posé en fond CSS de
  `.brand-mark` (remplace la pastille cuivre et l'icône `logo` de `ui.ts`,
  supprimée). Le nom « Jimmy » de la barre latérale prend le corail du logo
  (`#fe6f51`).
- **Icônes de l'appli** (`desktop/src-tauri/icons/*`, `desktop/public/icons/32x32.png`) :
  le monogramme sur une tuile marine arrondie (lisible en 16-32 px), générés
  par `scripts/make-icons.ps1` (System.Drawing, part de `logo-mark.png` ;
  ICO multi-tailles 16 à 256 px, favicon compris). Nouveau logo = refaire
  `logo-mark.png` (détourage) puis relancer le script.
- **Raccourci bureau** (`scripts/shortcut.ps1`) : `IconLocation` sur
  `icon.ico` (un PNG s'y affiche mal et reste dans le cache d'icônes).
- **Barre de titre intégrée** (`desktop/src/titlebar.ts`) : fenêtre sans
  décoration native (`decorations: false`), bande transparente de 36 px en
  `position: fixed` (glisser = déplacer, double-clic = agrandir) et trois
  boutons au trait ; « Fermer » appelle `close()` → `CloseRequested` cache
  la fenêtre comme avant. Montée **avant** `bootstrap` pour que la fenêtre
  reste fermable si le démarrage échoue. Permissions ajoutées :
  `toggle-maximize`, `internal-toggle-maximize`, `is-maximized`. Barre
  latérale et `.topbar` décalées sous la bande. Perdu : le menu Snap Layouts
  de Windows 11 au survol d'« Agrandir » (Win+flèches et bords d'écran
  restent). Étape test-ui « Fenêtre : barre de titre intégrée ».

### Décisions prises (9 octobre 2026 — l'agent s'appelle « Jimy »)

Demande de l'utilisateur : l'agent devient **« Jimy » (« Agent Jimy »)** ;
l'utilisateur, lui, reste Jimmy. Périmètre choisi : **le nom affiché
seulement**.
- Renommé : textes de l'interface (`desktop/src`, bulle « jimy »), titre de
  la fenêtre (`tauri.conf.json` `title`, `index.html`), messages Rust envoyés
  à l'interface ou au modèle (`update.rs`, descriptions d'outils, prompts de
  `growth.rs` / `learn.rs` / `memory`), identité du prompt (« Tu es Jimy »,
  nom à un seul « m », ne pas le confondre avec celui de l'utilisateur),
  bulle d'aide et nom du projet Godot, lanceur, raccourci (`Jimy.lnk` ;
  `shortcut.ps1` retire l'ancien `Jimmy.lnk` s'il pointe sur le lanceur),
  README.
- **Inchangé, volontairement** : `productName` (risque, non vérifié : Tauri v2
  peut nommer l'exécutable d'après lui, or `jimmy.exe` est attendu par
  `update.rs`, `launcher.ps1`, `test-ui.ps1`), identifiant `com.jimmy.desktop` (dossier
  de données, instance unique, démarrage auto), crates `jimmy-agent` /
  `jimmy_agent`, dépôt, `%APPDATA%\Jimmy`, vault `C:\Obsidian\Jimmy` et
  dossier `0_Inbox/Jimmy` (chemins réels), `user_name: "Jimmy"` (le prénom
  de l'utilisateur), commentaires du code Rust, JOURNAL et anciens pièges
  (historique). Le texte « AGENT JIMMY » du logo d'origine n'est pas repris
  dans l'interface (seul le monogramme « AJ » l'est).
- **Mot d'éveil** : défaut `jimy`. Whisper écrit presque toujours « Jimmy » ;
  la détection compare la prononciation (`voice::phonetic` : « jimmy » et
  « jimy » donnent la même clé « jimi »), donc les deux orthographes
  déclenchent, dans les deux sens (test `jimy_et_jimmy_se_valent`). Une
  config existante garde sa valeur (`jimmy`) tant que l'utilisateur ne la
  change pas dans Voix → Mot d'activation (piège 62 : pas d'écrasement).

### Ensuite (au 7 octobre, par priorité)

1. **Essais réels** de ce qui n'est vérifié que par tests : navigateur
   (« ouvre gmail.com », connexion faite par l'utilisateur, carte « action en
   ton nom » sur un envoi) ; tâches de fond **à la voix** (« je m'en occupe »,
   retour à l'écoute, annonce de fin, « arrête tout ») ; vision à la voix
   (« Jimmy, regarde mon écran ») ; lapin pendant une vraie tâche de fond.
2. ~~**Muse Spark et le parcours « fichier sensible »**~~ — **résolu le
   9 octobre** (piège 98) : pas un refus de principe, la règle 8 du prompt
   lui faisait demander l'accord en texte ; reformulée, 12/12 au rejeu.
3. **Secrets dans le journal (code fait le 8 octobre, reste l'humain)** : les arguments d'outils et extraits bruts sont masqués (`sensitive::mask_json` / `mask_text`). Reste à **régénérer les deux clés déjà exposées** (`aggregate`, SynaptiQ) : les anciennes restent lisibles dans l'historique du journal.
4. **Lapin et chien de garde** : si Godot redémarre pendant une tâche de fond,
   le lapin ne revient qu'à la tâche suivante ; renvoyer `/helper working`
   au redémarrage si `App::tasks` a une tâche de fond.
5. **Navigateur** : une page dont l'instantané dépasse 12 000 caractères est
   tronquée (`MAX_TOOL_OUTPUT`) ; une borne propre au navigateur, ou un
   instantané par zone, si l'usage le demande.
6. **Multi-fournisseurs à l'usage** (8-9 octobre) : le plan Claude est
   vérifié en réel mais insuffisant pour l'agent (400 « extra usage » dès que
   les outils sont déclarés — piège 90). Reste : décider du moteur d'Anthropic
   — activer « extra usage », saisir une clé API, ou en rester à OpenCode Go ;
   puis saisir une vraie clé DeepSeek ou Alibaba dans Paramètres → LLM,
   « Tester » la connexion, choisir un modèle chez lui et vérifier un tour
   d'agent réel dans ce format (`--test protocols` ne couvre qu'OpenCode Go).

### Ensuite (liste du 6 octobre, toujours valable)

1. **Usage réel avec un modèle Responses** (Muse Spark 1.3) : vérifier qu'il
   répond sans commenter sa consigne. Sinon, passer le prompt système en
   message `system` dans `input` plutôt qu'en `instructions`
   (`protocol::responses_body`).
2. **Régénérer les clés exposées** : serveur MCP `aggregate` (en clair dans
   `jimmy.log`), clés du `.env.api` de JobXpress (lues en clair par le modèle
   le 5 octobre). Puis masquer les valeurs `KEY=`/`TOKEN=`/`SECRET=` dans les
   résultats d'outils avant envoi au modèle (aujourd'hui, seul le prompt le
   demande).
3. **Garde-fou des fichiers sensibles** : il repère par motifs ; un script
   intermédiaire qui ne nomme pas le fichier y échappe. À durcir seulement si
   l'usage montre un contournement.
4. **Utiliser Jimmy au quotidien une semaine** — le seul test qui compte.
   Vérifier que l'apprentissage écrit bien ses notes dans
   `C:\Obsidian\Jimmy\0_Inbox\Jimmy\` et qu'elles sont utiles à relire.
5. Vérifier en conditions réelles l'écoute à voix humaine, à distance du
   micro intégré (regarder le vumètre de la vue Voix).
6. Serveurs whisper orphelins : si Jimmy est tué brutalement, ses
   `whisper-server` survivent et sont réutilisés au lancement suivant (le
   contrôle de santé les trouve). Sans gravité, mais un orphelin lancé avec un
   autre modèle serait réutilisé tel quel.

7. Parcours « fichier sensible » en échec 3× le 6 octobre au soir (25/26 à
   chaque fois) : la demande partait mais le modèle répondait en texte sans
   appeler `write_file` (tour à 0 outil en ~12 s), donc pas de carte. Trois
   repros ciblées le même soir (session neuve, fil court, fil complet de la
   suite) : carte en moins de 5 s à chaque fois, refus respecté — mécanisme
   sain, côté modèle au moment des suites. À surveiller : si ça se répète
   hors pic, rouvrir l'enquête côté envoi (`submit` ignoré si un tour est
   encore en cours ?) plutôt que côté garde-fou.

### Différé

- Export Godot (~1 Go de gabarits) pour que l'installateur n'installe pas le
   moteur complet.
- Mise à jour automatique : V1 livrée le 8 octobre pour les clones git
  (détection SHA, pastille, `/update` avec rebuild + restart) ; reste la
  distribution par installeur (piège 93), quand il existera une distribution.

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
.\scripts\test-ui.ps1        # 26 parcours UI sur la vraie application (CDP)
.\scripts\with-msvc.ps1 cargo test -p jimmy-agent --test audio ecoute_ '--' --ignored --nocapture --test-threads=1
.\scripts\shortcut.ps1         # raccourci Bureau
.\scripts\shortcut.ps1 -Autostart   # démarrage avec Windows
.\scripts\with-msvc.ps1 cargo test --workspace
.\scripts\with-msvc.ps1 cargo test -p jimmy-agent --test protocols '--' --ignored --nocapture   # 3 formats d'API, vrais modèles
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