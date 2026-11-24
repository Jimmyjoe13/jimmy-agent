# SEO de jimy-agent.me — dossier de pilotage

Pour Jimmy (humain). Le bot, lui, commence par `AGENTS.md`.

## En une phrase

Un bot Hermes sur le VPS rédige chaque semaine une page FR + EN ciblant des requêtes qui
mènent au téléchargement de Jimy, te l'envoie en aperçu sur Telegram, la publie quand tu
réponds `SEO OK`, puis mesure ses résultats et te fait un rapport.

## Comment ça marche

```
Lundi 07:00 UTC  rapport hebdo (Search Console + GA4)            → Telegram
Lundi 08:00 UTC  prochaine tâche du calendrier :
                 branche seo/<ID> → pages FR+EN → vérifications → push
                 → aperçu Vercel automatique                        → Telegram « à valider »
Toi              SEO OK <ID>  |  SEO CORRIGE <ID> : …  |  SEO ABANDON <ID>
Bot              fusion dans main → Vercel publie → sitemap Google  → Telegram « en ligne »
Jeudi 08:00 UTC  suivi : indexation J+14, mesure J+28, relances    → Telegram (ou silence)
1er du mois      rapport mensuel archivé dans rapports/
```

GitHub fait foi : Vercel publie automatiquement ce qui arrive sur `main` dans `site-jimy/`.
Tu ne publies plus avec `vercel --prod` ; tu pousses sur `main`, après un `git pull`
(le bot y pousse aussi).

## Tes commandes Telegram (bot Richard)

Dans le menu « / » de ta conversation avec Richard :

| Commande | Effet |
|---|---|
| `/seo` | État du bot SEO ; suivi d'un mot : `/seo ok S03`, `/seo corrige S03 …`, `/seo pause`… |
| `/seo_rapport` | Rapport d'audit Search Console + GA4 sur 28 jours (immédiat) |
| `/seo_derniere` | Dernière tâche SEO terminée, dernière entrée du journal, prochaine tâche |

Les messages texte complets marchent aussi :

| Message | Effet |
|---|---|
| `SEO OK S03` | Publie la tâche S03 (ou valide un brouillon hors site) |
| `SEO CORRIGE S03 : raccourcis l'intro, ajoute Ollama` | Le bot corrige et renvoie un aperçu |
| `SEO ABANDON S03` | Tâche abandonnée, branche supprimée |
| `SEO FAIT H02` | Tu as publié l'action hors site H02 toi-même |
| `SEO ETAT` | Où en est le calendrier |
| `SEO RAPPORT` | Rapport tout de suite |
| `SEO PAUSE` / `SEO REPRISE` | Coupe / relance toutes les tâches planifiées du bot |
| `SEO ANNULE S03` | Retire une publication (revert git) |

## Ce qui te revient

- Valider (ou corriger) chaque page avant sa mise en ligne.
- Cliquer « Demander une indexation » dans la Search Console quand le bot le demande (pas d'API).
- Publier toi-même les actions hors site (AlternativeTo, itch.io, Reddit…) préparées par le bot.
- Confirmer les objectifs de `06-mesure.md` §3 et la revue trimestrielle (S12).

## Fichiers

| Fichier | Contenu |
|---|---|
| `AGENTS.md` | Point d'entrée du bot |
| `01-strategie-mots-cles.md` | Stratégie de mots-clés (7 thèmes) |
| `02-plan-contenu.md` | Brief de chaque tâche |
| `03-charte-redactionnelle.md` | Ton, structure, sources, promesses interdites, fiche produit |
| `04-procedures.md` | Procédures P0-P8, messages M1-M9 |
| `05-garde-fous.md` | Autonomie, interdits, arrêts |
| `06-mesure.md` | Indicateurs, objectifs, déclencheurs |
| `calendrier.yaml` | Tâches et statuts (modifié par `outils/calendrier.py`) |
| `journal.md` · `backlog.md` · `rapports/` | Historique, idées, rapports |
| `modeles/` | Gabarits HTML des pages FR et EN |
| `outils/` | Scripts Python (vérification, données, calendrier, aperçu, périmètre) |
| `hermes/` | Skill Hermes et procédure d'installation (S00) |

## Faire évoluer les règles

Les règles (`AGENTS.md`, `0*.md`, `outils/`, `modeles/`, `hermes/`) ne sont modifiées que par
toi ou Claude Code, jamais par le bot. Après modification : pousser sur `main` ; le bot les
relit au début de chaque exécution. Si `hermes/SKILL.md` change, le recopier sur le VPS
(voir `hermes/installation.md` §5).

Tester les outils en local (PC) : définir `JIMY_SEO_GOOGLE_KEY` vers la clé de
`site-jimy/analytics-credentials/`, puis `python docs/seo/outils/<outil>.py`.
