# 02 — Plan de contenu : briefs des tâches

Un brief par tâche du calendrier (`calendrier.yaml`, même identifiant). Le bot produit
**exactement** ce que dit le brief ; s'il pense qu'il faut s'en écarter, il le propose dans le
message de validation au lieu de le faire.

## Règles communes à toutes les tâches de contenu

- **Livrable** : une page FR + une page EN (gabarits `modeles/`), reliées par hreflang.
- **Mots-clés** : avant d'écrire, contrôler le mot-clé principal et 2 secondaires avec
  `outils/suggestions.py` (voir P1). Si le mot-clé principal ne renvoie **aucune** suggestion,
  choisir la variante suggérée la plus proche et le signaler.
- **title / H1** proposés ci-dessous : à garder sauf meilleure formulation de même longueur
  contenant le mot-clé principal.
- **Questions H2** : celles du brief, dans cet ordre, reformulables légèrement ; le tableau
  comparatif s'insère après la 2ᵉ question.
- **Maillage sortant** (dans « À lire aussi » ou dans le texte) : les pages listées, si elles
  sont publiées ; sinon les remplacer par le hub des guides.
- **Maillage entrant** (pages existantes à modifier dans la même branche) :
  1. le hub de la langue (`site-jimy/guides/index.html`, `site-jimy/en/guides/index.html`) : ajouter une carte ;
  2. au moins **une** page déjà publiée listée « entrant » : ajouter un lien **dans le texte**
     (une phrase utile, pas seulement le pied de page) ;
  3. `site-jimy/sitemap.xml` : ajouter les deux URL (`lastmod` = jour de publication prévu,
     `changefreq` monthly, `priority` 0.7).
  La page sœur (autre langue) compte comme 3ᵉ lien entrant via le bouton de langue.
- **Schema** : TechArticle + BreadcrumbList + FAQPage (gabarit). Ajouter `HowTo` seulement
  si une section est un pas-à-pas complet.

---

## S01 — Assistant IA comme Jarvis sur PC

- **Thème** : 1 (01-strategie-mots-cles.md §5). Priorité : la plus forte (concurrence faible, intention de téléchargement).
- **EN (page principale)** : `site-jimy/en/guides/jarvis-ai-assistant-for-pc.html`
  - title : `Jarvis AI Assistant for PC: Free Windows Download (2026)`
  - H1 : `A Jarvis-like AI assistant for your Windows PC (free)`
  - Mot-clé principal : `jarvis ai assistant for pc` · secondaires : jarvis for pc free download, jarvis for pc windows 11, how to get jarvis on pc, best jarvis like ai assistant, is there a jarvis app
  - Questions H2 : Is there a real Jarvis app for Windows? · How do I get a Jarvis-like assistant on my PC for free? · What can a Jarvis-like AI actually do on Windows today? · How do I set up Jimy as my Jarvis in 3 minutes? · Does a Jarvis-like assistant listen all the time?
- **FR** : `site-jimy/guides/assistant-ia-comme-jarvis-pc.html`
  - title : `Un assistant IA comme Jarvis sur PC Windows (gratuit)`
  - H1 : `Avoir un Jarvis sur son PC Windows : l'assistant IA vocal gratuit`
  - Mot-clé principal : `jarvis pc` · secondaires : assistant comme jarvis, comment avoir jarvis sur pc, jarvis ai pc
  - Questions H2 : Existe-t-il un vrai Jarvis pour PC ? · Comment avoir un Jarvis sur son PC gratuitement ? · Que peut faire un assistant comme Jarvis sur Windows aujourd'hui ? · Comment installer Jimy en 3 minutes ? · Est-ce qu'il m'écoute en permanence ?
- **Angle** : le rêve « Jarvis » (parler à son PC, qu'il agisse et réponde) existe enfin pour le
  grand public ; les projets Jarvis trouvés en ligne demandent souvent Python ou sont des
  démos ; Jimy s'installe en un `.exe`. Rappeler : pas de lien avec Marvel.
- **Tableau** : « Jimy face aux autres "Jarvis" pour PC » — colonnes : Jimy · projets Jarvis open source (GitHub, Python) · applis Jarvis amateurs (itch.io) · Copilot (Hey Copilot). Critères : installation, voix, agit sur le PC, où va la voix, gratuit, open source.
- **Maillage sortant** : accueil (`/#fonctions` ou `/en/#features`), S02 (assistant vocal), S06 (agent qui contrôle le PC).
- **Maillage entrant** : hubs + page sœur (aucune page connexe n'est encore publiée ; les accueils ne se modifient pas, 05-garde-fous §3). Les tâches suivantes ajouteront des liens vers S01.

## S02 — Assistant vocal Windows 11 et remplaçant de Cortana

- **FR (principale)** : `site-jimy/guides/assistant-vocal-windows-11.html`
  - title : `Assistant vocal Windows 11 : contrôler son PC à la voix`
  - H1 : `Assistant vocal sur Windows 11 : parler à son PC et qu'il agisse`
  - Mot-clé principal : `assistant vocal windows 11` · secondaires : commande vocale windows 11, assistant vocal pc, comment contrôler son ordinateur par la voix, parler à son ordinateur
  - Questions H2 : Quel assistant vocal utiliser sur Windows 11 depuis la fin de Cortana ? · Comment contrôler son ordinateur par la voix ? · Comment parler à son ordinateur et qu'il réponde ? · Ma voix part-elle sur Internet ? · Comment installer un assistant vocal IA sur Windows 11 ?
- **EN** : `site-jimy/en/guides/voice-assistant-windows-11.html`
  - title : `Voice Assistant for Windows 11: Control Your PC by Voice`
  - H1 : `The best voice assistant for Windows 11 now that Cortana is gone`
  - Mot-clé principal : `voice assistant windows 11` · secondaires : voice assistant for pc, control pc with voice, cortana replacement windows 11, can i talk to my computer
  - Questions H2 : What replaced Cortana on Windows 11? · Is there a voice assistant for Windows 11? · Can I control my PC with my voice? · Does my voice leave my PC? · How do I install an AI voice assistant on Windows 11?
- **Angle** : Cortana a disparu ; Windows propose l'Accès vocal (commandes) et « Hey Copilot » (voix traitée en ligne) ; Jimy ajoute un assistant qui agit et dont l'écoute reste locale. Faits Microsoft sourcés (support.microsoft.com).
- **Tableau** : Accès vocal Windows · Hey Copilot · Jimy. Critères : comprend des phrases libres, agit sur fichiers/navigateur, répond à voix haute, où va la voix, gratuit.
- **Maillage sortant** : S01, S04 (comparatif Copilot), confidentialité (`/confidentialite.html`).
- **Maillage entrant** : hubs ; S01 (lien dans la section sur l'écoute).

## S03 — Compagnon IA de bureau (AI desktop pet)

- **EN (principale)** : `site-jimy/en/guides/ai-desktop-pet-windows.html`
  - title : `AI Desktop Pet for Windows: a 3D Companion That Helps`
  - H1 : `An AI desktop pet that actually does things on your PC`
  - Mot-clé principal : `ai desktop pet` · secondaires : desktop companion ai, best ai desktop companion, ai desktop pet windows free, clippy ai
  - Questions H2 : What is an AI desktop pet? · What is the best AI desktop companion for Windows? · Is there a Clippy with real AI? · Is a desktop companion AI safe? · How do I install a free AI desktop pet?
- **FR** : `site-jimy/guides/compagnon-ia-bureau-windows.html`
  - title : `Compagnon IA pour le bureau Windows : un avatar 3D utile`
  - H1 : `Un compagnon IA sur ton bureau Windows, qui t'aide vraiment`
  - Mot-clé principal : `compagnon virtuel pc` · secondaires : assistant virtuel pc, personnage animé bureau windows, desktop pet, clippy ia
  - Questions H2 : Qu'est-ce qu'un compagnon IA de bureau ? · Quel est le meilleur compagnon IA pour Windows ? · Existe-t-il un Clippy avec une vraie IA ? · Un compagnon de bureau IA est-il sûr ? · Comment installer un compagnon IA gratuit ?
- **Angle** : les « desktop pets » sont mignons mais décoratifs ; Jimy est un compagnon 3D qui agit (fichiers, navigateur) avec des garde-fous. Montrer les 8 skins (images `assets/skins/*.webp`).
- **Tableau** : Jimy · compagnons Live2D/VRM (Steam) · desktop pets classiques · Clippy (historique). Critères : 3D/2D, parle, agit sur le PC, modèle local possible, prix, open source.
- **Maillage sortant** : S01, accueil `/#skins` / `/en/#skins`, S09 (si publiée).
- **Maillage entrant** : hubs ; S01.

## S04 — Fusion des deux comparatifs Copilot + version anglaise

- **Type particulier** : consolidation. **Nécessite `--vercel-json`** (redirection) : à signaler en tête du message de validation.
- **FR (conservée)** : `site-jimy/comparatif/alternatives-copilot-windows-11.html`, réécrite au gabarit en reprenant le meilleur de `site-jimy/comparatif/agent-ia-windows-vs-copilot.html`.
  - title : `Alternatives à Copilot sur Windows 11 : le comparatif 2026`
  - H1 : `Les meilleures alternatives à Copilot sur Windows 11`
  - Mot-clé principal : `alternative copilot windows 11` · secondaires : remplacer copilot, alternative copilot gratuite, copilot gratuit pour pc, désactiver copilot
  - Questions H2 : Pourquoi chercher une alternative à Copilot ? · Quelles sont les meilleures alternatives à Copilot sur Windows 11 ? · Existe-t-il une alternative gratuite à Copilot ? · Copilot, ChatGPT, Claude ou Jimy : lequel agit vraiment sur le PC ? · Peut-on désactiver Copilot et le remplacer ?
- **EN (nouvelle)** : `site-jimy/en/compare/copilot-alternatives-windows-11.html`
  - title : `Copilot Alternatives for Windows 11: 2026 Comparison`
  - H1 : `The best Copilot alternatives for Windows 11`
  - Mot-clé principal : `copilot alternative windows 11` · secondaires : is there a free alternative to copilot, better than copilot, replace copilot windows
  - Questions H2 : Why look for a Copilot alternative? · What are the best Copilot alternatives on Windows 11? · Is there a free alternative to Copilot? · Copilot, ChatGPT, Claude or Jimy: which one can act on your PC? · Can you disable Copilot and replace it?
  - Exclure l'intention développeur (VS Code, GitHub Copilot) : une phrase qui la renvoie ailleurs suffit.
- **Opérations** :
  1. Redirection permanente dans `site-jimy/vercel.json` (tableau `redirects`) : `/comparatif/agent-ia-windows-vs-copilot.html` → `/comparatif/alternatives-copilot-windows-11.html`.
  2. Supprimer `site-jimy/comparatif/agent-ia-windows-vs-copilot.html` (`git rm`).
  3. Remplacer **tous** les liens vers l'ancienne URL dans le site (`grep -rl agent-ia-windows-vs-copilot site-jimy`).
  4. Retirer l'ancienne URL du sitemap, ajouter l'URL EN.
  5. `outils/verifier.py site` doit passer (contrôle « lien vers une URL redirigée »).
- **Tableau** : Copilot · ChatGPT (appli Windows) · Claude (appli) · Jimy. Critères : agit sur le PC, voix, où vont les données, prix de base, open source.
- **Maillage entrant** : hubs ; S02 ; S05 (si publiée).

## S05 — Quelle IA installer sur son PC

- **FR (principale)** : `site-jimy/guides/quelle-ia-pour-pc.html`
  - title : `Quelle IA installer sur son PC ? Le guide gratuit 2026`
  - H1 : `Quelle IA installer sur son PC Windows, gratuitement ?`
  - Mot-clé principal : `ia pour pc gratuit` · secondaires : quelle ia pour mon pc, meilleur ia gratuit pour pc, logiciel ia gratuit pc, télécharger ia pour pc
  - Questions H2 : Quelle est la meilleure IA à installer sur son PC ? · Peut-on avoir une IA gratuite sur PC ? · Appli ChatGPT, Copilot ou agent IA : quelle différence ? · Peut-on parler à une IA à la voix sur PC ? · Faut-il un PC puissant pour une IA ?
- **EN** : `site-jimy/en/guides/best-free-ai-assistant-for-pc.html`
  - title : `Best Free AI Assistant for PC (Windows 10 & 11, 2026)`
  - H1 : `The best free AI assistant for your Windows PC`
  - Mot-clé principal : `best free ai assistant for pc` · secondaires : free ai assistant for windows 11, is there an ai assistant for windows 11, chatgpt desktop app windows, chatgpt voice on pc
  - Questions H2 : What is the best free AI assistant for PC? · Is there a free AI assistant for Windows 11? · ChatGPT app, Copilot or an AI agent: what's the difference? · Can I talk to an AI by voice on my PC? · Do I need a powerful PC to run AI?
- **Angle** : trois familles (chatbot dans une fenêtre, assistant intégré à Windows, agent qui agit) ; aider à choisir selon le besoin ; Jimy pour qui veut parler et faire agir. Ne pas viser « chatgpt sur pc » de front.
- **Tableau** : ChatGPT (appli) · Copilot · Jimy · IA locale seule (LM Studio). Critères : gratuit, voix, agit sur le PC, hors ligne possible, compte requis.
- **Maillage sortant** : S04, S06 (si publiée), S07 (si publiée).
- **Maillage entrant** : hubs ; S02.

## S06 — Agent IA qui contrôle le PC

- **FR (principale)** : `site-jimy/guides/agent-ia-controle-pc.html`
  - title : `Agent IA qui contrôle le PC : guide simple et gratuit`
  - H1 : `Un agent IA qui contrôle ton PC : à quoi ça sert, comment l'installer`
  - Mot-clé principal : `agent ia pc` · secondaires : agent ia controle pc, agent ia gratuit, comment installer un agent ia sur son pc, ia qui contrôle mon pc
  - Questions H2 : Une IA peut-elle vraiment contrôler mon ordinateur ? · Qu'est-ce qu'un agent IA, et en quoi est-ce différent de ChatGPT ? · Quel est le meilleur agent IA gratuit pour Windows ? · Est-ce dangereux de laisser une IA agir sur son PC ? · Comment installer un agent IA sur son PC ?
- **EN** : `site-jimy/en/guides/ai-that-controls-your-computer.html`
  - title : `AI That Controls Your Computer: Free Agent for Windows`
  - H1 : `An AI that controls your computer: what it does and how to try it`
  - Mot-clé principal : `ai that controls your computer` · secondaires : ai agent for pc free, can chatgpt control my pc, best ai agent for windows 11, is there an ai that controls your computer
  - Questions H2 : Is there an AI that controls your computer? · What is an AI agent, and how is it different from ChatGPT? · What is the best free AI agent for Windows? · Is it safe to let an AI act on your PC? · How do I install an AI agent on my PC?
- **Angle** : 5 exemples concrets de tâches (ranger les téléchargements, comparer des devis PDF, remplir un formulaire, chercher un fichier, résumer sa journée) ; sécurité = cartes d'autorisation.
- **Tableau** : Jimy · agents « computer use » en ligne · scripts d'automatisation (Power Automate) · ChatGPT. Critères : agit sur le PC, voix, demande l'autorisation, local, prix.
- **Maillage sortant** : S01, S05, S11 (si publiée).
- **Maillage entrant** : hubs ; S01 ; S05.

## S07 — IA locale gratuite avec LM Studio

- **FR (principale)** : `site-jimy/guides/ia-locale-gratuite-pc-lm-studio.html`
  - title : `IA locale gratuite sur PC : Jimy + LM Studio pas à pas`
  - H1 : `Une IA locale et gratuite sur ton PC : Jimy avec LM Studio`
  - Mot-clé principal : `ia locale gratuite` · secondaires : ia hors ligne pc, chatgpt hors ligne, ia sans abonnement, quelle ia respecte la vie privée
  - Questions H2 : Peut-on utiliser une IA sans abonnement ni Internet ? · Quel PC faut-il pour faire tourner une IA en local ? · Comment brancher LM Studio (ou Ollama) sur Jimy ? · Une IA locale est-elle aussi bonne que ChatGPT ? · Quelle IA respecte vraiment la vie privée ?
- **EN** : `site-jimy/en/guides/free-local-ai-assistant-pc.html`
  - title : `Free Local AI Assistant for PC: Jimy + LM Studio Setup`
  - H1 : `A free local AI assistant on your PC: Jimy with LM Studio`
  - Mot-clé principal : `local ai assistant windows` · secondaires : offline ai assistant for pc, free local ai assistant, private ai assistant, ai assistant without subscription
  - Questions H2 : Can you use an AI assistant without a subscription or internet? · What PC do you need to run AI locally? · How do I connect LM Studio (or Ollama) to Jimy? · Is a local AI as good as ChatGPT? · What is the most private AI assistant?
- **Pas-à-pas** (schema `HowTo`) : installer LM Studio, télécharger un modèle, démarrer le serveur local (adresse par défaut `http://127.0.0.1:1234/v1`), ajouter dans Jimy un fournisseur compatible OpenAI avec cette adresse. Les libellés exacts de l'interface de Jimy sont **à vérifier dans le code** (`agent/src/config.rs`, `desktop/`) et à signaler à Jimmy s'ils sont incertains. Ollama : adresse `http://127.0.0.1:11434/v1`.
- **Honnêteté** : un PC modeste fera tourner de petits modèles, plus lents et moins capables ; la synthèse vocale en ligne enverra du texte, sauf moteur Windows local.
- **Tableau** : LM Studio + Jimy · Ollama + Jimy · ChatGPT · Copilot. Critères : gratuit, hors ligne, données sur le PC, voix, difficulté d'installation.
- **Maillage sortant** : S05, confidentialité, guide MCP (`/guides/serveurs-mcp-windows-local.html`, FR seulement).
- **Maillage entrant** : hubs ; S05 ; S02.

## S08 — Désactiver Copilot sur Windows 11

- **FR (principale)** : `site-jimy/guides/desactiver-copilot-windows-11.html`
  - title : `Désactiver Copilot sur Windows 11 : le guide complet 2026`
  - H1 : `Comment désactiver Copilot sur Windows 11 (complètement)`
  - Mot-clé principal : `désactiver copilot windows 11` · secondaires : comment désactiver copilot complètement, peut-on désinstaller copilot, désactiver copilot au démarrage, désactiver la touche copilot
  - Questions H2 : Peut-on désactiver Copilot sur Windows 11 ? · Comment désactiver Copilot dans les paramètres ? · Peut-on désinstaller complètement Copilot ? · Comment désactiver la touche Copilot du clavier ? · Par quoi remplacer Copilot ?
- **EN** : `site-jimy/en/guides/disable-copilot-windows-11.html`
  - title : `How to Disable Copilot on Windows 11 Completely (2026)`
  - H1 : `How to disable Copilot on Windows 11, completely`
  - Mot-clé principal : `disable copilot windows 11` · secondaires : how to disable copilot windows 11 completely, can you uninstall copilot, disable copilot key, disable copilot windows 11 home
  - Questions H2 : Can you disable Copilot on Windows 11? · How do I turn off Copilot in Settings? · Can you uninstall Copilot completely? · How do I disable the Copilot key? · What can replace Copilot?
- **Exactitude** : chaque méthode (Paramètres, désinstallation, stratégie de groupe Pro, registre Home, touche) doit être **sourcée Microsoft** ou presse tech de moins de 6 mois ; indiquer l'édition de Windows concernée. Avertissement avant toute manipulation du registre.
- **Angle** : utile d'abord (le lecteur veut désactiver), Jimy seulement dans la dernière question et le CTA.
- **Maillage sortant** : S04 (alternatives), S02.
- **Maillage entrant** : hubs ; S04 (remplacer la section « désactiver » courte par un lien vers ce guide).

## S09 — Clippy IA

- **EN (principale)** : `site-jimy/en/guides/clippy-ai.html`
  - title : `Clippy AI: Is There a Clippy With Real AI in 2026?`
  - H1 : `Is there a Clippy with real AI? The best Clippy-like assistants`
  - Mot-clé principal : `clippy ai` · secondaires : is there a clippy ai, clippy ai assistant, clippy alternatives, where is clippy now
  - Questions H2 : Is there a Clippy AI? · Where is Clippy now? · What are the best Clippy alternatives with AI? · Can a Clippy-like assistant do things on your PC? · How do I get a Clippy-like assistant on Windows?
- **FR** : `site-jimy/guides/clippy-ia.html`
  - title : `Clippy IA : le trombone de Microsoft revient-il avec l'IA ?`
  - H1 : `Clippy avec une vraie IA : ça existe ?`
  - Mot-clé principal : `clippy ia` · secondaires : clippy ai, alternative à clippy, assistant comme clippy
  - Questions H2 : Existe-t-il un Clippy avec une vraie IA ? · Qu'est devenu Clippy ? · Quelles alternatives à Clippy avec de l'IA ? · Un assistant façon Clippy peut-il agir sur le PC ? · Comment avoir un assistant façon Clippy sur Windows ?
- **Marque** : Clippy appartient à Microsoft : pas d'image, « assistant façon Clippy ».
- **Maillage sortant** : S03, S01. **Entrant** : hubs ; S03.

## S10 — Mode vocal ChatGPT sur PC

- **FR (principale)** : `site-jimy/guides/mode-vocal-chatgpt-pc.html`
  - title : `Mode vocal ChatGPT sur PC : mode d'emploi et alternatives`
  - H1 : `Parler à ChatGPT sur PC : le mode vocal et ses alternatives`
  - Mot-clé principal : `mode vocal chatgpt pc` · secondaires : chatgpt vocal pc, parler à chatgpt sur pc, chatgpt audio pc
  - Questions H2 : Peut-on utiliser le mode vocal de ChatGPT sur PC ? · Pourquoi le mode vocal de ChatGPT ne marche pas sur mon PC ? · ChatGPT peut-il agir sur mon ordinateur ? · Quelle alternative vocale à ChatGPT sur Windows ? · Ma voix est-elle envoyée sur Internet ?
- **EN** : `site-jimy/en/guides/chatgpt-voice-on-pc.html`
  - title : `ChatGPT Voice on PC: How It Works and Better Options`
  - H1 : `How to use ChatGPT voice on PC, and the alternatives`
  - Mot-clé principal : `chatgpt voice on pc` · secondaires : chatgpt voice mode not working pc, how to talk to chatgpt on pc, can chatgpt control my pc
  - Questions H2 : Can you use ChatGPT voice mode on PC? · Why is ChatGPT voice mode not working on my PC? · Can ChatGPT control my computer? · What is a good voice alternative to ChatGPT on Windows? · Is my voice sent to the internet?
- **Exactitude** : fonctionnement du mode vocal ChatGPT sur Windows sourcé (aide OpenAI) le jour même.
- **Maillage sortant** : S02, S05, S06. **Entrant** : hubs ; S05.

## S11 — Une IA qui range les fichiers

- **EN (principale)** : `site-jimy/en/guides/ai-that-organizes-files.html`
  - title : `AI That Organizes Your Files on Windows (Free, by Voice)`
  - H1 : `An AI that organizes your files on Windows, just by asking`
  - Mot-clé principal : `ai to organize files` · secondaires : can ai organize my files, best ai to organize files, ai to organize files on computer
  - Questions H2 : Can AI organize my files? · What is the best AI to organize files on Windows? · How do I ask an AI to clean my Downloads folder? · Is it safe to let an AI move my files? · How do I install it?
- **FR** : `site-jimy/guides/ia-qui-range-mes-fichiers.html` — mot-clé FR **non confirmé** par l'autocomplétion le 24/11 : lancer `suggestions.py "ranger ses fichiers" --questions` et `suggestions.py "organiser ses fichiers ia"`, choisir la formulation suggérée la plus proche, adapter title/H1 et le signaler.
  - Questions H2 (à ajuster) : Une IA peut-elle ranger mes fichiers ? · Comment ranger son dossier Téléchargements avec une IA ? · Est-ce risqué de laisser une IA déplacer mes fichiers ? · Quelle IA gratuite pour organiser ses fichiers ? · Comment l'installer ?
- **Maillage sortant** : S06, S01. **Entrant** : hubs ; S06.

## S12 — Revue trimestrielle (type revue)

Pas de page. Le bot :
1. lance `outils/seo_data.py rapport --jours 90` et `page` sur chaque URL publiée ;
2. relit `backlog.md` et les déclencheurs R3 des rapports ;
3. propose à Jimmy les 12 tâches suivantes (même format que ce fichier : un brief par tâche,
   priorisé selon 01-strategie §4 et les données réelles) dans `rapports/revue-AAAA-MM.md`
   sur une branche `seo/S12` ;
4. après « SEO OK S12 », ajoute les briefs ici et les tâches dans `calendrier.yaml` (nouveaux
   identifiants S13…S24, H06…) — c'est la seule fois où le bot modifie ce fichier.

## Tâches hors site (H01 à H05)

Le bot prépare, Jimmy publie (05-garde-fous §2). Livrable : `rapports/<ID>-brouillon.md`
sur la branche `seo/<ID>`, envoyé sur Telegram.

- **H01** — Relance PR `e2b-dev/awesome-ai-agents#1781` : vérifier son état (`gh` ou API
  GitHub publique) ; si toujours ouverte, rédiger un commentaire de relance court et poli (EN).
  Vérifier si Jimy apparaît sur saashub.com ; si non, le signaler.
- **H02** — AlternativeTo : rédiger la fiche (nom, description courte et longue EN, tags,
  plateformes, licence, « alternative to » : Microsoft Copilot, Cortana, Clippy, Desktop Mate).
- **H03** — itch.io : rédiger la page (titre, accroche, description EN, tags : desktop-pet,
  virtual-assistant, ai, windows, open-source ; type « Tool » ; lien de téléchargement GitHub ;
  captures = images existantes de `assets/skins`). Gratuit, « pay what you want » optionnel :
  question à poser à Jimmy.
- **H04** — Reddit r/LocalLLaMA : lire les règles du sous-forum (auto-promotion), rédiger un
  post de retour d'expérience (EN) centré sur LM Studio + voix locale, pas une pub ; proposer
  le jour et l'heure de publication (mardi-jeudi, 14-16 h UTC).
- **H05** — Product Hunt + Show HN : **ne rien préparer tant que l'installeur n'est pas signé**
  (SignPath) ; vérifier l'état de la signature, et si signé, rédiger les deux textes.
