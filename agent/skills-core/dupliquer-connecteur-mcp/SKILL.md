---
name: dupliquer-connecteur-mcp
description: quand l'utilisateur veut dupliquer un connecteur MCP existant (second compte, compte client) ou brancher un serveur avec une clé API : clé au coffre, mcp_add_server avec ${VAR}, vérification que le serveur répond
updated: 2026-10-14T00:00:00.000000+00:00
---

1. Identifier le connecteur source dans les réglages de Jimy (Skills, Serveurs MCP) : nom, URL, nom de la variable d'environnement. Ne jamais chercher un connecteur dans `~/.claude.json` ni `~/.claude/mcp.json` : ce sont ceux de Claude Code, pas ceux de Jimy.
2. Lire la clé visée uniquement (`read_file` sur le `.env` nommé, valeurs masquées à l'affichage). Ne jamais vider tout l'environnement (`Get-ChildItem Env:`, `printenv`) : les clés qui en sortent partent au modèle et bloquent l'envoi suivant.
3. Faire ranger la clé au coffre : proposer `secret:<nom>` (`%USERPROFILE%\.secrets\<nom>.json`, champ `api_key`). L'écriture au coffre demande l'accord de l'utilisateur (carte) ; à la voix, il dit « confirmé ».
4. Appeler `mcp_add_server` avec `--header "Authorization: Bearer ${VAR}"` et `"env": {"VAR": "secret:<nom>"}` (comme le serveur `aggregate`). Toute clé en clair est refusée avec la marche à suivre : ne pas contourner, demander la mise au coffre.
5. Vérifier : `serveur prêt : <nom>` au journal, outils listés (`mcp_list_tools`), puis un appel minimal réel (pour Emile : `list_campaigns`, puis `get_campaign_stats` avec `campaign_id` et `days`).
6. Repères Emile : même URL que le connecteur principal ; statistiques par `get_campaign_stats`, jamais par `GET /api/v1/campaigns/{id}/stats` (404) ; le compte client est l'espace Clients (id 2), les campagnes d'un client se filtrent par leurs boîtes d'envoi.
