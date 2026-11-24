# Installation du bot SEO (tâche S00)

Faite par Claude Code avec Jimmy, **jamais par le bot**. Chaque étape a sa vérification ;
ne passer à la suivante que si elle est verte. Cocher dans ce fichier au fur et à mesure.

## 1. Dépôt GitHub à jour (PC de Jimmy)

- [ ] Pousser `main` (commits du site du 24/11 + dossier `docs/seo/`).
- Vérif : `git ls-remote origin main` = `git rev-parse main`.

## 2. Vercel relié au dépôt (Jimmy, tableau de bord Vercel)

Projet `jimy-agent-site` :
- [ ] Settings → Git → Connect Git Repository → `Jimmyjoe13/jimmy-agent` (installer l'app
      Vercel sur ce dépôt si demandé). Branche de production : `main`.
- [ ] Settings → Build and Deployment → Root Directory : `site-jimy`.
- [x] `site-jimy/vercel.json` contient `"ignoreCommand": "git diff --quiet HEAD^ HEAD -- ."` (ajouté le 24/11) :
      un commit qui ne touche pas `site-jimy/` (appli Rust, `docs/`) ne redéploie pas le site.
- [ ] Deployment Protection : laisser « Vercel Authentication » sur les aperçus ; Jimmy doit
      être connecté à Vercel sur son téléphone pour ouvrir les liens d'aperçu.
- Vérif : pousser une branche de test qui modifie un commentaire HTML → un aperçu apparaît
  (`python3 docs/seo/outils/apercu_vercel.py <branche>` renvoie une URL) ; supprimer la branche.
- **Changement d'habitude pour Jimmy** : ne plus publier avec `vercel --prod` depuis le PC.
  Publier = pousser sur `main`. Avant de modifier le site sur le PC : `git pull` (le bot
  pousse aussi sur `main`).

## 3. Accès git du bot (VPS + PC)

Sur le VPS :
```bash
ssh-keygen -t ed25519 -N "" -C jimy-seo-bot -f ~/.ssh/jimy_seo_deploy
cat >> ~/.ssh/config <<'EOF'

Host github-jimy-seo
  HostName github.com
  User git
  IdentityFile ~/.ssh/jimy_seo_deploy
  IdentitiesOnly yes
EOF
```
Sur le PC (gh connecté au compte Jimmyjoe13), ajouter la clé publique en clé de déploiement
**avec écriture** :
```bash
gh repo deploy-key add jimy_seo_deploy.pub --allow-write -R Jimmyjoe13/jimmy-agent -t jimy-seo-bot
```
Sur le VPS, clone partiel limité au site et à la doc SEO :
```bash
mkdir -p /home/jimmy/jimy-seo && cd /home/jimmy/jimy-seo
git clone --filter=blob:none --sparse git@github-jimy-seo:Jimmyjoe13/jimmy-agent.git
cd jimmy-agent && git sparse-checkout set site-jimy docs/seo
git config user.name "Jimy SEO Bot"
git config user.email "jimy-seo-bot@users.noreply.github.com"
```
- Vérif : `git push --dry-run origin main` sans erreur d'authentification.

## 4. Clé Google

```bash
cp /home/jimmy/richard/config/analytics-credentials.json ~/.secrets/jimy_seo_google.json
chmod 600 ~/.secrets/jimy_seo_google.json
cd /home/jimmy/jimy-seo/jimmy-agent && python3 docs/seo/outils/seo_data.py inspecter https://jimy-agent.me/
```
- Vérif : la ligne affiche `PASS | Submitted and indexed`.
- `python3 -c "import yaml, googleapiclient, google.analytics.data_v1beta"` sans erreur.

## 5. Skill Hermes

```bash
mkdir -p ~/.hermes/skills/jimy-seo
cp /home/jimmy/jimy-seo/jimmy-agent/docs/seo/hermes/SKILL.md ~/.hermes/skills/jimy-seo/SKILL.md
```
- Vérif : envoyer `SEO ETAT` à Hermes sur Telegram → réponse au format M8. Si le skill n'est
  pas vu, redémarrer `hermes-gateway` **après avoir vérifié qu'aucune tâche planifiée ne tourne**.

## 6. Tâches planifiées (heures UTC)

```bash
H=/home/jimmy/.hermes/hermes-agent/venv/bin/hermes
W=/home/jimmy/jimy-seo/jimmy-agent/docs/seo
P="Tu es le bot SEO de jimy-agent.me. Suis docs/seo/AGENTS.md (injecté) et exécute"
$H cron create "0 7 * * 1" "$P la procédure P4, rapport hebdomadaire (--jours 7)." --name jimy-seo-rapport --deliver telegram --skill jimy-seo --workdir $W
$H cron create "0 8 * * 1" "$P la procédure P1 (produire la prochaine tâche de contenu)." --name jimy-seo-production --deliver telegram --skill jimy-seo --workdir $W
$H cron create "0 8 * * 4" "$P la procédure P5 (suivi : J+14, J+28, hors site, relances)." --name jimy-seo-suivi --deliver telegram --skill jimy-seo --workdir $W
$H cron create "0 7 1 * *" "$P la procédure P4, rapport mensuel (--jours 28)." --name jimy-seo-mensuel --deliver telegram --skill jimy-seo --workdir $W
$H cron list
```
- Vérif : `hermes cron run <id de jimy-seo-rapport>` → rapport M3 reçu sur Telegram.

## 7. Contenu de départ (Claude Code)

- [ ] Hubs `site-jimy/guides/index.html` et `site-jimy/en/guides/index.html` (liste des guides,
      sitemap, lien « Guides » dans la navigation et le pied de page des accueils).
- Vérif : `python3 docs/seo/outils/verifier.py site` sans ALERTE « hub absent ».

## 8. Clôture

```bash
python3 docs/seo/outils/calendrier.py statut S00 fait
```
puis commit d'administration (P8) et entrée dans `journal.md`.
