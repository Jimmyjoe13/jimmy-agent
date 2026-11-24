# -*- coding: utf-8 -*-
"""Vérifications automatiques des pages du site jimy-agent.me (bot SEO).

Usage :
  python3 verifier.py page <fichier.html> [<fichier.html> ...] [--mot-cle "requête"]
  python3 verifier.py site

Chemins : relatifs à la racine du dépôt (ex. site-jimy/guides/x.html) ou absolus.
Sortie : une ligne par contrôle (ERREUR / ALERTE / OK) ; code de sortie 1 s'il y a
au moins une ERREUR. Une page ne se publie qu'avec 0 ERREUR (voir 04-procedures.md).
Aucune dépendance hors bibliothèque standard.
"""
import json, os, re, sys
from html.parser import HTMLParser

sys.stdout.reconfigure(encoding="utf-8")

# Racine du dépôt = deux dossiers au-dessus de docs/seo/outils
DEPOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", ".."))
SITE = os.path.join(DEPOT, "site-jimy")
DOMAINE = "https://jimy-agent.me"
GA4_ID = "G-FK2NSSHQWM"
TRACKING = "/assets/tracking.js?v=2"

# Pages « guide » : soumises aux contrôles de contenu (longueur, questions, FAQ…)
PREFIXES_GUIDE = ("guides/", "en/guides/", "comparatif/", "en/compare/")
# Pages hors contrôle de contenu (légales, remerciement, hubs, accueils, notes de version)
HUBS = ("guides/index.html", "en/guides/index.html")

# Expressions interdites (charte §4) : insensible à la casse
INTERDITS = [
    r"100\s?%\s?hors[\s-]?ligne", r"100\s?%\s?offline", r"fully offline", r"enti[eè]rement hors[\s-]?ligne",
    r"jarvis officiel", r"official jarvis", r"sans cl[ée] d'?api", r"no api key", r"aucune cl[ée]",
    r"\{\{", r"\bTODO\b", r"lorem ipsum", r"\[à compléter\]", r"\[to be completed\]",
]


class Analyse(HTMLParser):
    """Collecte les éléments utiles d'une page en une passe."""

    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.title = ""; self.meta = {}; self.links_rel = []; self.lang = None
        self.h1 = []; self.h2 = []          # h2 : dicts {texte, reponse}
        self.hrefs = []; self.download = 0; self.scripts_src = []; self.jsonld = []
        self.imgs_sans_alt = 0; self.texte = []
        self._pile = None; self._buf = []; self._dans_jsonld = False; self._ignorer = 0
        self._attente_reponse = None        # dernier h2 en attente de son <p class="reponse">

    def handle_starttag(self, tag, attrs):
        a = dict(attrs)
        if tag == "html": self.lang = a.get("lang")
        elif tag == "meta":
            cle = a.get("name") or a.get("property")
            if cle: self.meta[cle] = a.get("content", "")
        elif tag == "link": self.links_rel.append(a)
        elif tag == "a":
            if a.get("href"): self.hrefs.append(a["href"])
            if a.get("data-track") == "download": self.download += 1
        elif tag == "img" and not a.get("alt"): self.imgs_sans_alt += 1
        elif tag == "script":
            if a.get("src"): self.scripts_src.append(a["src"])
            if a.get("type") == "application/ld+json": self._dans_jsonld = True; self._buf = []
            else: self._ignorer += 1
        elif tag == "style": self._ignorer += 1
        if tag in ("title", "h1", "h2") or (tag == "p" and "reponse" in (a.get("class") or "").split()):
            self._pile = tag if tag != "p" else "reponse"; self._buf = []

    def handle_endtag(self, tag):
        if tag == "script":
            if self._dans_jsonld: self.jsonld.append("".join(self._buf)); self._dans_jsonld = False
            elif self._ignorer: self._ignorer -= 1
            return
        if tag == "style" and self._ignorer: self._ignorer -= 1; return
        if self._pile and tag == ("p" if self._pile == "reponse" else self._pile):
            t = re.sub(r"\s+", " ", "".join(self._buf)).strip()
            if self._pile == "title": self.title = t
            elif self._pile == "h1": self.h1.append(t)
            elif self._pile == "h2":
                d = {"texte": t, "reponse": None}; self.h2.append(d)
                self._attente_reponse = d if t.endswith("?") else None
            elif self._pile == "reponse" and self._attente_reponse is not None:
                self._attente_reponse["reponse"] = t; self._attente_reponse = None
            self._pile = None

    def handle_data(self, data):
        if self._dans_jsonld or self._pile: self._buf.append(data)
        if not self._dans_jsonld and not self._ignorer: self.texte.append(data)


def chemin_relatif(f):
    """Chemin de la page relatif à site-jimy, avec des /."""
    return os.path.relpath(os.path.abspath(f), SITE).replace("\\", "/")


def url_de(rel):
    """URL publique attendue d'un fichier du site."""
    if rel == "index.html": return DOMAINE + "/"
    if rel.endswith("/index.html"): return DOMAINE + "/" + rel[: -len("index.html")]
    return DOMAINE + "/" + rel


def fichier_de(href, base_rel):
    """Fichier du site visé par un lien interne, ou None si lien externe."""
    h = href.split("#")[0].split("?")[0]
    if not h: return "ANCRE"
    if h.startswith(DOMAINE): h = h[len(DOMAINE):] or "/"
    elif h.startswith(("http://", "https://", "mailto:", "tel:", "javascript:")): return None
    if not h.startswith("/"):  # lien relatif
        h = "/" + os.path.normpath(os.path.join(os.path.dirname(base_rel), h)).replace("\\", "/")
    p = h.lstrip("/")
    if p == "" or p.endswith("/"): p += "index.html"
    elif "." not in os.path.basename(p): p += "/index.html"  # /en → en/index.html
    return p


def analyser(f):
    a = Analyse()
    with open(f, encoding="utf-8") as fh: a.feed(fh.read())
    return a


def blocs_jsonld(a, rapport):
    types = []
    for bloc in a.jsonld:
        try:
            d = json.loads(bloc)
        except Exception as e:
            rapport("ERREUR", f"JSON-LD invalide : {str(e)[:80]}"); continue
        for item in (d if isinstance(d, list) else [d]):
            types.append(item.get("@type"))
            if item.get("@type") in ("TechArticle", "Article", "BlogPosting"):
                for k in ("datePublished", "dateModified", "headline"):
                    if not item.get(k): rapport("ERREUR", f"{item['@type']} sans « {k} »")
    return types


def verifier_page(f, mot_cle=None):
    rel = chemin_relatif(f)
    resultats = []
    def rapport(niveau, msg): resultats.append((niveau, msg))
    if not os.path.isfile(f):
        print(f"\n## {rel}\nERREUR fichier introuvable"); return 1
    a = analyser(f)
    attendu = url_de(rel)
    guide = rel.startswith(PREFIXES_GUIDE) and rel not in HUBS
    langue = "en" if rel.startswith("en/") else "fr"

    # Balises de tête
    n = len(a.title)
    if not 30 <= n <= 60: rapport("ERREUR", f"title {n} caractères (30 à 60) : {a.title}")
    d = a.meta.get("description", "")
    if not 120 <= len(d) <= 155: rapport("ERREUR", f"meta description {len(d)} caractères (120 à 155)")
    canon = [l.get("href") for l in a.links_rel if l.get("rel") == "canonical"]
    if canon != [attendu]: rapport("ERREUR", f"canonical {canon} au lieu de {attendu}")
    if a.lang != langue: rapport("ERREUR", f"<html lang> = {a.lang}, attendu {langue}")
    if a.meta.get("og:url") != attendu: rapport("ERREUR", "og:url différent du canonical")
    if not a.meta.get("og:title"): rapport("ERREUR", "og:title manquant")
    if a.meta.get("robots", "").find("noindex") >= 0 and guide: rapport("ERREUR", "page guide en noindex")

    # Titres
    if len(a.h1) != 1: rapport("ERREUR", f"{len(a.h1)} balise(s) H1 (exactement 1)")

    # hreflang : soi-même + x-default + réciprocité
    alts = {l.get("hreflang"): l.get("href") for l in a.links_rel if l.get("rel") == "alternate" and l.get("hreflang")}
    if alts:
        if alts.get(langue) != attendu: rapport("ERREUR", f"hreflang {langue} ne pointe pas sur la page elle-même")
        if "x-default" not in alts: rapport("ERREUR", "hreflang x-default manquant")
        autre = "fr" if langue == "en" else "en"
        if autre in alts:
            cible = fichier_de(alts[autre], rel)
            fc = os.path.join(SITE, cible) if cible else None
            if not fc or not os.path.isfile(fc): rapport("ERREUR", f"hreflang {autre} vers une page absente : {alts[autre]}")
            else:
                b = analyser(fc)
                retour = {l.get("hreflang"): l.get("href") for l in b.links_rel if l.get("rel") == "alternate"}
                if retour.get(langue) != attendu: rapport("ERREUR", f"hreflang non réciproque depuis {cible}")
    elif guide:
        rapport("ERREUR", "aucun hreflang (chaque guide a sa version FR et EN)")

    # Données structurées
    types = blocs_jsonld(a, rapport)
    if guide:
        for t in ("BreadcrumbList", "FAQPage"):
            if t not in types: rapport("ERREUR", f"JSON-LD {t} manquant")
        if not set(types) & {"TechArticle", "Article", "BlogPosting"}: rapport("ERREUR", "JSON-LD Article/TechArticle manquant")

    # Mesure et conversion
    html = open(f, encoding="utf-8").read()
    if GA4_ID not in html: rapport("ERREUR", f"ID GA4 {GA4_ID} absent")
    if TRACKING not in a.scripts_src: rapport("ERREUR", f"script {TRACKING} absent")
    if a.download == 0 and guide: rapport("ERREUR", "aucun bouton de téléchargement data-track=\"download\"")

    # Liens internes cassés
    for h in sorted(set(a.hrefs)):
        cible = fichier_de(h, rel)
        if cible and cible != "ANCRE" and not os.path.isfile(os.path.join(SITE, cible)):
            rapport("ERREUR", f"lien interne cassé : {h}")
    if a.imgs_sans_alt: rapport("ERREUR", f"{a.imgs_sans_alt} image(s) sans alt")

    # Contenu (guides uniquement)
    texte = re.sub(r"\s+", " ", " ".join(a.texte))
    mots = len(texte.split())
    if guide:
        if mots < 900: rapport("ERREUR", f"{mots} mots (minimum 900)")
        questions = [h for h in a.h2 if h["texte"].endswith("?")]
        if len(questions) < 4: rapport("ERREUR", f"{len(questions)} H2 en question (minimum 4)")
        for q in questions:
            if not q["reponse"]: rapport("ERREUR", f"H2 sans <p class=\"reponse\"> juste après : {q['texte'][:60]}")
            else:
                nm = len(q["reponse"].split())
                if not 35 <= nm <= 70: rapport("ERREUR", f"réponse directe de {nm} mots (35 à 70) : {q['texte'][:60]}")
    for motif in INTERDITS:
        m = re.search(motif, html, re.I)
        if m: rapport("ERREUR", f"expression interdite : « {m.group(0)} »")

    # Mot-clé principal (alerte seulement)
    if mot_cle:
        mc = mot_cle.lower()
        debut = " ".join(texte.split()[:150]).lower()
        for nom, val in (("title", a.title), ("H1", " ".join(a.h1)), ("150 premiers mots", debut)):
            mots_cle = [w for w in re.split(r"\W+", mc) if len(w) > 2]
            if not all(w in val.lower() for w in mots_cle): rapport("ALERTE", f"mot-clé « {mot_cle} » incomplet dans {nom}")

    print(f"\n## {rel} ({mots} mots)")
    for niveau, msg in resultats: print(f"{niveau} {msg}")
    nb = sum(1 for n_, _ in resultats if n_ == "ERREUR")
    print("OK aucune erreur" if nb == 0 else f"=> {nb} ERREUR(S)")
    return 1 if nb else 0


def pages_du_site():
    for racine, dossiers, fichiers in os.walk(SITE):
        dossiers[:] = [d for d in dossiers if d not in ("api", "assets", "analytics-credentials", ".vercel", "node_modules")]
        for x in fichiers:
            if x.endswith(".html"): yield os.path.relpath(os.path.join(racine, x), SITE).replace("\\", "/")


def verifier_site():
    erreurs = 0
    def out(niveau, msg):
        nonlocal erreurs
        erreurs += niveau == "ERREUR"; print(f"{niveau} {msg}")
    print("## Site")
    sm = open(os.path.join(SITE, "sitemap.xml"), encoding="utf-8").read()
    locs = re.findall(r"<loc>(.*?)</loc>", sm)
    urls_pages = [u for u in locs if not re.search(r"\.(png|jpe?g|webp|gif|svg)$", u)]
    # S1 : chaque URL du sitemap existe
    for u in urls_pages:
        c = fichier_de(u, "index.html")
        if not c or not os.path.isfile(os.path.join(SITE, c)): out("ERREUR", f"sitemap : {u} sans fichier")
    # Redirections déclarées (on ne doit plus lier ces URL)
    vj = json.load(open(os.path.join(SITE, "vercel.json"), encoding="utf-8"))
    redirigees = {r["source"] for r in vj.get("redirects", []) if "has" not in r and "(" not in r["source"]}
    pages = list(pages_du_site())
    entrants = {p: set() for p in pages}
    for p in pages:
        a = analyser(os.path.join(SITE, p))
        noindex = "noindex" in a.meta.get("robots", "")
        # S2 : toute page indexable est dans le sitemap
        if not noindex and url_de(p) not in urls_pages: out("ERREUR", f"{p} absente du sitemap")
        for h in set(a.hrefs):
            c = fichier_de(h, p)
            if c and c != "ANCRE":
                if c in entrants and c != p: entrants[c].add(p)
                # Chemin tel qu'écrit dans le lien (sans domaine, ancre ni paramètres)
                brut = h.split("#")[0].split("?")[0]
                if brut.startswith(DOMAINE): brut = brut[len(DOMAINE):] or "/"
                if brut in redirigees: out("ERREUR", f"{p} lie une URL redirigée : {h}")
    # S3 : maillage entrant des guides
    for p in pages:
        if p.startswith(PREFIXES_GUIDE) and p not in HUBS:
            n = len(entrants[p])
            if n < 2: out("ERREUR", f"{p} : {n} page(s) la lient (minimum 2, objectif 3)")
            elif n < 3: out("ALERTE", f"{p} : {n} pages la lient (objectif 3)")
    # S4 : les hubs listent tous les guides de leur langue
    for hub, prefixes in (("guides/index.html", ("guides/", "comparatif/")), ("en/guides/index.html", ("en/guides/", "en/compare/"))):
        if not os.path.isfile(os.path.join(SITE, hub)):
            out("ALERTE", f"hub {hub} absent"); continue
        liens = {fichier_de(h, hub) for h in analyser(os.path.join(SITE, hub)).hrefs}
        for p in pages:
            if p.startswith(prefixes) and p not in HUBS and p not in liens: out("ERREUR", f"hub {hub} ne liste pas {p}")
    print("OK site sans erreur" if erreurs == 0 else f"=> {erreurs} ERREUR(S)")
    return 1 if erreurs else 0


if __name__ == "__main__":
    args = sys.argv[1:]
    if not args or args[0] not in ("page", "site"):
        print(__doc__); sys.exit(2)
    if args[0] == "site": sys.exit(verifier_site())
    mot_cle = None
    if "--mot-cle" in args:
        i = args.index("--mot-cle"); mot_cle = args[i + 1]; del args[i:i + 2]
    code = 0
    for f in args[1:]:
        f = f if os.path.isabs(f) else os.path.join(DEPOT, f)
        code |= verifier_page(f, mot_cle)
    sys.exit(code)
