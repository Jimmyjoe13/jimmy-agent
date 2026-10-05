# Jimmy — consignes pour les agents

**Avant toute modification, lire `INSTRUCTIONS.md`** (directives obligatoires,
à respecter absolument), puis `CONTRIBUTION.md` (règles, erreurs déjà
commises, vérifications à lancer) et `HANDOFF.md` (état technique et pièges
numérotés). `JOURNAL.md` raconte l'historique.

Les quatre règles qui évitent le plus de régressions :

1. **Compiler uniquement avec `.\scripts\build.ps1 -Release`, depuis PowerShell.**
   Jamais `cargo build --release` seul (interface vide : « localhost a refusé de
   se connecter »), jamais depuis Git Bash (mauvais `link.exe`).
2. **Reproduire un bug par un test avant de le corriger**, et vérifier avec la
   vraie application (`.\scripts\test-ui.ps1`), pas seulement `cargo build`.
3. **Tout ce qui est ajouté doit avoir un appelant réel** (`rg`) : une fonction
   définie mais jamais appelée est déjà arrivée quatre fois.
4. **Ne pas toucher à Jimmy pendant que l'utilisateur s'en sert** (compilation,
   tests d'écoute, arrêt de processus) sans l'avoir vérifié.

Réponses en français, courtes. Aucun secret dans un prompt ni dans un commit.
Rien n'est poussé sur le dépôt distant sans demande explicite. Fin de run
autonome : bloc `Fait / Restant / Bloqué` (voir `CONTRIBUTION.md` §9).
