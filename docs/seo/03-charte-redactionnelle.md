# 03 — Charte rédactionnelle et fiche produit

Règles d'écriture de toute page produite par le bot. `outils/verifier.py` contrôle
automatiquement ce qui est mesurable (marqué **[auto]**) ; le reste est de la responsabilité
du bot et sera relu par Jimmy à la validation.

## 1. Lecteur et ton

- **Lecteur** : grand public Windows, pas développeur. Il veut une solution, pas un cours.
- **FR** : tutoiement (comme l'accueil du site), phrases courtes, vocabulaire courant.
  Un terme technique (LLM, MCP, API) est expliqué en une demi-phrase la première fois.
- **EN** : anglais américain simple, « you », pas de jargon. La page EN n'est **pas une
  traduction mot à mot** de la page FR : mêmes faits et même structure, mais mots-clés et
  questions EN du brief, formulés comme un anglophone les tape.
- Honnête et factuel : on présente Jimy comme une option parmi d'autres, avec ses limites.
  Jamais de dénigrement d'un concurrent.
- Pas de remplissage (« Dans le monde d'aujourd'hui… », « Il est important de noter… »).
  Pas d'emojis dans le texte des pages.

## 2. Structure imposée d'une page guide

Partir de `modeles/guide-fr.html` / `modeles/guide-en.html`. Dans l'ordre :

1. **title** 30-60 caractères, mot-clé principal au début si possible **[auto]**.
2. **meta description** 120-155 caractères, contient le mot-clé et un bénéfice concret **[auto]**.
3. **H1** unique, formulé pour le lecteur (peut différer du title) **[auto]**.
4. **Chapeau** : 2 à 4 phrases, mot-clé principal dans la première phrase **[auto : alerte]**.
5. **Sommaire** reprenant les H2.
6. **Sections « question »** : au moins 4 H2 rédigés comme les questions du brief **[auto]**.
   Chaque H2 est suivi **immédiatement** d'un `<p class="reponse">` de 40 à 60 mots
   (35 à 70 tolérés) **[auto]** qui répond seul à la question, sans renvoyer à la suite :
   c'est le passage que Google et les moteurs IA extraient. Le détail vient après (listes,
   étapes, exemples).
7. **Tableau comparatif** : colonnes du brief, Jimy en colonne `highlight`, puis 2 à 3 phrases
   qui lisent le tableau honnêtement (où Jimy est moins bon aussi).
8. **FAQ** : 3 à 5 questions en H3, et le **même texte** dans le bloc JSON-LD `FAQPage` **[auto : présence]**.
9. **À lire aussi** : liens du brief (maillage sortant) + hub des guides.
10. **Appel à l'action** : bouton `data-track="download"` du gabarit, sans le modifier **[auto]**.

Longueur : 900 mots minimum **[auto]**, 1 200 à 1 800 visés. Plus long seulement si la
question l'exige (pas-à-pas).

Liens : ancres descriptives (« comparer les alternatives à Copilot »), jamais « cliquez ici ».
Liens externes : seulement vers des sources de faits (Microsoft, éditeurs, presse reconnue),
avec `rel="noopener"` ; aucun lien vers des sites de téléchargement tiers.

## 3. Exactitude : d'où viennent les faits

| Type d'affirmation | Source obligatoire |
|---|---|
| Ce que fait Jimy | §6 « Fiche produit » ci-dessous, sinon le code du dépôt (`agent/src`, `desktop/`), sinon **ne pas l'écrire** |
| Étapes dans l'interface de Jimy | Le code ou l'accueil du site ; si un libellé exact n'est pas vérifiable, l'écrire de façon générique (« dans les réglages du fournisseur ») et le **signaler** dans le message de validation |
| Fonctionnement de Windows, Copilot, Cortana | Page Microsoft (support.microsoft.com, learn.microsoft.com) ou presse tech reconnue, citée dans le message de validation |
| Concurrents (prix, fonctions) | Page officielle du concurrent, consultée le jour même ; sinon rester générique (« les projets Jarvis open source sur GitHub ») |
| Chiffres (volumes, parts de marché, statistiques) | Interdits sans source vérifiable. Jamais de volume de recherche inventé |
| Version de Jimy | Lire `softwareVersion` dans `site-jimy/index.html` le jour de la rédaction |

Toute source utilisée est listée dans le message de validation envoyé à Jimmy (P1, étape 9).

## 4. Promesses interdites **[auto pour les formulations ci-dessous]**

- « 100 % hors ligne », « fully offline », « entièrement hors ligne » : faux par défaut. Écrire :
  « la reconnaissance vocale tourne en local ; avec un modèle local (LM Studio, Ollama),
  tout reste sur ton PC ».
- « sans clé d'API », « aucune clé », « no API key » : faux. Jimy demande un fournisseur
  de modèle ; il peut être local et gratuit.
- « Jarvis officiel », « official Jarvis » : Jarvis est une marque Marvel. Écrire « un assistant
  comme Jarvis », « Jarvis-like ». Jamais de logo, d'image ni de citation de film.
- « gratuit » sans précision quand on parle de l'usage du modèle : l'appli est gratuite ;
  l'usage est gratuit avec un modèle local ou un modèle gratuit, sinon payé au fournisseur.
- Aucune promesse sur la signature de l'installeur (en cours chez SignPath, sans date).
- Aucune fonction future présentée comme disponible (Mac, JEVA OMEGA, boutique de skins).

## 5. Comparatifs

- Comparer des **catégories** quand les faits précis ne sont pas sourcés : « Copilot (intégré
  à Windows) », « applis Jarvis amateurs », « compagnons Live2D sur Steam ».
- Critères toujours factuels : installation, voix, agit sur le PC, où vont les données, prix,
  open source, langues.
- Valeurs : « Oui », « Non », « Partiel » + précision courte. Pas d'étoiles ni de notes.

## 6. Fiche produit vérifiée (état au 24/11/2026, version 0.2.4)

Source : accueil du site et code du dépôt. À relire si `softwareVersion` change.

- **Quoi** : agent IA de bureau pour Windows, incarné par un personnage 3D animé qui reste
  au-dessus des fenêtres (on peut le déplacer, il esquive la souris).
- **Système** : Windows 10 et 11, 64 bits. Pas de Mac (prévu, pas de date). Développé sur un
  portable Ryzen 7 / 16 Go sans carte graphique dédiée ; trois profils graphiques pour les
  machines modestes.
- **Prix et licence** : appli gratuite, open source (licence MIT, code sur GitHub), installeur
  construit par GitHub Actions. Cinq skins de base offerts ; skins premium payants à venir
  (boutique pas encore ouverte).
- **Voix** : mot d'éveil « Jimy » détecté sur la machine ; reconnaissance vocale whisper.cpp
  locale (téléchargée au premier lancement : ~65 Mo, +180 Mo en option) ; l'audio du micro ne
  sort pas du PC. Réponse à voix haute (synthèse vocale : 4 moteurs dont un local Windows,
  les autres en ligne). Écoute réglable ou coupable dans Paramètres → Écoute.
- **Cerveau** : modèle de langage au choix avec sa propre clé : OpenCode Go, OpenRouter,
  Claude, DeepSeek, ou un fournisseur compatible OpenAI ajouté à la main, y compris un serveur
  **local** (LM Studio, Ollama) → usage gratuit et données sur le PC. Un modèle pour le travail,
  un plus rapide pour la voix.
- **Actions** : fichiers, commandes, son propre navigateur Chrome (recherche, comparaison,
  formulaires), serveurs MCP, skills appris. Tâches longues en arrière-plan avec un compagnon.
  « Regarde mon écran » : voit la fenêtre active, seulement sur demande.
- **Garde-fous** : carte d'autorisation avant tout ce qui engage (payer, envoyer, supprimer,
  écrire un secret, publier) ; « Toujours autoriser » possible.
- **Mémoire** : préférences, projets, erreurs passées ; peut se brancher sur un coffre Obsidian ;
  âme (SOUL.md) qu'il fait évoluer quand on le corrige (0.2.4).
- **Vie privée** : pas de compte, pas de pub, pas de télémétrie ; historique, mémoire, réglages
  et clés stockés localement. Partent sur le réseau : le fournisseur de modèle, la synthèse
  vocale en ligne, et ce qu'on lui demande de faire en ligne.
- **Langues** : français et anglais.
- **Skins** : huit personnages (Renard, Arctique, Fennec, Ours, Robot, Néon, JEVA, ARK) ;
  cinq offerts.
- **Installation** : `Jimy-setup.exe` (bouton du gabarit) ; installeur pas encore signé →
  SmartScreen peut avertir (procédure sur `/#defender`, `/en/#defender`).
- **Limites à dire quand c'est pertinent** : Windows seulement ; il faut un fournisseur de
  modèle (local ou en ligne) ; installeur non signé pour l'instant ; projet jeune (0.2.x).

## 7. Images

Pas d'image générée ni téléchargée. Utiliser uniquement les fichiers existants de
`site-jimy/assets/` (ex. `assets/skins/*.webp`, `assets/og.png`) avec `alt` descriptif,
`width` et `height` **[auto : alt]**.
