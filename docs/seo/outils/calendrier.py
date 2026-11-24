# -*- coding: utf-8 -*-
"""Lecture et mise à jour de docs/seo/calendrier.yaml (bot SEO).

Usage :
  python3 calendrier.py etat                       tableau de toutes les tâches
  python3 calendrier.py prochaine                  prochaine tâche de contenu à produire (ou rien)
  python3 calendrier.py dues                       tâches hors site et mesures dues aujourd'hui
  python3 calendrier.py derniere                   dernière tâche terminée + dernière entrée du journal (/seo_derniere)
  python3 calendrier.py statut <ID> <STATUT> [--date AAAA-MM-JJ] [--note "texte"]

Ne jamais éditer calendrier.yaml à la main : passer par ce script (format et statuts garantis).
L'état « en_redaction » / « en_validation » d'une tâche de contenu se lit aussi dans git :
une branche origin/seo/<ID> existe tant que la tâche n'est pas publiée ou abandonnée.
"""
import os, subprocess, sys
from datetime import date, timedelta
import yaml

sys.stdout.reconfigure(encoding="utf-8")
FICHIER = os.path.join(os.path.dirname(__file__), "..", "calendrier.yaml")
STATUTS = {
    "contenu": ["a_faire", "en_redaction", "en_validation", "en_correction", "publie", "mesure", "abandonne"],
    "hors_site": ["a_faire", "en_validation", "fait", "abandonne"],
    "manuel": ["a_faire", "fait"],
    "revue": ["a_faire", "en_validation", "fait"],
}
OUVERTS = ("en_redaction", "en_validation", "en_correction")
MAX_OUVERTES = 2  # 05-garde-fous.md : au plus 2 tâches de contenu ouvertes à la fois


def charger():
    with open(FICHIER, encoding="utf-8") as f:
        texte = f.read()
    entete = "".join(l for l in texte.splitlines(True) if l.startswith("#"))
    return entete, yaml.safe_load(texte)


def sauver(entete, data):
    with open(FICHIER, "w", encoding="utf-8", newline="\n") as f:
        f.write(entete)
        yaml.safe_dump(data, f, allow_unicode=True, sort_keys=False, width=200)


def taches():
    return charger()[1]["taches"]


def jour(x):
    return x if isinstance(x, date) else date.fromisoformat(str(x))


def faite(t):
    return t["statut"] in ("publie", "mesure", "fait")


def branches_ouvertes():
    """IDs ayant une branche origin/seo/<ID> : tâche en cours, même si main dit encore a_faire."""
    s = subprocess.run(["git", "branch", "-r", "--list", "origin/seo/*"], capture_output=True, text=True)
    return {l.strip().split("/")[-1] for l in s.stdout.splitlines() if l.strip()}


def etat():
    br = branches_ouvertes()
    print(f"{'ID':5} {'type':10} {'date':11} {'statut':14} titre")
    for t in taches():
        marque = " [branche ouverte]" if t["id"] in br else ""
        print(f"{t['id']:5} {t['type']:10} {str(t['date']):11} {t['statut']:14} {t['titre']}{marque}")


def prochaine():
    ts = taches(); par_id = {t["id"]: t for t in ts}
    br = branches_ouvertes()
    ouvertes = [t for t in ts if t["type"] == "contenu" and (t["statut"] in OUVERTS or t["id"] in br)]
    if len(ouvertes) >= MAX_OUVERTES:
        print(f"RIEN : {len(ouvertes)} tâches ouvertes ({', '.join(t['id'] for t in ouvertes)}), maximum {MAX_OUVERTES}")
        return
    for t in ts:
        if t["id"] in br: continue  # déjà en cours sur sa branche
        if t["type"] in ("contenu", "revue") and t["statut"] == "a_faire" and jour(t["date"]) <= date.today():
            bloquantes = [d for d in t.get("depend_de", []) if not faite(par_id[d])]
            if bloquantes:
                print(f"RIEN : {t['id']} attend {', '.join(bloquantes)}")
                return
            print(f"{t['id']} | {t['type']} | {t['titre']} | brief : section « ## {t['id']} » de 02-plan-contenu.md")
            return
    print("RIEN : aucune tâche due")


def dues():
    aujourd = date.today(); rien = True
    for t in taches():
        # Tâches hors site dont la date est passée
        par_id = {x["id"]: x for x in taches()}
        pret = all(faite(par_id[d]) for d in t.get("depend_de", []))
        if t["type"] == "hors_site" and t["statut"] == "a_faire" and pret and jour(t["date"]) <= aujourd \
                and t["id"] not in branches_ouvertes():
            print(f"HORS_SITE {t['id']} | {t['titre']}"); rien = False
        # Mesures J+14 (indexation) et J+28 (performance) des pages publiées
        if t["type"] == "contenu" and t["statut"] == "publie" and t.get("publie_le"):
            age = (aujourd - jour(t["publie_le"])).days
            if age >= 28:
                print(f"MESURE_J28 {t['id']} | {', '.join(t.get('urls', []))}"); rien = False
            elif age >= 14 and not t.get("controle_j14"):
                print(f"CONTROLE_J14 {t['id']} | {', '.join(t.get('urls', []))}"); rien = False
    if rien: print("RIEN")


def derniere():
    """Dernière tâche terminée (publiée, mesurée, faite ou abandonnée) + dernière entrée du journal."""
    finies = [t for t in taches() if t.get("publie_le") or t.get("clos_le")]
    if not finies:
        print("Aucune tâche SEO terminée pour l'instant.")
    else:
        t = max(finies, key=lambda x: str(x.get("clos_le") or x.get("publie_le")))
        quand = t.get("clos_le") or t.get("publie_le")
        print(f"Dernière tâche SEO : {t['id']} — {t['titre']}")
        print(f"Statut : {t['statut']} (le {quand})")
        for u in t.get("urls", []): print(f"- {u}")
        for n in t.get("notes", [])[-2:]: print(f"Note : {n}")
    # Dernière entrée du journal (bloc commençant par « ### »)
    journal = os.path.join(os.path.dirname(FICHIER), "journal.md")
    if os.path.isfile(journal):
        blocs = open(journal, encoding="utf-8").read().split("\n### ")
        if len(blocs) > 1:
            print("\nJournal : ### " + blocs[-1].strip())
    prochaine_tache = next((t for t in taches() if t["statut"] == "a_faire" and t["type"] in ("contenu", "revue")), None)
    if prochaine_tache:
        print(f"\nProchaine : {prochaine_tache['id']} — {prochaine_tache['titre']} (à partir du {prochaine_tache['date']})")


def statut(ident, nouveau, jour_=None, note=None):
    entete, data = charger()
    t = next((x for x in data["taches"] if x["id"] == ident), None)
    if not t: sys.exit(f"ERREUR tâche {ident} inconnue")
    if nouveau == "controle_j14":  # marqueur, pas un statut
        t["controle_j14"] = str(jour_ or date.today())
    else:
        if nouveau not in STATUTS[t["type"]]: sys.exit(f"ERREUR statut {nouveau} invalide pour {t['type']} : {STATUTS[t['type']]}")
        t["statut"] = nouveau
        if nouveau == "publie": t["publie_le"] = str(jour_ or date.today())
        if nouveau in ("mesure", "fait", "abandonne"): t["clos_le"] = str(jour_ or date.today())
    if note: t.setdefault("notes", []).append(f"{date.today()} : {note}")
    sauver(entete, data)
    print(f"OK {ident} -> {nouveau}")


if __name__ == "__main__":
    a = sys.argv[1:]
    def opt(nom):
        if nom in a:
            i = a.index(nom); v = a[i + 1]; del a[i:i + 2]; return v
    d = opt("--date"); n = opt("--note")
    if a[:1] == ["etat"]: etat()
    elif a[:1] == ["prochaine"]: prochaine()
    elif a[:1] == ["dues"]: dues()
    elif a[:1] == ["derniere"]: derniere()
    elif a[:1] == ["statut"] and len(a) == 3: statut(a[1], a[2], d, n)
    else: print(__doc__); sys.exit(2)
