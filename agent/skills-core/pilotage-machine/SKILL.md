---
name: pilotage-machine
description: quand il faut agir sur la machine (commande, processus, OS inconnu) : découvrir le système avec system_info, préférer les outils natifs aux lignes shell, lectures ciblées, hygiène des secrets
updated: 2026-10-14T00:00:00.000000+00:00
---

1. Découvrir d'abord : `system_info` (OS, interpréteur) avant toute commande qui dépend du système. Ne jamais deviner Windows ou macOS.
2. Préférer les outils natifs : `process_list` (avec `filter`) plutôt que `tasklist`/`ps` tapés à la main — leurs sorties excluent les lignes de commande, qui peuvent contenir des secrets.
3. `run_command` parle déjà le terminal natif (PowerShell sur Windows, zsh/sh ailleurs) : ne jamais l'envelopper dans un autre appel shell. Une commande = un besoin, bornée dans le temps.
4. Lectures ciblées uniquement : jamais de balayage récursif depuis la racine du profil, jamais de vidage de l'environnement — lire le fichier nommé, valeurs masquées.
5. Ce qui engage ou détruit (arrêt, suppression large, écriture sensible) demande l'accord : carte dans le Chat, « confirmé » à la voix pour une fois. En cas de doute, décrire et laisser l'utilisateur décider.
6. Arrêter un programme : `process_list` pour le PID, puis `process_kill` (carte à chaque fois, portée « Toujours » au type d'action). Jimmy lui-même et le système sont refusés d'office. Doux d'abord, `force` en dernier recours.
7. Ouvrir : `open_path` pour un document ou un dossier (jamais un exécutable : refusé, à montrer dans l'Explorateur).
8. Fenêtres et applis : `window_list` pour lire (sans prendre le focus), `window_focus` par PID pour amener au premier plan — seulement sur demande ou en fin de tâche attendue. `app_open` lance une appli par son nom ; pour un fichier, `open_path`, pour une commande, `run_command`.
