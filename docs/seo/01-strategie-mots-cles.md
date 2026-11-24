# Stratégie de mots-clés — jimy-agent.me

Version du 24 novembre 2026, validée par Jimmy. Remplace la version du
22 octobre, dont les volumes de recherche (« 1 200/mois », « +250 % »)
n'avaient aucune source.

## 1. Cadre

- **Objectif unique** : le téléchargement de Jimy (événement GA4 `download_installer`).
  Un mot-clé se juge sur sa proximité avec le téléchargement, pas sur son volume.
- **Cible** : grand public Windows. Le profil technique (MCP, VS Code, développeurs)
  n'est plus prioritaire : on garde le guide MCP existant, sans contenu technique nouveau.
- **Langues** : FR et EN. La Search Console montre déjà plus d'impressions sur `/en/`
  (43) que sur l'accueil FR (26).
- **Requêtes conversationnelles** : avec l'IA (Google AI Overviews, ChatGPT, Perplexity,
  Gemini), les gens posent des questions. Chaque thème a donc ses mots-clés têtes
  et sa longue traîne sous forme de questions.

## 2. Contraintes d'honnêteté (le discours doit rester vrai)

- Jimy a besoin d'un modèle de langage. Deux façons de l'avoir gratuitement :
  un **modèle local** (LM Studio, Ollama ou tout serveur compatible OpenAI, branché
  comme fournisseur personnalisé) ou un modèle gratuit chez un fournisseur. Sinon,
  c'est le fournisseur (OpenRouter, OpenCode Go…) qui facture l'usage.
- La reconnaissance vocale (whisper.cpp) est toujours locale. Le « 100 % hors ligne »
  n'est vrai **qu'avec un modèle local** : le dire, avec la condition (PC assez puissant).
- « Jarvis » est une marque Marvel : écrire « un assistant comme Jarvis » ou
  « Jarvis-like », jamais de logo, jamais « Jarvis officiel ».

## 3. Méthode et limites

- Collecte : environ 700 suggestions de l'autocomplétion Google (FR `gl=fr`, EN `gl=us`),
  à partir de graines par thème et de modificateurs de question (comment, quel,
  meilleur, existe-t-il / how to, best, is there, can, free). Une suggestion prouve
  qu'une requête est réellement tapée, **pas son volume**.
- Concurrence : lecture de la première page de résultats sur les requêtes clés (24/11/2026).
- Aucun volume chiffré dans ce document. Pour en avoir : Google Ads Keyword Planner
  (gratuit avec un compte Ads) ou un outil payant (Haloscan, Semrush, Ahrefs).

## 4. Les 7 thèmes, par priorité

| # | Thème | Langue | Proximité téléchargement | Concurrence observée | Page cible (proposée) |
|---|---|---|---|---|---|
| 1 | Un assistant « comme Jarvis » sur PC | EN puis FR | forte (« free download ») | **faible** : projets amateurs itch.io / PyPI, fiche Windows Report de 2020 | `/en/guides/jarvis-ai-assistant-for-pc.html`, `/guides/assistant-ia-comme-jarvis-pc.html` |
| 2 | Assistant vocal Windows 11 / parler à son PC / remplacer Cortana | FR + EN | moyenne | moyenne en FR (Clubic, Fnac sur « Hey Copilot »), faible sur « cortana replacement » (articles 2023) | `/guides/assistant-vocal-windows-11.html`, `/en/guides/voice-assistant-windows-11.html` |
| 3 | Compagnon IA sur le bureau (« AI desktop pet ») | EN | forte | faible à moyenne : fiches Steam / itch.io, peu d'articles | `/en/guides/ai-desktop-pet-windows.html` |
| 4 | Quelle IA pour mon PC / ChatGPT à la voix sur PC | FR surtout | moyenne | **forte** sur « chatgpt sur pc », faible sur la longue traîne vocale | `/guides/quelle-ia-pour-pc.html`, `/en/guides/best-free-ai-assistant-for-pc.html` |
| 5 | Une IA qui agit sur mon PC | FR + EN | moyenne | moyenne : produits (Vy, EasyClaw), presse, GitHub | `/guides/agent-ia-controle-pc.html`, `/en/guides/ai-that-controls-your-computer.html` |
| 6 | IA locale et gratuite sur PC (LM Studio) | FR + EN | moyenne | moyenne, public plus averti | `/guides/ia-locale-gratuite-pc.html`, `/en/guides/free-local-ai-assistant-pc.html` |
| 7 | Copilot | FR + EN | faible (information) mais gros volume | forte | fusion des deux pages `/comparatif/` existantes (elles se cannibalisent) |

## 5. Mots-clés et questions par thème

### Thème 1 — Assistant comme Jarvis
- Têtes EN : jarvis for pc · jarvis ai assistant for pc free download · jarvis for pc windows 11 · jarvis ai assistant · best jarvis ai for pc · free jarvis ai for pc
- Têtes FR : jarvis pc · jarvis ai pc · assistant comme jarvis
- Questions : how to get jarvis on pc for free? · is there a jarvis app for windows? · what is the best jarvis-like ai assistant? · how to set up jarvis on pc? · how to download jarvis for windows 11? · comment avoir jarvis sur pc ? · existe-t-il un assistant comme jarvis sur pc ?

### Thème 2 — Assistant vocal Windows
- Têtes FR : assistant vocal windows 11 · assistant vocal pc · commande vocale windows 11 · commande vocale pc · parler à son ordinateur
- Têtes EN : voice assistant for pc · voice assistant windows 11 · control pc with voice · voice control pc · cortana replacement windows 11 · best ai voice assistant for pc
- Questions : comment contrôler son ordinateur par la voix ? · comment parler à son ordinateur ? · comment faire parler son ordinateur ? · comment activer la commande vocale sur pc ? · is there a voice assistant for windows 11? · can I control my pc with my voice? · can I talk to my computer? · what replaced cortana? · is cortana discontinued?
- Angle : Cortana est mort, « Hey Copilot » envoie la voix chez Microsoft, Jimy l'écoute en local.

### Thème 3 — Compagnon IA sur le bureau
- Têtes : ai desktop pet · desktop companion ai · best ai desktop companion · ai desktop pet windows · free ai desktop pet · clippy ai · clippy alternatives
- Questions : is there a clippy ai? · what is the best ai desktop companion? · is desktop companion ai safe? · ai desktop pet free download? · how to make a desktop buddy?
- Atout unique : les huit personnages 3D. Canal complémentaire : fiche itch.io (découverte + lien).

### Thème 4 — Quelle IA pour mon PC
- Têtes FR : ia pour pc gratuit · quelle ia pour mon pc · meilleur ia gratuit pour pc · logiciel ia gratuit pc · télécharger ia pour pc · meilleure application ia pour pc
- Têtes EN : best free ai assistant for pc · free ai assistant for windows 11 · best ai assistant for windows 11
- Questions : quelle est la meilleure IA pour PC ? · comment installer une IA sur son PC ? · peut-on parler à ChatGPT à la voix sur PC ? · mode vocal chatgpt pc · chatgpt voice mode not working on pc? · is there a chatgpt desktop app for windows? · is there an ai assistant for windows 11?
- Ne pas viser « chatgpt sur pc » de front (OpenAI imbattable) : contourner par la voix et le comparatif.

### Thème 5 — Une IA qui agit sur mon PC
- Têtes FR : agent ia pc · agent ia controle pc · agent ia gratuit · ia qui contrôle mon pc
- Têtes EN : ai that controls your computer · ai agent that controls your pc · ai agent for pc free · best ai agent for windows 11
- Questions : une IA peut-elle contrôler mon ordinateur ? · comment installer un agent IA sur son PC ? · quel est le meilleur agent IA gratuit ? · ia qui range mes fichiers ? · can chatgpt control my pc? · can ai organize my files? · is there an ai that controls your computer?

### Thème 6 — IA locale et gratuite
- Têtes FR : ia locale gratuite · ia hors ligne pc · ia sans abonnement · chatgpt hors ligne · quelle ia respecte la vie privée
- Têtes EN : local ai assistant windows · offline ai assistant for pc · best offline ai assistant for pc · free local ai assistant · private ai assistant · ai assistant without subscription
- Questions : peut-on utiliser ChatGPT hors ligne ? · quelle IA respecte la vie privée ? · is there an offline ai assistant? · what is the most private ai assistant? · best local ai voice assistant?
- Toujours préciser la condition : modèle local via LM Studio / Ollama, PC assez puissant.

### Thème 7 — Copilot
- FR : désactiver copilot windows 11 · comment désactiver copilot complètement · peut-on désinstaller copilot · alternative copilot gratuite · copilot gratuit pour pc
- EN : how to disable copilot windows 11 completely · is there a free alternative to copilot · better than copilot · is there anything better than copilot
- Suite logique de la page : « et le remplacer par quoi ? ». Exclure l'intention développeur (VS Code, GitHub Copilot).

## 6. Écarté pour cette cible

MCP et tout le contenu développeur (guide existant conservé tel quel) · « dicter à son
ordinateur » (la dictée Windows suffit, pas de téléchargement) · VS Code / GitHub Copilot ·
« chatgpt sur pc » en requête principale.

## 7. Règles GEO (être cité par les moteurs IA)

1. Sous-titres H2/H3 formulés comme les questions ci-dessus.
2. Réponse directe de 40 à 60 mots juste sous chaque question, puis le détail.
3. Un tableau comparatif par page (prix, local ou cloud, voix, Windows, open source).
4. Balisage `FAQPage` + `HowTo` quand il y a des étapes ; `BreadcrumbList` partout.
5. Présence hors site là où les IA puisent : Reddit, itch.io, AlternativeTo, Product Hunt,
   README GitHub (lien vers jimy-agent.me).
6. Chaque page de contenu renvoie vers le téléchargement (accueil `#telecharger`).

## 8. Suite

Déclinée en tâches dans `02-plan-contenu.md` et `calendrier.yaml` ; règles d'écriture dans
`03-charte-redactionnelle.md` ; suivi dans `06-mesure.md` et `journal.md`.
