# 06 — Mesure, objectifs et déclencheurs

## 1. Accès aux données

- Compte de service `nana-analytics-agent@n8n-ai-agent-451510.iam.gserviceaccount.com` :
  propriétaire de la Search Console `sc-domain:jimy-agent.me`, lecteur GA4 `properties/558505422`
  (flux `G-FK2NSSHQWM`).
- Clé : `~/.secrets/jimy_seo_google.json` sur le VPS (jamais dans le dépôt ; copie locale de
  Jimmy dans `site-jimy/analytics-credentials/`, ignorée par git et Vercel).
- Tout passe par `outils/seo_data.py`. La Search Console a ~3 jours de retard.
- **Fiabilité GA4** : trafic robot connu (Hong Kong / Chine, langue chinoise) et visites de
  Jimmy (exclues depuis le 24/11/2026 grâce à `?interne=1` + filtre « Internal Traffic »).
  La référence pour l'audience organique est la **Search Console** ; GA4 sert aux
  téléchargements (`download_installer`).

## 2. Indicateurs suivis

| Indicateur | Source | Pourquoi |
|---|---|---|
| Impressions et clics **hors marque** | Search Console | Seul signal de la stratégie (la marque « jimy » ne dépend pas de nous) |
| Position moyenne des requêtes ciblées | Search Console | Progression page par page |
| Pages indexées / publiées | Inspection d'URL | Une page non indexée ne rapporte rien |
| `download_installer` par canal et page d'arrivée | GA4 | **Objectif final** : téléchargements |
| Sessions organiques par page d'arrivée | GA4 | Quelles pages amènent du monde |

## 3. Objectifs (proposition au 24/11/2026, à confirmer ou ajuster par Jimmy)

Point de départ (25/10 → 21/11) : 2 impressions hors marque, 0 clic hors marque, ~3
téléchargements en deux mois et demi.

| Échéance | Impressions hors marque / 28 j | Clics hors marque / 28 j | Téléchargements organiques / 28 j |
|---|---|---|---|
| Fin janvier 2027 | 300 | 5 | 3 |
| Fin mars 2027 | 1 500 | 30 | 10 |
| Fin juin 2027 | 5 000 | 120 | 30 |

Ces cibles sont des hypothèses, pas des promesses : un site neuf sans liens entrants progresse
lentement. La revue S12 les recale sur les données réelles.

## 4. Déclencheurs (appliqués par P4 et P5)

Calculés par `seo_data.py` ; le bot n'agit **jamais directement** sur une page publiée : il
ajoute une entrée à `backlog.md` et la mentionne dans le message.

| Code | Condition (fenêtre 28 j sauf mention) | Action proposée |
|---|---|---|
| R1 | Page : ≥ 100 impressions et CTR < 1,5 % | Nouveau title + meta description (2 propositions) |
| R2 | Requête d'une page : ≥ 30 impressions et position 8-20 | Ajouter une section / un H2 qui répond à cette requête |
| R3 | Requête hors marque : ≥ 20 impressions (rapport 7 j) | Vérifier qu'une page la cible ; sinon candidate au backlog (nouvelle page) |
| R4 | Page publiée depuis 28 j : 0 impression | Contrôler indexation, maillage entrant, mot-clé (`suggestions.py`) |
| R5 | Page : ≥ 20 sessions organiques et 0 téléchargement | Revoir l'appel à l'action et la réponse à l'intention |
| J14 | Page publiée depuis 14 j non indexée | Demander à Jimmy « Demander une indexation » + vérifier le maillage |
| A1 | Impressions totales -50 % d'une semaine à l'autre (≥ 50 avant) | Alerte en tête du rapport, aucune action seule |

## 5. Calendrier des mesures

- Lundi 07:00 UTC : rapport hebdomadaire (P4, `--jours 7`).
- Le 1er du mois 07:00 UTC : rapport mensuel (P4, `--jours 28`) → `rapports/AAAA-MM.md` + journal.
- Jeudi 08:00 UTC : suivi (P5) — contrôles J+14, mesures J+28, relances.
- Semaine S12 : revue trimestrielle (`--jours 90`).
