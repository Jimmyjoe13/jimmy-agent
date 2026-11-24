# Journal SEO — jimy-agent.me

Événements notables du référencement (une entrée par événement, la plus récente en bas,
format défini dans `04-procedures.md` § Journal). Remplace `site-jimy/JOURNAL_SEO.md` et
`docs/SUIVI_SEO_MENSUEL.md` depuis le 24/11/2026 ; leur contenu est repris ci-dessous.

## État au 22 octobre 2026

| Indicateur | Valeur | Source |
|---|---|---|
| URL | https://jimy-agent.me/ | — |
| Page d'accueil indexée | OUI (PASS / Submitted and indexed) | Search Console (API) |
| Sitemap soumis | 22/10/2026 (19:59 UTC, sans erreur) | Search Console (API) |
| Pages soumises au sitemap | 9 pages + 9 images | Search Console (API) |
| Pages indexées | En attente de crawl (sitemap en cours de traitement) | Search Console (API) |
| Impressions / clics | 0 (fenêtre jeune < 10 jours) | Search Console (API) |
| Backlinks | 0 (PR awesome-ai-agents #1781 & SaaSHub en cours) | Search Console |
| Version GA4 | G-FK2NSSHQWM | code du site |
| En-têtes sécurité & cache | Actifs (X-Frame-Options, X-Content-Type-Options, Referrer, CSP) | vercel.json (Vercel Prod) |
| Santé technique Screaming Frog | 100% propre (zéro rupture Hn, titres < 60 car, meta < 155 car, alts renseignés) | Code source & Vercel |

## Historique — État au 17 octobre 2026

| Indicateur | Valeur | Source |
|---|---|---|
| URL | https://jimy-agent.me/ | — |
| Page d'accueil indexée | OUI (PASS) | Search Console (API) |
| Sitemap soumis | 17/10/2026, sans erreur | Search Console |
| Pages soumises au sitemap | 6 pages + 9 images | Search Console |
| Pages indexées | 0 | Search Console |
| Impressions / clics | 0 (fenêtre vide) | Search Console |
| Backlinks | 0 | Search Console |
| Version GA4 | G-FK2NSSHQWM | code du site |

## Suivi mensuel

### 2026-10-17 — Lancement du suivi

- Contexte : le plan SEO de jimy-agent.me est terminé (16 tâches sur 16), il
  manquait un journal dédié au référencement dans le dossier du site.
- Fait : création de ce fichier ; reprise de l'état mesuré à cette date
  (indexation, sitemap, GA4) ; renvoi vers le protocole de collecte.
- Résultat : le site est sain et indexable, mais aucune page n'est encore
  indexée et le sitemap n'a pas été exploré. Zéro impression, zéro backlink.
- Blocage connu : la lecture GA4 par API est impossible, le jeton du connecteur
  MCP n'a pas le droit `analyticsdata`. Le suivi du trafic se fera donc à la
  main dans l'interface GA4 tant que le droit n'est pas accordé.
- Prochaine collecte : début novembre 2026 (impressions, clics, positions,
  indexation, nouveaux backlinks).

### 2026-10-22 — Audit complet Screaming Frog & Correctifs On-Page / Headers

- Contexte : crawl complet Screaming Frog réalisé le 22/10/2026 (`screaming-frog/22102026`). Diagnostic de l'absence de trafic organique (domaine jeune, 0 backlink, 8 pages).
- Fait par l'agent `Tech-SEO-Auditor` :
  1. Création de `vercel.json` : configuration des en-têtes de sécurité HTTP (`X-Frame-Options`, `X-Content-Type-Options`, `Referrer-Policy`, `Permissions-Policy`, `Content-Security-Policy`) et mise en cache long-terme des assets `/assets/`.
  2. Optimisation des balises `<title>` et `<meta name="description">` sur 7 pages pour éliminer les troncatures Google (< 60 car. / 560 px et < 155 car.).
  3. Correction de la hiérarchie Hn dans `guides/serveurs-mcp-windows-local.html` (remplacement du `<h3>Sommaire` par un paragraphe stylé, passage en séquence 100% logique).
  4. Accessibilité & CLS images : ajout des `alt` descriptifs et dimensions `width`/`height` sur les miniatures et boutons de skins (`index.html` et `en/index.html`).
  5. Compression WebP de `ark.webp` de 106,9 Ko à 95,5 Ko (< 100 Ko).
  6. Ajout de `hreflang="x-default"` sur les pages de nouveautés 0.2.2 FR et EN.
  7. Mise à jour Schema.org : `softwareVersion: "0.2.2"` synchronisé sur l'accueil FR et EN.
- Résultat : 100% des anomalies techniques signalées par le crawl Screaming Frog sont résolues dans le code source.

### 2026-10-22 — Déploiement Page Pilier & Soumission GSC

- Fait par les agents `Content-Semantic-Strategist` & `Tech-SEO-Auditor` :
  1. Création de la page pilier `/comparatif/alternatives-copilot-windows-11.html` ciblant le mot-clé transactionnel « alternative copilot windows 11 » : synthèse GEO, tableau comparatif multi-critères, guide de désactivation de Copilot, balisage Schema.org (`TechArticle`, `BreadcrumbList`, `FAQPage`).
  2. Maillage interne enrichi dans le comparatif existant et le sitemap.
  3. Déploiement en production sur Vercel (`https://jimy-agent.me`).
  4. Soumission officielle du sitemap mis à jour (`sitemap.xml`) via l'API Google Search Console (`sc-domain:jimy-agent.me`).
  5. Inspection d'URL GSC effectuée : accueil validé (PASS, INDEXING_ALLOWED), nouvelle page soumise pour inclusion dans le prochain crawl.

### 2026-10-28 — Release v0.2.4 : articles FR/EN et nouvelle soumission GSC
- **Agent** : Claude Code
- **Actions** :
  1. Articles `/nouveautes/jimy-0-2-4.html` et `/en/news/jimy-0-2-4.html` (l'âme qui apprend des corrections, récapitulatif de la 0.2.3) : `BlogPosting`, hreflang FR/EN/x-default, maillage vers les articles 0.2.2.
  2. Accueils FR/EN : bandeau et lien « Nouveautés 0.2.4 », `softwareVersion` 0.2.4. Sitemap : 2 URL ajoutées (13 au total), `lastmod` des accueils au 2026-10-28.
  3. Déploiement Vercel (`vercel --prod`), pages vérifiées en ligne (200), fichiers internes toujours en 404.
  4. Sitemap resoumis via l'API GSC (`sc-domain:jimy-agent.me`, 2026-10-28 15:20 UTC) : en attente, 0 erreur, 0 avertissement.
  5. Inspection des deux articles : « URL is unknown to Google » (normal le jour de la publication) ; l'indexation se demande à la main depuis l'interface GSC, l'API ne le permet pas.

### 2026-11-24 — Audit GSC/GA4 par compte de service et gains rapides
- **Agent** : Claude Code
- **Mesures (1/09 → 23/11)** : GSC 88 impressions, 2 clics, position 7,5, uniquement sur la marque (« jimy » position 6,4 : marque pas encore sécurisée). 10 URL sur 11 indexées (inspection) ; `/comparatif/alternatives-copilot-windows-11.html` en « Explorée, actuellement non indexée ». GA4 (désormais lisible via le compte de service `nana-analytics-agent`) : 55 utilisateurs, 109 sessions, dont ~46 robots (HK/Chine) ; 4 `download_installer`.
- **Fait** : redirections 308 (www → apex, jimy-agent-site.vercel.app → apex sauf /api/, /index.html → /, /en et /en/index.html → /en/) ; meta descriptions 0.2.4 FR/EN raccourcies ; `sameAs` GitHub dans le schema des accueils ; lien pied de page vers le comparatif pilier (accueil + guide MCP) ; `logo-jimy.jpg` et `logo-nana-small.png` exclus du déploiement ; URL du dépôt GitHub corrigée (`jimmy-agent.me` → `jimy-agent.me`). Déployé et vérifié en prod.
- **Reste à la main** : demander l'indexation du comparatif pilier dans l'interface GSC ; filtre trafic interne GA4.

## Liens entrants en attente (repris de l'ancien suivi mensuel)

| Source | État | Délai attendu |
|---|---|---|
| awesome-ai-agents (PR #1781) | en attente de merge | inconnu |
| SaaSHub | en file d'attente | 1 à 12 semaines |
| AlternativeTo, Product Hunt, Show HN, Reddit | non soumis, connexion requise | — |


### 2026-11-24 — Audit, gains rapides, stratégie et dossier du bot
- Fait : audit GSC/GA4 via le compte de service (lecture GA4 désormais possible) ; gains
  rapides (redirections www/vercel.app/index.html/en, meta 0.2.4, sameAs, lien vers le
  comparatif pilier, images inutiles exclues, URL du dépôt GitHub corrigée) ; marquage du
  trafic interne GA4 par `?interne=1` ; stratégie de mots-clés réécrite à partir de ~700
  suggestions Google réelles ; dossier `docs/seo/` (procédures, charte, calendrier, outils)
  pour le bot Hermes.
- Résultat : 88 impressions et 2 clics depuis le lancement, uniquement sur la marque ;
  10 URL sur 11 indexées ; comparatif pilier « Explorée, actuellement non indexée »
  (indexation redemandée par Jimmy le 24/11).
- Suite : tâche S00 (installation, `hermes/installation.md`), puis S01 le 30/11.

### 2026-11-24 — S00 Installation du bot SEO
- Fait : Vercel relié au dépôt GitHub (racine `site-jimy`, production `main`, aperçu de branche testé) ; clé de déploiement `jimy-seo-bot` (écriture) ; clone partiel sur le VPS `/home/jimmy/jimy-seo/jimmy-agent` ; clé Google `~/.secrets/jimy_seo_google.json` ; skill Hermes `jimy-seo` ; tâches planifiées jimy-seo-rapport (lun. 07:00 UTC), jimy-seo-production (lun. 08:00), jimy-seo-suivi (jeu. 08:00), jimy-seo-mensuel (1er, 07:00) ; hubs `/guides/` et `/en/guides/` + lien « Guides » sur les accueils.
- Résultat : outils testés sur le VPS (Search Console, calendrier, vérificateur, suggestions, périmètre).
- Suite : test Telegram `SEO ETAT` par Jimmy ; S01 produite le lundi 30/11 à 08:00 UTC.
