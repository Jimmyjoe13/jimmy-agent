# 04 — Procédures du bot SEO

Chaque procédure s'exécute **dans l'ordre**, étape par étape. Une étape qui échoue arrête la
procédure : appliquer P7 (incident), sauf mention contraire. Les commandes sont à lancer
telles quelles sur le VPS (Linux, bash).

## Constantes

| Nom | Valeur |
|---|---|
| Dépôt (clone du bot) | `/home/jimmy/jimy-seo/jimmy-agent` |
| Outils | `/home/jimmy/jimy-seo/jimmy-agent/docs/seo/outils` (appelés depuis la racine du dépôt) |
| Python | `python3` |
| Clé Google (compte de service) | `~/.secrets/jimy_seo_google.json` |
| Remote git | `origin` = `git@github-jimy-seo:Jimmyjoe13/jimmy-agent.git` |
| Branche de production | `main` (Vercel publie automatiquement chaque push sur `main` qui touche `site-jimy/`) |
| Branche d'une tâche | `seo/<ID>` (ex. `seo/S01`) |
| Site | `https://jimy-agent.me` |
| Messages | Telegram, canal par défaut de Hermes |
| Hermes (ligne de commande) | `/home/jimmy/.hermes/hermes-agent/venv/bin/hermes` (pas dans le PATH) |

Toutes les commandes `python3 docs/seo/outils/…` se lancent depuis la racine du dépôt.

---

## P0 — Démarrage (au début de CHAQUE exécution, cron ou message)

```bash
cd /home/jimmy/jimy-seo/jimmy-agent
git status --porcelain            # doit être vide
git fetch origin --prune
git checkout main
git pull --ff-only origin main
```

- `git status` non vide : une exécution précédente s'est interrompue. Ne rien effacer :
  P7 avec la liste des fichiers.
- `pull --ff-only` refusé : P7 (quelqu'un a réécrit `main` ; ne jamais forcer).
- Relire `docs/seo/AGENTS.md` si sa date de modification a changé depuis la dernière exécution
  (`git log -1 --format=%cd -- docs/seo/`).

---

## P1 — Produire une tâche de contenu (cron du lundi)

1. **P0.**
2. **Choisir la tâche** : `python3 docs/seo/outils/calendrier.py prochaine`
   - `RIEN : …` → répondre exactement `[SILENT]` et s'arrêter. Exception : si la raison est
     « tâches ouvertes » et qu'une d'elles est `en_validation` depuis plus de 7 jours, envoyer
     le message M6 (relance).
   - Une tâche de type `revue` → appliquer le brief S12 de `02-plan-contenu.md`.
3. **Ouvrir la branche** :
   ```bash
   git checkout -b seo/<ID> origin/main
   python3 docs/seo/outils/calendrier.py statut <ID> en_redaction
   ```
4. **Lire** : le brief `## <ID>` de `02-plan-contenu.md`, `03-charte-redactionnelle.md` en
   entier, le thème correspondant de `01-strategie-mots-cles.md`, et les pages existantes à
   lier (pour le ton et pour ne pas se contredire).
5. **Valider les mots-clés** :
   ```bash
   python3 docs/seo/outils/suggestions.py "<mot-clé principal FR>" --langue fr --questions
   python3 docs/seo/outils/suggestions.py "<mot-clé principal EN>" --langue en --questions
   ```
   Garder 2-3 questions suggérées pertinentes pour la FAQ. Si le mot-clé principal ne sort
   pas, appliquer la règle du brief (variante la plus proche, à signaler).
6. **Rechercher les faits** nécessaires (charte §3) : sources officielles, consultées le jour
   même. Noter chaque URL source.
7. **Rédiger** les deux pages en copiant les gabarits :
   ```bash
   cp docs/seo/modeles/guide-fr.html site-jimy/<chemin FR du brief>
   cp docs/seo/modeles/guide-en.html site-jimy/<chemin EN du brief>
   ```
   Remplir tous les `{{…}}`, supprimer les commentaires de gabarit en majuscules. Dates :
   `DATE_ISO` = jour de rédaction au format `2026-11-30T09:00:00+01:00` (+02:00 en heure d'été).
   Puis le **maillage entrant** (hubs, page(s) « entrant » du brief, `sitemap.xml`), selon les
   règles communes de `02-plan-contenu.md`.
8. **Vérifier** (boucle : corriger puis relancer, **3 tentatives au maximum**) :
   ```bash
   python3 docs/seo/outils/verifier.py page site-jimy/<FR> --mot-cle "<mot-clé FR>"
   python3 docs/seo/outils/verifier.py page site-jimy/<EN> --mot-cle "<mot-clé EN>"
   python3 docs/seo/outils/verifier.py site
   ```
   0 ERREUR exigée sur les trois. Les ALERTES sont acceptées si elles sont expliquées dans M1.
   Après 3 échecs : P7.
9. **Passer en validation, contrôler le périmètre, pousser** (un seul commit) :
   ```bash
   python3 docs/seo/outils/calendrier.py statut <ID> en_validation
   git add -A site-jimy docs/seo/calendrier.yaml
   git commit -m "SEO <ID> : <titre de la tâche>"
   python3 docs/seo/outils/garde_chemins.py tache        # S04 seulement : ajouter --vercel-json
   git push -u origin seo/<ID>
   ```
   `garde_chemins` en erreur → ne pas pousser, P7.
   **Règle Vercel** : le dernier commit poussé sur une branche `seo/*` ou sur `main` doit
   modifier au moins un fichier de `site-jimy/`, sinon Vercel ne construit rien (règle
   « ignoreCommand » de `site-jimy/vercel.json`). D'où un commit unique pages + calendrier.
10. **Attendre l'aperçu** : `python3 docs/seo/outils/apercu_vercel.py seo/<ID>` (15 min max).
    Échec → P7 (la branche reste poussée).
11. **Envoyer M1** à Jimmy (réponse finale du cron), puis revenir sur `main` (`git checkout main`).

---

## P2 — Comprendre un message de Jimmy

Les messages Telegram commençant par `SEO` sont des commandes. Insensible à la casse ;
`<ID>` = identifiant du calendrier (S01, H02…).

| Message | Action |
|---|---|
| `SEO OK <ID>` | Tâche de contenu ou revue : **P3**. Tâche hors site : P6 étape 4. |
| `SEO CORRIGE <ID> : <consignes>` | P0, `git checkout seo/<ID>`, `git pull --ff-only`, appliquer les consignes, reprendre P1 à l'étape 8 (vérifier, puis étape 9 : statut `en_validation` + commit unique + push, aperçu, M1). |
| `SEO ABANDON <ID>` | P0, `git push origin --delete seo/<ID>` (si elle existe), `calendrier.py statut <ID> abandonne --note "<raison donnée>"`, commit admin (P8), répondre M7. |
| `SEO FAIT <ID>` | Jimmy confirme une action hors site ou manuelle : `calendrier.py statut <ID> fait`, P8, répondre M7. |
| `SEO ETAT` | P0, `calendrier.py etat` + branches `git branch -r --list 'origin/seo/*'` → répondre M8. |
| `SEO RAPPORT` | Lancer P4 tout de suite. |
| `SEO PAUSE` | Mettre en pause les tâches planifiées (`hermes cron pause <id>` pour chaque tâche `jimy-seo-*`), répondre M7. |
| `SEO REPRISE` | Reprendre les tâches planifiées (`hermes cron resume <id>`), répondre M7. |
| `SEO ANNULE <ID>` | Annuler une publication : P7-B. |
| Autre message `SEO …` | Répondre la liste des commandes ci-dessus ; ne rien exécuter. |

Un message qui ne commence pas par `SEO` n'est **pas** une commande du bot SEO : le traiter
normalement, sans toucher au dépôt ni au calendrier.

---

## P3 — Publier une tâche validée (`SEO OK <ID>`)

1. **P0.** Vérifier que `origin/seo/<ID>` existe et que la tâche est `en_validation` sur la
   branche (`git show origin/seo/<ID>:docs/seo/calendrier.yaml`). Sinon répondre que la tâche
   n'est pas en attente de validation, et s'arrêter.
2. **Fusionner sans valider le commit** :
   ```bash
   git merge --no-ff --no-commit origin/seo/<ID>
   ```
   Conflit → `git merge --abort`, P7 (ne jamais résoudre un conflit sur une page sans Jimmy).
3. **Mettre à jour, contrôler, valider en UN commit, pousser** :
   ```bash
   python3 docs/seo/outils/calendrier.py statut <ID> publie
   # sitemap.xml : lastmod des URL de la tâche = date du jour
   python3 docs/seo/outils/verifier.py site
   git add -A site-jimy docs/seo/calendrier.yaml
   git commit -m "SEO <ID> : publication validée par Jimmy"
   git push origin main
   git push origin --delete seo/<ID>
   ```
   Un seul commit de fusion qui contient les pages : Vercel le publie (règle Vercel de P1 étape 9).
   `verifier.py site` en erreur → `git merge --abort`, P7.
4. **Attendre la production** : `python3 docs/seo/outils/apercu_vercel.py --production` (10 min).
5. **Contrôler en ligne** : chaque URL de la tâche et `https://jimy-agent.me/` doivent répondre 200 :
   ```bash
   curl -s -o /dev/null -w "%{http_code}\n" <URL>
   ```
   Accueil ≠ 200 → **P7-B immédiatement** (annulation automatique autorisée, 05-garde-fous §4).
   Page de la tâche ≠ 200 alors que l'accueil répond → P7 sans annuler.
6. **Signaler à Google** :
   ```bash
   python3 docs/seo/outils/seo_data.py sitemap
   python3 docs/seo/outils/seo_data.py inspecter <URL FR> <URL EN>
   ```
7. **Journaliser** dans `docs/seo/journal.md` (format §Journal ci-dessous), puis P8.
8. **Envoyer M2.**

---

## P4 — Rapports (cron du lundi 07:00 UTC ; mensuel le 1er ; ou `SEO RAPPORT`)

1. **P0.**
2. Hebdomadaire : `python3 docs/seo/outils/seo_data.py rapport --jours 7`
   Mensuel : `python3 docs/seo/outils/seo_data.py rapport --jours 28`
3. Lire la sortie et appliquer les règles de `06-mesure.md` :
   - déclencheur R3 → ajouter la requête à `backlog.md` (si absente) ;
   - alerte « baisse de plus de 50 % » → la mettre en tête du message.
4. Mensuel seulement : écrire le rapport complet dans `docs/seo/rapports/AAAA-MM.md` et une
   entrée résumée dans `journal.md`, puis P8.
5. **Envoyer M3** (synthèse courte : la sortie brute ne part pas sur Telegram).

---

## P5 — Suivi (cron du jeudi 08:00 UTC)

1. **P0.**
2. `python3 docs/seo/outils/calendrier.py dues` → pour chaque ligne :
   - `CONTROLE_J14 <ID>` : `seo_data.py inspecter <URLs>`. Toute URL non indexée → l'inclure
     dans M4 (Jimmy doit cliquer « Demander une indexation » dans la Search Console, l'API ne
     le permet pas) et vérifier le maillage entrant (`verifier.py site`). Puis
     `calendrier.py statut <ID> controle_j14`.
   - `MESURE_J28 <ID>` : `seo_data.py page <URL> --jours 28` pour chaque URL ; appliquer
     R1/R2/R4/R5 de `06-mesure.md` ; écrire `docs/seo/rapports/<ID>-j28.md` ;
     `calendrier.py statut <ID> mesure`. Les propositions d'amélioration deviennent des
     entrées de `backlog.md` (type « optimisation »), jamais des modifications directes.
   - `HORS_SITE <ID>` : P6.
3. Tâches `en_validation` depuis plus de 3 jours (date du dernier commit de la branche :
   `git log -1 --format=%cs origin/seo/<ID>`) → M6.
4. Si au moins une action a eu lieu : P8 puis M4. Sinon répondre `[SILENT]`.

---

## P6 — Tâche hors site (H01…)

1. `git checkout -b seo/<ID> origin/main`.
2. Lire le brief `H0x` (fin de `02-plan-contenu.md`), rédiger `docs/seo/rapports/<ID>-brouillon.md` :
   textes prêts à copier, liens, champs du formulaire de la plateforme, jour conseillé.
3. `calendrier.py statut <ID> en_validation`, commit, `garde_chemins.py tache`, push de la
   branche, **M5** (contient le brouillon en entier).
4. Sur `SEO OK <ID>` : répondre « À toi de publier ; écris `SEO FAIT <ID>` une fois fait. »
   Sur `SEO FAIT <ID>` :
   ```bash
   git checkout main && git pull --ff-only origin main
   git merge --no-ff --no-commit origin/seo/<ID>
   python3 docs/seo/outils/calendrier.py statut <ID> fait
   git add docs/seo && git commit -m "SEO <ID> : fait par Jimmy"
   python3 docs/seo/outils/garde_chemins.py admin
   git push origin main && git push origin --delete seo/<ID>
   ```
   (Ne touche pas `site-jimy/` : pas de déploiement, c'est normal.)

Le bot ne crée aucun compte, ne se connecte à aucune plateforme, ne publie rien hors du site.

---

## P7 — Incident

**P7-A (par défaut)** : s'arrêter, laisser l'état tel quel (ne rien effacer, ne rien forcer),
envoyer **M9** avec : procédure et étape, commande exacte, sortie d'erreur (20 dernières
lignes), état git (`git status`, branche), et ce qui reste à faire. Si la même tâche a déjà
déclenché P7 deux fois sans intervention de Jimmy : mettre en pause les tâches planifiées
`jimy-seo-*` et le dire dans M9.

**P7-B (annuler une publication)** — sur `SEO ANNULE <ID>`, ou automatiquement si l'accueil
ne répond plus 200 après P3 :
```bash
git checkout main && git pull --ff-only origin main
git revert -m 1 --no-edit <sha du commit de fusion « SEO <ID> : publication validée »>
git push origin main
python3 docs/seo/outils/apercu_vercel.py --production
curl -s -o /dev/null -w "%{http_code}\n" https://jimy-agent.me/
python3 docs/seo/outils/calendrier.py statut <ID> en_correction --note "publication annulée : <raison>"
```
Puis P8 et M9. Ne jamais utiliser `git reset` ni `git push --force` sur `main`.

---

## P8 — Commit d'administration sur main

Pour les fichiers de suivi uniquement (journal, calendrier, backlog, rapports), sans validation :
```bash
git checkout main && git pull --ff-only origin main
git add docs/seo/journal.md docs/seo/calendrier.yaml docs/seo/backlog.md docs/seo/rapports
git commit -m "SEO suivi : <résumé>"
python3 docs/seo/outils/garde_chemins.py admin
git push origin main
```
`garde_chemins admin` en erreur → `git reset --soft HEAD~1`, P7.

---

## Journal (`docs/seo/journal.md`)

Une entrée par événement notable, la plus récente en bas :
```
### AAAA-MM-JJ — <ID> <titre court>
- Fait : <ce qui a été fait, URLs>
- Résultat : <chiffres ou état>
- Suite : <prochaine étape, ou « rien »>
```

---

## Messages Telegram

Courts, en français, sans tableau Markdown (illisible sur mobile). Toujours commencer par
l'identifiant de la tâche.

**M1 — Demande de validation**
```
SEO <ID> à valider : <titre>
Aperçu FR : <URL d'aperçu>/<chemin FR>
Aperçu EN : <URL d'aperçu>/<chemin EN>
Mots-clés : <FR> / <EN> (suggestions Google : oui/variante <x>)
Vérifications : 0 erreur (<n> alertes : <résumé>)
Pages modifiées pour le maillage : <liste>
Sources utilisées : <URLs>
À vérifier par toi : <points incertains, ou « rien »>
Réponds : SEO OK <ID> · SEO CORRIGE <ID> : … · SEO ABANDON <ID>
```

**M2 — Publié**
```
SEO <ID> en ligne : <URL FR> · <URL EN>
Sitemap resoumis, indexation : <état de l'inspection>
Action pour toi : Search Console → Inspection d'URL → « Demander une indexation » pour les 2 URL.
```

**M3 — Rapport** : 5 à 8 lignes : clics, impressions (dont hors marque) et évolution ;
téléchargements ; 3 requêtes hors marque principales ; alertes ; actions décidées.

**M4 — Suivi** : une ligne par action faite (contrôle J+14, mesure J+28, backlog alimenté) +
les actions demandées à Jimmy.

**M5 — Hors site à valider** : `SEO <ID> : brouillon prêt`, la plateforme, le texte complet, puis
« Réponds SEO OK <ID> pour le garder, SEO CORRIGE <ID> : … pour le modifier. »

**M6 — Relance** : `SEO <ID> attend ta validation depuis <n> jours : <aperçu>`.

**M7 — Accusé** : une ligne confirmant l'action effectuée.

**M8 — État** : tâches ouvertes, prochaine tâche et sa date, dernières publications.

**M9 — Incident** : `⚠️ SEO incident <ID ou procédure>` + contenu décrit en P7-A.
