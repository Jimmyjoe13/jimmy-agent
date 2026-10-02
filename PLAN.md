# JIMMY — Agent IA desktop incarné

> **Statut : Prototype personnel — V1 locale**  
> **Objectif : Construire une première version fonctionnelle pour tester personnellement le concept avant toute décision de commercialisation.**

---

# 1. Cadrage stratégique & Objectifs

## Le problème à résoudre

Les assistants IA agentiques actuels permettent d'utiliser des modèles de langage, des outils, des skills, des MCP et des services externes, mais leur interaction reste principalement centrée sur une interface de chat classique.

Jimmy vise à expérimenter une nouvelle forme d'interaction avec un agent IA :

> **Un assistant IA agentique incarné par un personnage 3D vivant, présent directement sur le bureau de l'utilisateur et capable d'interagir avec lui principalement par la voix.**

L'objectif initial n'est pas de créer immédiatement un produit commercial, mais de vérifier si cette expérience apporte réellement quelque chose par rapport à une interface agentique classique.

## Proposition de valeur

> **Jimmy est un assistant IA personnel agentique qui peut comprendre les demandes vocales, utiliser des outils externes et agir sur l'environnement de travail de l'utilisateur, tout en étant incarné par un personnage 3D animé et personnalisable.**

Jimmy doit donner davantage l'impression d'interagir avec un **assistant vivant** plutôt qu'avec une simple fenêtre de chat.

## Concept produit

Jimmy est composé de plusieurs couches :

```text
┌─────────────────────────────────────────────┐
│                  JIMMY                      │
│                                             │
│     Avatar 3D + voix + personnalité        │
└──────────────────────┬──────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────┐
│             Agent Runtime                   │
│                                             │
│ LLM + contexte + mémoire + skills + tools  │
└──────────────┬──────────────────────────────┘
               │
       ┌───────┼────────┐
       ▼       ▼        ▼
      CLI     MCP      APIs
       │       │        │
       └───────┼────────┘
               ▼
      Environnement utilisateur
```

## Livrables attendus

Pour la V1 personnelle :

- Application desktop Windows fonctionnelle.
- Avatar 3D Jimmy.
- Premier skin : renard humanoïde.
- Avatar flottant sur le bureau.
- Bulle de dialogue flottante.
- Interface complète accessible au clic.
- Interaction vocale par défaut.
- Wake word : **« Jimmy »**.
- Speech-to-Text.
- Réponse LLM.
- Text-to-Speech.
- Voix française féminine **Clémence** via Fish Audio.
- Provider LLM **OpenCode Go**.
- Choix du modèle configurable.
- Mémoire conversationnelle locale.
- Mémoire personnelle construite automatiquement à partir des conversations.
- Système de tools.
- Support CLI.
- Support APIs / MCP.
- Support navigateur.
- Système de skills.
- Capacité de Jimmy à créer et améliorer ses skills.
- Détection et installation de MCP pertinents.
- Intégration locale avec **Synaptiq** pour les tâches où son utilisation est pertinente.
- Système de permissions.
- Console agentique simplifiée.
- Trois niveaux graphiques pour l'avatar.
- Installateur Windows capable de vérifier les prérequis.
- Système de mise à jour avec notification utilisateur.

## Critères de succès

Le prototype sera considéré comme pertinent si :

### Expérience

- Jimmy peut être utilisé principalement à la voix.
- Le wake word fonctionne correctement.
- La réponse vocale est suffisamment naturelle.
- L'avatar donne une impression de présence.
- Les animations correspondent aux principaux états de Jimmy.
- L'interaction est plus naturelle qu'un simple chat pour les tâches courantes.

### Agent

- Jimmy comprend correctement les demandes.
- Jimmy peut utiliser les tools disponibles.
- Jimmy peut utiliser CLI, APIs/MCP et navigateur.
- Jimmy peut enchaîner plusieurs actions pour accomplir une tâche.
- Jimmy peut créer des skills lorsqu'il identifie un besoin récurrent.
- Jimmy peut utiliser Synaptiq lorsque celui-ci est pertinent.

### Technique

- Application stable sur la machine personnelle.
- Consommation CPU/GPU/RAM acceptable.
- Avatar fluide sur les trois niveaux graphiques.
- Aucun serveur applicatif Jimmy nécessaire.
- Les données locales restent sur la machine, hors requêtes vers les services externes explicitement utilisés.

### Validation globale

Le principal KPI de la V1 est qualitatif :

> **Est-ce que Jimmy devient réellement utile dans mon usage quotidien et apporte une meilleure expérience qu'un agent IA classique ?**

La décision de commercialiser ou non le produit sera prise après cette phase.

---

# 2. Périmètre (Scope Management)

## Dans le périmètre — V1

| In-Scope | Hors périmètre V1 |
|---|---|
| Prototype personnel | Commercialisation immédiate |
| Windows | macOS |
| Tauri | Android |
| Godot | iOS |
| Avatar 3D | Marketplace de skins |
| Skin renard | Vente de skins |
| Interaction vocale | Système d'abonnement |
| Wake word | Paiement |
| Chat textuel | Comptes utilisateurs cloud |
| LLM via OpenCode Go | Backend cloud Jimmy |
| Modèle configurable | Infrastructure SaaS |
| Skills | Marketplace de skills |
| Création automatique de skills | Communauté |
| MCP | Multi-utilisateurs |
| CLI | Synchronisation cloud |
| APIs | Infrastructure distante |
| Navigateur | Système de billing |
| Mémoire locale | Social features |
| Synaptiq local | Application iOS |
| Historique local | Application Android |
| Permissions | Économie de skins |
| Animations avatar | Personnalisation avancée des skins |

## Cas d'usage nominal

```text
Utilisateur
     │
     │ « Jimmy, analyse ce dossier »
     ▼
Wake Word
     │
     ▼
Speech-to-Text
     │
     ▼
Agent Jimmy
     │
     ├── Mémoire
     ├── Skills
     ├── Synaptiq si pertinent
     ├── Tools
     ├── MCP
     ├── CLI
     ├── API
     └── Navigateur
     │
     ▼
Résultat
     │
     ├── Réponse texte
     ├── Réponse vocale
     └── Animation Godot
```

---

# 3. Architecture & Choix techniques

## Principe architectural

La V1 doit être **locale par conception**.

Aucun backend Jimmy ne sera hébergé dans le cloud.

Les composants locaux doivent fonctionner directement sur la machine de l'utilisateur lorsque cela est techniquement pertinent.

Les services externes ne sont utilisés que lorsqu'ils sont nécessaires, notamment pour le LLM et la génération vocale.

## Architecture

```text
                    WINDOWS
┌────────────────────────────────────────────────────┐
│                                                    │
│                   TAURI APP                        │
│                                                    │
│  ┌──────────────────────────────────────────────┐  │
│  │ Interface utilisateur                       │  │
│  │                                              │  │
│  │ Chat / Historique / Settings / Skin         │  │
│  └──────────────────────┬───────────────────────┘  │
│                         │                          │
│                         ▼                          │
│  ┌──────────────────────────────────────────────┐  │
│  │              JIMMY AGENT                    │  │
│  │                                              │  │
│  │ Context / Planning / Tools / Skills         │  │
│  └───────┬──────────┬──────────┬──────────────┘  │
│          │          │          │                 │
│          ▼          ▼          ▼                 │
│       Synaptiq    Memory      Tools              │
│       local       locale       │                 │
│                                ├── CLI            │
│                                ├── MCP            │
│                                ├── APIs           │
│                                └── Browser        │
│                                                    │
│  ┌──────────────────────────────────────────────┐  │
│  │              GODOT                          │  │
│  │                                              │  │
│  │ Avatar / Animations / Expressions           │  │
│  └──────────────────────┬───────────────────────┘  │
│                         │                          │
│                     HTTP local                    │
│                         │                          │
└─────────────────────────┼──────────────────────────┘
                          │
                          ▼
                Services externes nécessaires
                ├── OpenCode Go
                └── Fish Audio
```

## Stack

### Desktop

- Tauri
- Windows V1
- macOS/Linux ultérieurement

### Avatar

- Godot
- Modèles 3D
- Animations
- Expressions
- Gestion des états

### Communication

- HTTP local entre Tauri et Godot.

### Agent

- OpenCode Go
- Modèle configurable
- Architecture permettant de changer de modèle.

### Intelligence complémentaire

- Synaptiq installé localement.
- Jimmy décide dynamiquement quand l'utiliser selon la complexité ou la pertinence de la tâche.

### Voice

```text
« Jimmy »
    ↓
Wake Word local
    ↓
Speech-to-Text
    ↓
Agent
    ↓
Text-to-Speech
    ↓
Fish Audio
    ↓
Voix Clémence
```

### Mémoire

- Historique local.
- Mémoire vectorielle locale.
- Construction automatique de la mémoire à partir des conversations.
- Architecture exacte du moteur vectoriel à définir ultérieurement.

### Données

- SQLite + fichiers locaux.
- Historique local.
- Configuration locale.
- Assets locaux.
- Données de mémoire locales.

### Secrets

Pour la V1 personnelle :

```text
.env
```

Les clés API et tokens nécessaires sont stockés localement dans les variables d'environnement.

## États principaux de l'avatar

La première version reste volontairement simple :

```text
IDLE
  ↓
LISTENING
  ↓
THINKING
  ↓
SPEAKING
```

Chaque état peut déclencher une animation différente.

Le système devra être extensible afin d'ajouter ultérieurement :

```text
EXECUTING
SUCCESS
ERROR
WAITING
SURPRISED
HAPPY
etc.
```

## Qualité graphique

Trois niveaux :

```text
LOW
MEDIUM
HIGH
```

L'utilisateur pourra choisir manuellement le niveau adapté à sa machine.

## Interface

### Mode normal

Jimmy apparaît comme un personnage flottant sur le bureau.

Il peut être déplacé librement.

La bulle de dialogue :

- apparaît lors des interactions ;
- affiche les réponses ;
- affiche les états importants ;
- permet d'accéder à l'interface complète.

### Interface complète

Au clic sur Jimmy :

- Chat
- Historique
- Paramètres
- Configuration du skin

Le mode avancé pourra être développé ultérieurement.

## Installation

L'installateur doit :

1. Vérifier les prérequis.
2. Vérifier les composants nécessaires.
3. Installer/configurer les dépendances nécessaires.
4. Demander les permissions système.
5. Installer Jimmy.
6. Lancer le premier onboarding.

## Premier lancement

Jimmy accueille vocalement l'utilisateur.

Il guide l'utilisateur à travers un onboarding conversationnel.

L'onboarding permet notamment de :

- configurer Jimmy ;
- définir certaines préférences ;
- sélectionner le skin ;
- configurer les accès ;
- configurer le modèle ;
- configurer les outils nécessaires.

Un mode de configuration avancé pourra être accessible ultérieurement.

---

# 4. Plan de déploiement & Jalons

## Phase 1 — Cadrage & Maquettage

Objectifs :

- Valider l'architecture locale.
- Préparer le projet Tauri.
- Préparer le projet Godot.
- Créer le premier avatar.
- Définir les états de l'avatar.
- Définir la communication Tauri ↔ Godot.
- Définir le flux vocal.
- Définir la structure de l'agent.

### Livrable

Un prototype technique minimal capable d'afficher Jimmy et de recevoir des commandes.

---

## Phase 2 — Développement du MVP personnel

### Étape 1 — Shell desktop

- Tauri.
- Fenêtre/app flottante.
- Positionnement de Jimmy.
- Configuration du démarrage Windows.

### Étape 2 — Avatar

- Import du skin.
- Animations.
- États `idle / listening / thinking / speaking`.
- Trois niveaux graphiques.
- Communication HTTP locale.

### Étape 3 — Agent

- OpenCode Go.
- Choix du modèle.
- Context management.
- Tools.
- CLI.
- APIs.
- MCP.
- Navigateur.

### Étape 4 — Voix

- Wake word local.
- STT.
- Agent.
- Fish Audio.
- Voix Clémence.

### Étape 5 — Mémoire

- Historique local.
- Mémoire vectorielle locale.
- Récupération de contexte.

### Étape 6 — Skills

- Skills.
- Détection de besoins récurrents.
- Création automatique.
- Test/amélioration des skills.

### Étape 7 — Synaptiq

- Connexion locale.
- Détection des tâches pertinentes.
- Utilisation dynamique selon la complexité.

---

## Phase 3 — Tests & Recette

Tester :

- Wake word.
- Reconnaissance vocale.
- Réponse vocale.
- Conversations longues.
- Mémoire.
- Skills.
- Création de skills.
- MCP.
- CLI.
- Navigateur.
- APIs.
- Erreurs réseau.
- Timeout.
- Perte de connexion au provider.
- Permissions.
- Crash de Godot.
- Crash de Tauri.
- Synchronisation Tauri/Godot.
- Performance graphique.
- Consommation mémoire.
- Consommation CPU/GPU.

---

## Phase 4 — Validation personnelle

Le prototype sera utilisé quotidiennement sur la machine personnelle.

Objectif :

> Déterminer si Jimmy est suffisamment utile, agréable et fiable pour justifier la création d'une véritable V1 commerciale.

Cette phase permettra notamment d'identifier :

- les fonctionnalités réellement utiles ;
- les fonctionnalités inutiles ;
- les problèmes d'UX ;
- les limites techniques ;
- les coûts externes ;
- les problèmes de performance ;
- les besoins en personnalisation ;
- les fonctionnalités à conserver pour une éventuelle commercialisation.

---

# 5. Gestion des risques & Dépendances

## Dépendances externes

Même si l'application est locale, certains services restent externes :

- OpenCode Go.
- Modèle LLM sélectionné.
- Fish Audio pour le TTS.
- Éventuelles APIs utilisées par les tools.
- Éventuels MCP externes.

## Risque 1 — Scope trop important

Le concept mélange :

- agent IA ;
- voix ;
- mémoire ;
- MCP ;
- skills ;
- CLI ;
- navigateur ;
- avatar 3D ;
- desktop app.

### Mitigation

Construire progressivement le happy path :

```text
Jimmy
  ↓
Voix
  ↓
LLM
  ↓
Réponse
  ↓
Voix + Avatar
```

Puis ajouter les capacités agentiques une par une.

---

## Risque 2 — Complexité Tauri + Godot

Deux runtimes doivent fonctionner ensemble.

### Mitigation

Maintenir une séparation claire :

```text
Tauri = application / agent / interface
Godot = avatar / animation / rendu
```

Communication uniquement via HTTP local dans la V1.

---

## Risque 3 — Performance

Le rendu 3D peut devenir coûteux sur certaines machines.

### Mitigation

Trois profils :

```text
LOW
MEDIUM
HIGH
```

Limiter également la complexité du premier skin et des animations.

---

## Risque 4 — Dépendance aux providers externes

Le prototype dépend notamment d'OpenCode Go et Fish Audio.

### Mitigation

Abstraire les interfaces :

```text
LLM Provider
TTS Provider
STT Provider
Tool Provider
```

Cela permettra de changer de fournisseur sans réécrire Jimmy.

---

## Risque 5 — Autonomie excessive des skills

Jimmy peut créer et améliorer automatiquement ses skills.

### Mitigation

Les skills restent soumis au système de permissions défini par l'utilisateur.

Jimmy ne doit pas obtenir automatiquement de nouveaux droits système simplement parce qu'il crée un skill.

---

## Risque 6 — Permissions système

Jimmy peut interagir avec :

- CLI ;
- fichiers ;
- navigateur ;
- APIs ;
- applications.

### Mitigation

Système de permissions :

```text
LECTURE
MODIFICATION
EXÉCUTION
```

Les permissions sont configurées par l'utilisateur avant utilisation.

---

## Risque 7 — Le personnage devient uniquement cosmétique

L'avatar ne doit pas être uniquement décoratif.

### Mitigation

Connecter directement les états de l'agent aux animations :

```text
Listening → animation d'écoute
Thinking → animation de réflexion
Speaking → animation de parole
Idle → animation de vie
```

L'avatar devient ainsi une représentation visuelle de l'état de Jimmy.

---

# 6. Definition of Done (DoD)

Le prototype personnel est officiellement terminé lorsque :

### Application

- [ ] Jimmy peut être installé sur Windows.
- [ ] L'installateur vérifie les prérequis.
- [ ] Les permissions nécessaires sont demandées.
- [ ] Jimmy peut démarrer automatiquement ou manuellement selon la configuration.
- [ ] Jimmy apparaît comme avatar flottant.
- [ ] Jimmy peut être déplacé sur le bureau.

### Avatar

- [ ] Le skin renard est intégré.
- [ ] L'avatar fonctionne dans Godot.
- [ ] Les animations principales fonctionnent.
- [ ] Les états `idle`, `listening`, `thinking`, `speaking` sont opérationnels.
- [ ] Les trois niveaux graphiques fonctionnent.
- [ ] Tauri et Godot communiquent correctement.

### Agent

- [ ] Jimmy peut recevoir une demande textuelle.
- [ ] Jimmy peut recevoir une demande vocale.
- [ ] Le wake word fonctionne.
- [ ] Le modèle OpenCode Go fonctionne.
- [ ] Le modèle peut être configuré.
- [ ] Jimmy peut utiliser les tools.
- [ ] Jimmy peut utiliser CLI.
- [ ] Jimmy peut utiliser APIs/MCP.
- [ ] Jimmy peut utiliser le navigateur.

### Skills

- [ ] Jimmy peut utiliser des skills.
- [ ] Jimmy peut identifier un besoin récurrent.
- [ ] Jimmy peut créer un skill.
- [ ] Jimmy peut tester/améliorer un skill.
- [ ] Les permissions restent respectées.

### Mémoire

- [ ] L'historique est stocké localement.
- [ ] La mémoire personnelle est construite à partir des conversations.
- [ ] La mémoire vectorielle locale fonctionne.
- [ ] Jimmy peut récupérer des informations pertinentes de sa mémoire.

### Voix

- [ ] Wake word « Jimmy » fonctionnel.
- [ ] Speech-to-Text fonctionnel.
- [ ] Fish Audio fonctionnel.
- [ ] Voix française « Clémence » fonctionnelle.
- [ ] Réponse vocale fonctionnelle.

### Synaptiq

- [ ] Jimmy peut communiquer avec l'instance locale Synaptiq.
- [ ] Jimmy peut déterminer quand son utilisation est pertinente.

### Qualité

- [ ] Les erreurs principales sont gérées.
- [ ] Les crashs critiques sont identifiés.
- [ ] Les performances sont acceptables.
- [ ] Les secrets sont stockés dans `.env`.
- [ ] Le code est versionné.
- [ ] Le projet possède une documentation minimale.
- [ ] L'installation fonctionne depuis une machine propre.

### Validation finale

Le prototype doit permettre de réaliser cette boucle de bout en bout :

```text
┌────────────────────────────────────────────┐
│                                            │
│  « Jimmy, fais X »                         │
│          │                                 │
│          ▼                                 │
│      Wake Word                             │
│          │                                 │
│          ▼                                 │
│     Speech-to-Text                         │
│          │                                 │
│          ▼                                 │
│       Jimmy Agent                          │
│          │                                 │
│    ┌─────┼─────┐                           │
│    ▼     ▼     ▼                           │
│  Skill  Tool  Synaptiq                     │
│    │     │     │                           │
│    └─────┼─────┘                           │
│          ▼                                 │
│       Résultat                             │
│          │                                 │
│     ┌────┴────┐                            │
│     ▼         ▼                            │
│   Voix      Avatar                         │
│ Clémence   Godot                           │
│                                            │
└────────────────────────────────────────────┘
```

## Décision de sortie du prototype

À la fin de cette phase, trois décisions seront possibles :

**1. Stop**  
Le concept n'apporte pas suffisamment de valeur.

**2. Itération personnelle**  
Le concept est intéressant mais nécessite plusieurs itérations avant commercialisation.

**3. Commercialisation**  
Le prototype valide suffisamment l'expérience pour commencer à construire une véritable V1 produit.

La commercialisation, l'abonnement, les skins payants et les applications Android/iOS seront traités **après cette validation**, et ne font pas partie du prototype personnel.