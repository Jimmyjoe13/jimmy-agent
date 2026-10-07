# INSTRUCTIONS — directives obligatoires pour travailler sur Jimmy

À lire **en premier**, par tout agent (Claude Code, OpenCode, Codex…) ou toute
personne qui touche au projet. Ce document résume les règles **non
négociables** ; le détail est dans `CONTRIBUTION.md` (règles et vérifications),
`HANDOFF.md` (état technique et **82 pièges numérotés**) et `JOURNAL.md`
(historique). Chaque règle ci-dessous a déjà coûté une régression réelle.

---

## 1. Avant de commencer

1. Lire `HANDOFF.md` : l'état actuel, les « Décisions prises », les pièges.
   Ne jamais reproposer une décision déjà tranchée.
2. Lire le code concerné, pas un résumé. Un document qui dit « fait » ne
   prouve rien : vérifier dans le code (`rg`).
3. Reformuler une demande floue et **attendre la validation** de
   l'utilisateur avant un chantier (architecture, nouvelle fonctionnalité,
   suppression, dépendance, changement de modèle ou de fournisseur).
4. Répondre en **français**, court et direct, tutoiement.

## 2. Jimmy est utilisé en direct : ne pas le casser

- **Avant tout build, test d'écoute ou arrêt de processus**, vérifier que
  l'utilisateur ne s'en sert pas :
  `data/logs/jimmy.log`, dernière ligne `[agent]`, `commande :` ou `[tts]`.
  Activité de moins de 2 minutes → **attendre** (ou demander).
- Arrêter Jimmy **avec tout son arbre** (`taskkill /T /F /PID <jimmy>`), sinon
  ses serveurs MCP et whisper restent orphelins.
- Ne jamais tuer un processus non identifié (chemin, parent, date).
- Les tests d'écoute (`--test audio … --ignored`) prennent les ports whisper
  8178/8179 : **Jimmy arrêté** pendant ce temps (piège 35).
- Après tout test, Jimmy doit tourner **une seule fois, sans port de
  débogage** (`test-ui.ps1` le relance proprement à la fin).

## 3. Compiler et vérifier : la seule procédure valable

```powershell
.\scripts\with-msvc.ps1 cargo test --workspace   # 0 échec, 0 avertissement
cd desktop; npm run build; cd ..                  # TypeScript strict
.\scripts\build.ps1 -Release                      # LE binaire (jamais cargo build --release seul)
.\scripts\test-ui.ps1                             # tous les parcours OK, 0 erreur JS
```

- Toujours depuis **PowerShell**, jamais Git Bash (mauvais `link.exe`).
- `cargo build --release` seul = interface vide « localhost a refusé de se
  connecter » (pièges 7, 56).
- Une modification d'interface n'est visible qu'après `build.ps1 -Release`
  (les assets sont embarqués dans le binaire).
- Scripts Godot : `--headless --check-only --script res://…` ; recharger
  l'avatar sans relancer Jimmy = tuer son Godot (le chien de garde le relance).

## 4. Méthode de correction

1. **Reproduire avant de corriger** : un test qui échoue (unitaire, intégration
   `--ignored`, ou rejeu d'une requête réelle `agent/tests/replay.rs`).
2. **Mesurer avant de supposer** : le journal donne les durées et les jetons
   (`appel au modèle … jetons : X en entrée, Y en sortie`). Les « évidences »
   se sont souvent révélées fausses (pièges 60, 70).
3. Une tâche qui écrit dans un **projet de l'utilisateur** se teste sur une
   **copie** (`robocopy … /XD .venv .git data`), jamais sur l'original.
4. Tout ajout doit avoir un **appelant réel** (`rg`) : une fonction définie et
   jamais appelée est déjà arrivée quatre fois.
5. Un changement = un besoin. Une amélioration repérée se signale en une ligne
   à la fin, elle ne se glisse pas dans le correctif.
6. Ne jamais dire « corrigé » sans l'avoir observé ; distinguer **vérifié** et
   **non vérifié** dans le compte rendu.

## 5. Pièges techniques à connaître absolument

| Domaine | Règle | Piège |
|---|---|---|
| Tauri | Une structure `Deserialize` reçue de l'interface porte `#[serde(rename_all = "camelCase")]` (Tauri ne renomme que le 1er niveau) | 59 |
| Tauri | Commande ajoutée = Rust + `invoke_handler` + `api.ts` + un appelant dans l'interface | — |
| Interface | Élément masqué par `hidden` et stylé en `display` → règle `[hidden] { display: none }` | 68 |
| Interface | Commande sans retour : `attempt` ; avec données : `guard` | 13 |
| Interface | Écouteurs via `ctx.onEvent` / écouteur porté par la vue, jamais global nu | — |
| Interface | Classes CSS propres à chaque composant (réutiliser celles d'un autre fausse ses sélecteurs et ses tests) | 63 |
| Config | Le `.env` ne donne que des **valeurs de départ** ; jamais d'écrasement des choix de l'utilisateur | 62 |
| Base | Colonne ou table ajoutée → monter `SCHEMA_VERSION` + `ALTER` explicite pour les bases existantes | 30 |
| Historique | `History::conversation` ne renvoie jamais de messages `tool` (HTTP 400) | 45 |
| Modèle | `max_tokens` ≥ 16 384 : le raisonnement caché compte ; une réponse coupée arrive en texte (`core::inline_tools`) | 70 |
| Modèle | Ne pas « corriger » une dérive du modèle par la température (rejouer d'abord) | 60 |
| Outils | Chaque outil borne son temps et sa sortie ; un gros fichier se lit par morceaux (`start_line`) | 55, 70 |
| MCP | Outils MCP **à la demande** (`mcp_list_tools`, `mcp_call`) : ne jamais renvoyer leurs ~80 définitions à chaque appel | 70 |
| Voix | Ce qui doit survivre à la boucle d'écoute va sur `App`, pas dans la boucle | 54 |
| Voix | Toute tâche passe par `App::start_task` (arrêt « STOP », passage en fond) ; ne pas contourner | 71, 82 |
| Vault | Partagé avec d'autres agents : Jimmy n'écrit que dans `0_Inbox/Jimmy` et distingue ses notes | 66 |
| Modèle | Chaque modèle a **un** format d'API (Chat, Responses, Messages), lu dans le catalogue ; un appel passe par `LlmClient::send`, jamais par une URL `/chat/completions` en dur | 78 |
| Sécurité | Une écriture dans un fichier sensible passe par `sensitive::authorize` (carte dans le Chat) ; la lecture reste libre | 77 |
| Chat | Le premier appel au modèle est en flux (`chat_stream`) ; les chemins de secours rejouent `chat` sans flux | 74 |
| Tâches | Un tour de l'agent passe par `App::start_task` (jamais `agent::run` nu) ; le délai de passage en fond part du premier outil | 82 |

## 6. Tests d'interface : ils touchent aux données réelles

- Une étape qui modifie un réglage (modèle, voix, projet) **relève la valeur
  avant et la restaure** à la fin, vérifiée par une commande (piège 61).
- Lire l'**état réel** (session, commande `status`), pas l'affichage : une
  commande vocale parasite peut s'afficher dans le Chat.
- Supprimer les sessions créées par le test.
- Un échec isolé d'une étape gênée par l'écoute se **relance** avant
  d'enquêter (instabilités connues, piège 62). « Skin » attend désormais
  l'état du bouton (Godot se relance à chaque changement) au lieu d'un délai
  fixe : un échec y est un vrai signal.

## 7. Secrets et données

- Aucun secret dans un prompt, un commit, un journal ou une réponse. Clés dans
  `%USERPROFILE%\.secrets\`, lecture ciblée d'une seule valeur.
- **Le dépôt est public** (github.com/Jimmyjoe13/jimmy-agent, depuis le
  6 octobre) : aucun chemin personnel, IP, nom de clé, adresse ni valeur
  réelle dans le code, les tests ou les docs — des exemples neutres
  (`C:\Users\user`, `203.0.113.10`, `vps.key`).
- `data/` (base, journaux, audio) ne se committe jamais et ne se recopie pas
  dans une réponse.
- **Ouvert** : la clé du serveur MCP `aggregate` figure en clair dans
  `data/logs/jimmy.log` (arguments d'outils journalisés tels quels) → à
  régénérer, et à masquer dans ce journal.

## 8. Git

- **Rien n'est poussé sans demande explicite.** Pas de `--force`, pas de
  `reset --hard`.
- Commits en français, un sujet par commit, qui dit quoi et pourquoi.
- Avant de committer un gros lot : relire le diff fichier par fichier et
  relancer les vérifications du §3.
- Dépôt distant : `origin` = github.com/Jimmyjoe13/jimmy-agent (**public**).
  Identité des commits : `Jimmyjoe13 <192435933+Jimmyjoe13@users.noreply.github.com>`
  (l'historique a été réécrit le 6 octobre pour retirer l'adresse
  personnelle : les identifiants de commit d'avant ont changé).
- État au 6 octobre 2026 : tout est commité ; `main` est publié jusqu'à
  `bb71b2b`, les commits suivants attendent une demande de push.

## 9. Fin de travail

1. Consigner chaque piège découvert dans `HANDOFF.md` (numéro suivant, jamais
   renuméroter), chaque décision dans « Décisions prises », le récit dans
   `JOURNAL.md`. Ne jamais écraser ces fichiers : ajouter.
2. Terminer par trois lignes :

```
Fait : <ce qui est vérifié, avec les fichiers>
Restant : <les étapes suivantes, dans l'ordre>
Bloqué : <ce qui empêche d'avancer, ou "rien">
```

## 10. Préférences de l'utilisateur (validées)

- Modèle : **MiMo-V2.6-Flash** (principal et vocal). Voix : **« Le narrateur »**
  (Fish Audio `4f2a0684dd0247dda68f339738c780e6`).
- Jimmy doit être **réactif et aller au bout** d'une tâche validée, en disant
  où il en est ; « STOP » l'arrête à tout moment.
- **Sortie courte et nette (validée et livrée le 5 octobre)** : les
  réponses de Jimmy doivent être courtes, sans blabla inutile, précises — il
  n'explique que l'essentiel et va droit au but. La synthèse vocale ne doit
  pas réciter du code ni du jargon de dev ; l'écran peut être plus détaillé
  que la voix, l'inverse est interdit.
- **Fichiers sensibles** (5-6 octobre) : Jimmy ne modifie jamais un `.env`,
  une clé ou un secret sans accord explicite (carte dans le Chat) ; il ne
  doit **pas** demander pour une simple lecture (« il demande trop
  l'autorisation »). Piège 77.
- **Tous les modèles du compte doivent marcher**, quel que soit leur format
  d'API (6 octobre). Piège 78.
- Ouvrir un fichier = **application par défaut de Windows**.
- Prochains chantiers validés : lot 2 du Chat livré (5 octobre : @-mentions,
  chemins cliquables, bloc travaux + diff, historique groupé) ; **tâches
  longues en arrière-plan livrées le 7 octobre** (une seule à la fois,
  « STOP » = premier plan, « arrête tout » = tout ; HANDOFF, décisions du
  7 octobre et piège 82).
