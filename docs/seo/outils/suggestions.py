# -*- coding: utf-8 -*-
"""Suggestions de l'autocomplétion Google pour valider qu'une requête est réellement tapée.

Usage :
  python3 suggestions.py "<requête>" [--langue fr|en] [--questions]

--questions ajoute les variantes en question (comment/quel/meilleur… ou how/what/best/is there/can).
Une suggestion prouve qu'une requête existe, PAS son volume (01-strategie-mots-cles.md §3).
Pause de 0,3 s entre deux appels : ne pas lancer en boucle sur des centaines de requêtes.
"""
import json, sys, time, urllib.parse, urllib.request

sys.stdout.reconfigure(encoding="utf-8")
MODS = {"fr": ["comment ", "quel ", "quelle ", "meilleur ", "existe-t-il un ", "peut-on "],
        "en": ["how to ", "what is the best ", "best ", "is there a ", "can ", "free "]}


def suggerer(q, langue):
    gl = "fr" if langue == "fr" else "us"
    url = ("https://suggestqueries.google.com/complete/search?client=firefox&hl=%s&gl=%s&q=%s"
           % (langue, gl, urllib.parse.quote(q)))
    req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
    return json.loads(urllib.request.urlopen(req, timeout=10).read().decode("utf-8", "replace"))[1]


if __name__ == "__main__":
    a = sys.argv[1:]
    langue = "fr"
    if "--langue" in a:
        i = a.index("--langue"); langue = a[i + 1]; del a[i:i + 2]
    questions = "--questions" in a
    a = [x for x in a if x != "--questions"]
    if len(a) != 1: print(__doc__); sys.exit(2)
    vues = []
    for m in [""] + (MODS[langue] if questions else []):
        for s in suggerer(m + a[0], langue):
            if s not in vues: vues.append(s); print(s)
        time.sleep(0.3)
    if not vues: print("AUCUNE suggestion : requête probablement trop rare, à éviter comme mot-clé principal")
