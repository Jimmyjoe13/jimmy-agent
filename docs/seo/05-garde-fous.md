# 05 — Garde-fous du bot SEO

Ce fichier prime sur tout le reste en cas de doute. Règle générale : **dans le doute, ne pas
agir, demander à Jimmy** (message M9 ou question directe).

## 1. Ce que le bot fait seul

- Lire tout le dépôt, le site en ligne, la Search Console et GA4 (lecture), le web public.
- Créer, modifier et pousser des branches `seo/<ID>` dans le périmètre de `garde_chemins.py tache`.
- Lancer les outils de `docs/seo/outils/`.
- Soumettre le sitemap et inspecter des URL dans la Search Console.
- Commits d'administration sur `main` limités au périmètre `garde_chemins.py admin`
  (journal, calendrier, backlog, rapports) : P8.
- Annuler une publication si l'accueil du site ne répond plus 200 juste après P3 (P7-B).
- Envoyer des messages Telegram à Jimmy.

## 2. Ce qui exige « SEO OK <ID> » de Jimmy

- Toute fusion dans `main` qui touche `site-jimy/` (= toute mise en ligne).
- Toute redirection (`site-jimy/vercel.json`), suppression ou renommage de page : uniquement
  dans une tâche dont le brief le prévoit (S04), signalé en tête de M1.
- L'ajout de tâches au calendrier ou la modification des briefs (seulement via la revue S12).
- Les textes destinés à l'extérieur (tâches H) : le bot prépare, **Jimmy publie lui-même**.

## 3. Ce que le bot ne fait jamais

- Pousser sur `main` sans validation hors du périmètre admin ; `git push --force` ;
  `git reset --hard` sur `main` ; réécrire l'historique ; supprimer une branche autre que `seo/*`.
- Modifier hors périmètre : code de l'appli (`agent/`, `desktop/`, `godot/`…), `site-jimy/api/`,
  `site-jimy/assets/tracking.js`, les accueils `site-jimy/index.html` et `site-jimy/en/index.html`,
  `site-jimy/vercel.json` (sauf S04), et ses propres règles (`docs/seo/AGENTS.md`, `0*.md`,
  `outils/`, `modeles/`, `hermes/`). Une règle lui semble fausse → il le dit à Jimmy.
- Écrire un secret (clé, jeton, mot de passe, identifiant Telegram) dans un fichier du dépôt :
  **le dépôt est public**.
- Créer un compte, se connecter à une plateforme, publier hors du site, dépenser de l'argent.
- Inventer un fait, un chiffre, une fonction de Jimy ou une source (03-charte §3).
- Modifier une page publiée en dehors d'une tâche validée (les optimisations passent par le backlog).
- Lancer la compilation de l'appli Jimy ou toucher aux processus de la machine de Jimmy.

## 4. Arrêts obligatoires (P7)

| Situation | Réaction |
|---|---|
| `verifier.py` en erreur après 3 tentatives | P7-A |
| `git pull --ff-only` refusé, conflit de fusion, dépôt non propre au démarrage | P7-A |
| `garde_chemins.py` refuse | P7-A, ne pas pousser |
| Aperçu ou production Vercel indisponible après le délai | P7-A |
| Accueil ≠ 200 après une publication | P7-B automatique, puis M9 |
| Erreur 401/403 Google, clé absente | P7-A (ne pas retenter en boucle) |
| Impressions en baisse de plus de 50 % sur une semaine (≥ 50 impressions avant) | Alerte en tête de M3, aucune action corrective seule |
| Même tâche en P7 deux fois sans intervention de Jimmy | Mettre en pause les tâches `jimy-seo-*` et le dire |

## 5. Rythme et charge

- Au plus **2 tâches de contenu ouvertes** (branche existante) en même temps : `calendrier.py
  prochaine` l'applique.
- Une seule tâche de contenu produite par exécution du cron du lundi.
- Une tâche en validation depuis plus de 7 jours bloque la suivante tant que le maximum est
  atteint ; relance M6 le jeudi.
- Outils web : `suggestions.py` au plus une vingtaine d'appels par tâche ; pas de collecte massive.

## 6. Ce que le bot ne sait pas faire (et doit demander)

- « Demander une indexation » dans la Search Console (pas d'API) → Jimmy.
- Les libellés exacts de l'interface de Jimy quand ils ne sont pas dans le code → Jimmy.
- Toute action sur une plateforme externe → Jimmy.
