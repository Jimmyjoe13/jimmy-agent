# -*- coding: utf-8 -*-
"""Attend le déploiement d'aperçu Vercel d'une branche et affiche son URL (bot SEO).

Usage :
  python3 apercu_vercel.py <branche> [--attente 900]
  python3 apercu_vercel.py --production [--attente 600]   attend que la prod serve le dernier commit de main

Source : API publique GitHub (dépôt public, sans jeton) ; Vercel y publie un « deployment »
par commit poussé, avec son statut et son URL. Code de sortie 1 si échec ou délai dépassé.
Les aperçus Vercel peuvent demander une connexion au compte Vercel de Jimmy : c'est normal.
"""
import json, subprocess, sys, time, urllib.request

sys.stdout.reconfigure(encoding="utf-8")
DEPOT = "Jimmyjoe13/jimmy-agent"


def api(chemin):
    req = urllib.request.Request(f"https://api.github.com/repos/{DEPOT}/{chemin}",
                                 headers={"Accept": "application/vnd.github+json", "User-Agent": "jimy-seo-bot"})
    return json.load(urllib.request.urlopen(req, timeout=20))


def sha_distant(ref):
    s = subprocess.run(["git", "rev-parse", f"origin/{ref}"], capture_output=True, text=True)
    return s.stdout.strip()


def attendre(sha, attente, production):
    fin = time.time() + attente
    while time.time() < fin:
        try:
            deps = api(f"deployments?sha={sha}&per_page=10")
            for d in deps:
                env = d.get("environment", "")
                if production != env.lower().startswith("production"): continue
                statuts = api(f"deployments/{d['id']}/statuses?per_page=5")
                if not statuts: continue
                s = statuts[0]
                if s["state"] == "success":
                    print(f"OK {env} : {s.get('environment_url') or s.get('target_url')}"); return 0
                if s["state"] in ("failure", "error"):
                    print(f"ERREUR déploiement {env} en échec : {s.get('target_url')}"); return 1
        except Exception as e:
            print(f"(nouvel essai : {str(e)[:80]})")
        time.sleep(30)
    print(f"ERREUR aucun déploiement {'production' if production else 'aperçu'} réussi après {attente} s pour {sha[:8]}")
    return 1


if __name__ == "__main__":
    a = sys.argv[1:]
    attente = 900
    if "--attente" in a:
        i = a.index("--attente"); attente = int(a[i + 1]); del a[i:i + 2]
    if a == ["--production"]:
        sys.exit(attendre(sha_distant("main"), attente, True))
    if len(a) == 1:
        sha = sha_distant(a[0])
        if not sha: print(f"ERREUR branche origin/{a[0]} introuvable (git fetch ?)"); sys.exit(1)
        sys.exit(attendre(sha, attente, False))
    print(__doc__); sys.exit(2)
