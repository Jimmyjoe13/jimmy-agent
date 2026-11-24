# -*- coding: utf-8 -*-
"""Données Search Console + GA4 pour jimy-agent.me (bot SEO).

Usage :
  python3 seo_data.py rapport [--jours 7]       rapport de période vs période précédente (Markdown)
  python3 seo_data.py page <URL> [--jours 28]   métriques d'une page + déclencheurs R1-R5 (06-mesure.md)
  python3 seo_data.py inspecter <URL> [<URL>…]  état d'indexation Google de chaque URL
  python3 seo_data.py sitemap                   (re)soumet https://jimy-agent.me/sitemap.xml

Clé du compte de service : variable JIMY_SEO_GOOGLE_KEY, sinon ~/.secrets/jimy_seo_google.json.
Les données Search Console ont ~3 jours de retard : la période se termine à J-3.
"""
import json, os, re, sys, warnings
from datetime import date, timedelta
from urllib.parse import quote

warnings.filterwarnings("ignore")
sys.stdout.reconfigure(encoding="utf-8")

SITE = "sc-domain:jimy-agent.me"
PROP = "properties/558505422"
SITEMAP = "https://jimy-agent.me/sitemap.xml"
MARQUE = re.compile(r"\bjim+y\b|jiminy|jimy", re.I)  # requêtes de marque, exclues du « hors marque »
CLE = os.environ.get("JIMY_SEO_GOOGLE_KEY") or os.path.expanduser("~/.secrets/jimy_seo_google.json")


def services():
    from google.oauth2 import service_account
    from googleapiclient.discovery import build
    from google.analytics.data_v1beta import BetaAnalyticsDataClient
    if not os.path.isfile(CLE):
        sys.exit(f"ERREUR clé introuvable : {CLE}")
    c = service_account.Credentials.from_service_account_file(CLE, scopes=[
        "https://www.googleapis.com/auth/webmasters",
        "https://www.googleapis.com/auth/analytics.readonly"])
    return build("searchconsole", "v1", credentials=c, cache_discovery=False), BetaAnalyticsDataClient(credentials=c)


def periodes(jours):
    fin = date.today() - timedelta(days=3)
    debut = fin - timedelta(days=jours - 1)
    return (debut, fin), (debut - timedelta(days=jours), debut - timedelta(days=1))


def gsc(svc, per, dims, filtre_page=None, limite=250):
    body = {"startDate": str(per[0]), "endDate": str(per[1]), "dimensions": dims, "rowLimit": limite}
    if filtre_page:
        body["dimensionFilterGroups"] = [{"filters": [{"dimension": "page", "operator": "equals", "expression": filtre_page}]}]
    return svc.searchanalytics().query(siteUrl=SITE, body=body).execute().get("rows", [])


def total(rows):
    c = sum(r["clicks"] for r in rows); i = sum(r["impressions"] for r in rows)
    pos = sum(r["position"] * r["impressions"] for r in rows) / i if i else 0
    return c, i, (c / i if i else 0), pos


def var(a, b):
    return "n.d." if not b else f"{(a - b) / b * 100:+.0f} %"


def ga4(client, per, dims, mets, filtre_canal=None, limite=50):
    from google.analytics.data_v1beta.types import (RunReportRequest, Dimension, Metric, DateRange,
                                                    FilterExpression, Filter)
    req = RunReportRequest(property=PROP, dimensions=[Dimension(name=d) for d in dims],
                           metrics=[Metric(name=m) for m in mets],
                           date_ranges=[DateRange(start_date=str(per[0]), end_date=str(per[1]))], limit=limite)
    if filtre_canal:
        req.dimension_filter = FilterExpression(filter=Filter(field_name="sessionDefaultChannelGroup",
                                                              string_filter=Filter.StringFilter(value=filtre_canal)))
    r = client.run_report(req)
    return [([v.value for v in x.dimension_values], [float(v.value) for v in x.metric_values]) for x in r.rows]


def rapport(jours):
    svc, ga = services()
    cur, prev = periodes(jours)
    print(f"# Rapport SEO jimy-agent.me — {cur[0]} → {cur[1]} ({jours} j, comparé à {prev[0]} → {prev[1]})\n")
    # Search Console : toutes requêtes (lignes par requête pour séparer marque / hors marque)
    q_cur = gsc(svc, cur, ["query"], limite=1000); q_prev = gsc(svc, prev, ["query"], limite=1000)
    t_cur = total(gsc(svc, cur, ["date"])); t_prev = total(gsc(svc, prev, ["date"]))
    hm_cur = total([r for r in q_cur if not MARQUE.search(r["keys"][0])])
    hm_prev = total([r for r in q_prev if not MARQUE.search(r["keys"][0])])
    print("## Search Console")
    print(f"- Clics : {t_cur[0]:.0f} ({var(t_cur[0], t_prev[0])}) · Impressions : {t_cur[1]:.0f} ({var(t_cur[1], t_prev[1])})"
          f" · CTR : {t_cur[2]*100:.1f} % · Position moy. : {t_cur[3]:.1f}")
    print(f"- Hors marque : {hm_cur[0]:.0f} clics ({var(hm_cur[0], hm_prev[0])}), {hm_cur[1]:.0f} impressions ({var(hm_cur[1], hm_prev[1])})")
    if t_prev[1] >= 50 and t_cur[1] < t_prev[1] * 0.5:
        print("- ⚠️ ALERTE : impressions en baisse de plus de 50 % (voir 05-garde-fous.md)")
    print("\n### Top requêtes hors marque")
    for r in sorted([r for r in q_cur if not MARQUE.search(r["keys"][0])], key=lambda r: -r["impressions"])[:15]:
        print(f"- « {r['keys'][0]} » : {r['impressions']:.0f} impr., {r['clicks']:.0f} clics, pos. {r['position']:.1f}")
    anciennes = {r["keys"][0] for r in q_prev}
    nouvelles = [r for r in q_cur if r["keys"][0] not in anciennes and not MARQUE.search(r["keys"][0])]
    if nouvelles:
        print("\n### Nouvelles requêtes hors marque (absentes de la période précédente)")
        for r in sorted(nouvelles, key=lambda r: -r["impressions"])[:10]:
            print(f"- « {r['keys'][0]} » : {r['impressions']:.0f} impr., pos. {r['position']:.1f}")
    print("\n### Pages")
    for r in sorted(gsc(svc, cur, ["page"]), key=lambda r: -r["impressions"])[:15]:
        print(f"- {r['keys'][0]} : {r['impressions']:.0f} impr., {r['clicks']:.0f} clics, CTR {r['ctr']*100:.1f} %, pos. {r['position']:.1f}")
    # Déclencheur R3 sur l'ensemble des requêtes
    r3 = [r for r in q_cur if r["impressions"] >= 20 and not MARQUE.search(r["keys"][0])]
    if r3:
        print("\n### Déclencheur R3 (requête ≥ 20 impressions : vérifier qu'une page la cible, sinon backlog)")
        for r in r3: print(f"- « {r['keys'][0]} » ({r['impressions']:.0f} impr.)")
    # GA4
    print("\n## GA4")
    for canal in (None, "Organic Search"):
        lignes = ga4(ga, cur, [], ["sessions", "totalUsers", "keyEvents"], canal)
        lp = ga4(ga, prev, [], ["sessions", "totalUsers", "keyEvents"], canal)
        s, u, k = lignes[0][1] if lignes else (0, 0, 0)
        sp = lp[0][1][0] if lp else 0
        print(f"- {'Recherche organique' if canal else 'Tout le trafic'} : {s:.0f} sessions ({var(s, sp)}), {u:.0f} utilisateurs, {k:.0f} événements clés")
    print("\n### Téléchargements (download_installer) par canal")
    from google.analytics.data_v1beta.types import RunReportRequest, Dimension, Metric, DateRange, FilterExpression, Filter
    req = RunReportRequest(property=PROP, dimensions=[Dimension(name="sessionDefaultChannelGroup"), Dimension(name="landingPage")],
                           metrics=[Metric(name="eventCount")],
                           date_ranges=[DateRange(start_date=str(cur[0]), end_date=str(cur[1]))],
                           dimension_filter=FilterExpression(filter=Filter(field_name="eventName",
                                                             string_filter=Filter.StringFilter(value="download_installer"))))
    lignes = ga.run_report(req).rows
    if not lignes: print("- aucun")
    for x in lignes:
        print(f"- {x.dimension_values[0].value} · arrivée sur {x.dimension_values[1].value} : {x.metric_values[0].value}")
    print("\n### Pages d'arrivée (recherche organique)")
    for d, m in ga4(ga, cur, ["landingPage"], ["sessions", "engagementRate"], "Organic Search")[:10]:
        print(f"- {d[0]} : {m[0]:.0f} sessions, engagement {m[1]*100:.0f} %")
    print("\nNote : GA4 contient du trafic robot (Hong Kong / Chine, langue chinoise) ; ne pas l'interpréter comme humain.")


def page(url, jours):
    svc, ga = services()
    cur, prev = periodes(jours)
    rows = gsc(svc, cur, ["query"], filtre_page=url, limite=500)
    # Totaux au niveau page : les lignes par requête excluent les requêtes anonymisées par Google
    c, i, ctr, pos = total(gsc(svc, cur, ["page"], filtre_page=url))
    print(f"# Page {url} — {cur[0]} → {cur[1]} ({jours} j)\n")
    print(f"- Impressions : {i:.0f} · Clics : {c:.0f} · CTR : {ctr*100:.1f} % · Position moy. : {pos:.1f}")
    chemin = re.sub(r"^https://jimy-agent\.me", "", url) or "/"
    org = ga4(ga, cur, ["landingPage"], ["sessions", "keyEvents"], "Organic Search", limite=200)
    s, k = next((m for d, m in org if d[0] == chemin), (0, 0))
    print(f"- GA4 organique (arrivée sur la page) : {s:.0f} sessions, {k:.0f} événements clés")
    print("\n## Requêtes")
    for r in sorted(rows, key=lambda r: -r["impressions"])[:20]:
        print(f"- « {r['keys'][0]} » : {r['impressions']:.0f} impr., {r['clicks']:.0f} clics, pos. {r['position']:.1f}")
    print("\n## Déclencheurs (06-mesure.md)")
    vus = False
    if i >= 100 and ctr < 0.015:
        print("- R1 : ≥ 100 impressions et CTR < 1,5 % → proposer un nouveau title + meta description"); vus = True
    for r in rows:
        if r["impressions"] >= 30 and 8 <= r["position"] <= 20:
            print(f"- R2 : « {r['keys'][0]} » ({r['impressions']:.0f} impr., pos. {r['position']:.1f}) → proposer une section dédiée"); vus = True
    if i == 0:
        print("- R4 : 0 impression → vérifier indexation, maillage entrant et mot-clé (suggestions.py)"); vus = True
    if s >= 20 and k == 0:
        print("- R5 : ≥ 20 sessions organiques sans téléchargement → proposer un meilleur appel à l'action"); vus = True
    if not vus: print("- aucun")


def inspecter(urls):
    svc, _ = services()
    for u in urls:
        try:
            r = svc.urlInspection().index().inspect(body={"inspectionUrl": u, "siteUrl": SITE}).execute()
            s = r["inspectionResult"]["indexStatusResult"]
            print(f"{u} | {s.get('verdict')} | {s.get('coverageState')} | dernier crawl : {s.get('lastCrawlTime', '-')}")
        except Exception as e:
            print(f"{u} | ERREUR | {str(e)[:150]}")


def sitemap():
    svc, _ = services()
    svc.sitemaps().submit(siteUrl=SITE, feedpath=SITEMAP).execute()
    s = svc.sitemaps().get(siteUrl=SITE, feedpath=SITEMAP).execute()
    print(f"OK sitemap soumis · dernière soumission {s.get('lastSubmitted')} · erreurs {s.get('errors')} · avertissements {s.get('warnings')}")


if __name__ == "__main__":
    a = sys.argv[1:]
    def opt(nom, defaut):
        if nom in a:
            i = a.index(nom); v = int(a[i + 1]); del a[i:i + 2]; return v
        return defaut
    if not a: print(__doc__); sys.exit(2)
    cmd = a.pop(0)
    if cmd == "rapport": rapport(opt("--jours", 7))
    elif cmd == "page" and a: j = opt("--jours", 28); page(a[0], j)
    elif cmd == "inspecter" and a: inspecter(a)
    elif cmd == "sitemap": sitemap()
    else: print(__doc__); sys.exit(2)
