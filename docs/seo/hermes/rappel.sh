#!/bin/bash
# Rappel de la veille du bot SEO : tâche Hermes jimy-seo-rappel (--no-agent, aucun modèle de langage).
# Hermes envoie la sortie telle quelle sur Telegram (bot Richard) ; sortie vide = aucun message.
# Installé en copie dans ~/.hermes/scripts/jimy_seo_rappel.sh (voir installation.md §6).
cd /home/jimmy/jimy-seo/jimmy-agent || exit 0
git fetch -q --prune origin 2>/dev/null
# Mise à jour seulement si le clone est sur main : ne jamais mélanger main dans une branche seo/<ID>.
[ "$(git rev-parse --abbrev-ref HEAD)" = main ] && git merge -q --ff-only origin/main >/dev/null 2>&1
/usr/bin/python3 docs/seo/outils/calendrier.py rappel 2>/dev/null
exit 0
