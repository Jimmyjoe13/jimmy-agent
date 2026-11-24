# -*- coding: utf-8 -*-
"""Refuse un push si la branche courante modifie des fichiers hors du périmètre du bot SEO.

Usage :
  python3 garde_chemins.py tache [--vercel-json]   branche seo/<ID> (contenu du site + docs/seo)
  python3 garde_chemins.py admin                   commit direct sur main (journal, calendrier, backlog, rapports)

Compare HEAD à origin/main. Code de sortie 1 si un fichier sort du périmètre : ne pas pousser,
prévenir Jimmy (05-garde-fous.md). --vercel-json n'est permis que pour une tâche qui le prévoit
dans son brief (redirections), et doit être signalé dans le message de validation.
"""
import fnmatch, subprocess, sys

sys.stdout.reconfigure(encoding="utf-8")

TACHE = ["site-jimy/*.html", "site-jimy/**/*.html", "site-jimy/sitemap.xml", "site-jimy/robots.txt",
         "site-jimy/assets/*", "site-jimy/assets/**", "docs/seo/*", "docs/seo/**"]
TOUJOURS_INTERDIT = ["site-jimy/api/**", "site-jimy/analytics-credentials/**", "site-jimy/.vercel*",
                     "site-jimy/assets/tracking.js", "site-jimy/index.html", "site-jimy/en/index.html",
                     "docs/seo/AGENTS.md", "docs/seo/README.md", "docs/seo/0*.md",
                     "docs/seo/outils/**", "docs/seo/hermes/**", "docs/seo/modeles/**"]
ADMIN = ["docs/seo/journal.md", "docs/seo/calendrier.yaml", "docs/seo/backlog.md", "docs/seo/rapports/*"]


def correspond(chemin, motifs):
    return any(fnmatch.fnmatch(chemin, m) for m in motifs)


def main():
    a = sys.argv[1:]
    if not a or a[0] not in ("tache", "admin"):
        print(__doc__); sys.exit(2)
    sortie = subprocess.run(["git", "diff", "--name-only", "origin/main...HEAD"], capture_output=True, text=True)
    if sortie.returncode:
        print("ERREUR git diff :", sortie.stderr.strip()); sys.exit(1)
    fichiers = [f for f in sortie.stdout.splitlines() if f]
    autorises = list(ADMIN) if a[0] == "admin" else list(TACHE)
    if a[0] == "tache" and "--vercel-json" in a: autorises.append("site-jimy/vercel.json")
    fautes = [f for f in fichiers if correspond(f, TOUJOURS_INTERDIT) or not correspond(f, autorises)]
    for f in fichiers: print(("REFUSÉ " if f in fautes else "ok     ") + f)
    if fautes:
        print(f"ERREUR {len(fautes)} fichier(s) hors périmètre : ne pas pousser, prévenir Jimmy."); sys.exit(1)
    print(f"OK {len(fichiers)} fichier(s) dans le périmètre « {a[0]} »")


if __name__ == "__main__":
    main()
