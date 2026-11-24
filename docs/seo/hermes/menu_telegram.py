# -*- coding: utf-8 -*-
"""Ajoute les commandes du bot SEO en tête du menu Telegram de Richard, pour UNE conversation.

Usage (sur le VPS) :
  /usr/bin/python3 menu_telegram.py <chat_id>

Pourquoi par conversation : Hermes réécrit le menu général (30 commandes, son maximum) à
chaque démarrage et n'y met pas les commandes rapides. Il ne touche jamais au menu propre
à une conversation privée, qui est prioritaire pour cette conversation.
Le script recopie le menu général actuel de Hermes et place devant les 3 commandes SEO :
le relancer après une mise à jour de Hermes pour reprendre ses nouvelles commandes.
Le jeton du bot est lu dans ~/.hermes/.env (TELEGRAM_BOT_TOKEN), jamais affiché.
Le chat_id n'est pas écrit dans le dépôt (public) : il est passé en argument.
"""
import json, os, sys, urllib.parse, urllib.request

SEO = [
    ("seo", "SEO : état du bot (+ ok S01, corrige S01 …)"),
    ("seo_rapport", "SEO : rapport Search Console + GA4 (28 jours)"),
    ("seo_derniere", "SEO : dernière tâche faite et prochaine"),
]


def jeton():
    with open(os.path.expanduser("~/.hermes/.env"), encoding="utf-8") as f:
        for l in f:
            if l.startswith("TELEGRAM_BOT_TOKEN="):
                return l.split("=", 1)[1].strip().strip('"').strip("'")
    sys.exit("ERREUR TELEGRAM_BOT_TOKEN absent de ~/.hermes/.env")


def appel(methode, **params):
    data = urllib.parse.urlencode({k: json.dumps(v) if isinstance(v, (dict, list)) else v
                                   for k, v in params.items()}).encode()
    r = json.load(urllib.request.urlopen(f"https://api.telegram.org/bot{jeton()}/{methode}", data, timeout=20))
    if not r.get("ok"): sys.exit(f"ERREUR {methode} : {r.get('description')}")
    return r["result"]


if __name__ == "__main__":
    if len(sys.argv) != 2: print(__doc__); sys.exit(2)
    chat = int(sys.argv[1])
    general = appel("getMyCommands", scope={"type": "all_private_chats"}) or appel("getMyCommands")
    noms_seo = {n for n, _ in SEO}
    menu = [{"command": n, "description": d} for n, d in SEO]
    menu += [c for c in general if c["command"] not in noms_seo]
    appel("setMyCommands", commands=menu[:100], scope={"type": "chat", "chat_id": chat})
    pose = appel("getMyCommands", scope={"type": "chat", "chat_id": chat})
    print(f"OK menu de la conversation : {len(pose)} commandes, en tête : " + ", ".join("/" + c["command"] for c in pose[:3]))
