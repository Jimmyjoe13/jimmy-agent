# -*- coding: utf-8 -*-
"""Squelette automatique de JEVA.

Entrée : le .glb (TRELLIS). Sortie : un fichier `.rig` binaire lu par Godot
(jimmy.gd) : en-tête JSON (os : nom, parent, origine ; nombre de sommets),
puis positions (float32 x3), os (int32 x4) et poids (float32 x4) par sommet
unique. Godot retrouve ses sommets par leur position (l'ordre peut différer).

Méthode : os du corps posés sur les repères mesurés (coupes par tranches),
chaque os porte une capsule (segment + rayon) ; un sommet est rattaché aux
os dont il est le plus près, distance rapportée au rayon. Les sommets loin
de toute capsule sont des câbles : ils reçoivent des chaînes d'os tracées
d'après leur vraie forme (secteurs angulaires autour d'un point d'attache
dans le dos, centres moyens par tranche de distance).

Usage : python autorig.py <modele.glb> <sortie.rig> [dossier_vues]
"""
import json, struct, sys
import numpy as np
import trimesh
from PIL import Image

glb, out_rig = sys.argv[1], sys.argv[2]
views = sys.argv[3] if len(sys.argv) > 3 else None
mesh = trimesh.load(glb, process=False).to_geometry()
V = np.asarray(mesh.vertices, dtype=np.float64)

# ── Os du corps : (nom, parent, origine, capsule a, capsule b, rayon) ─────
# Repères de JEVA (unités du .glb, mesurés par coupes : voir HANDOFF). Pour
# un autre modèle, les remesurer : les noms d'os, eux, sont attendus tels
# quels par jimmy.gd (`_drive_rig`). Suffixe _n : côté x < 0, _p : x > 0.
P = lambda *a: np.array(a, dtype=np.float64)
BODY = [
    ("hips",       None,        P(0, 0.02, 0.015),    P(0, -0.01, 0.02),   P(0, 0.10, 0.0),     0.095),
    ("spine",      "hips",      P(0, 0.12, 0.0),      P(0, 0.12, 0.0),     P(0, 0.20, 0.01),    0.090),
    ("chest",      "spine",     P(0, 0.22, 0.01),     P(0, 0.22, 0.01),    P(0, 0.31, -0.01),   0.100),
    ("neck",       "chest",     P(0, 0.335, -0.005),  P(0, 0.33, -0.005),  P(0, 0.375, 0.02),   0.040),
    ("head",       "neck",      P(0, 0.375, 0.03),    P(0, 0.415, 0.035),  P(0, 0.46, 0.035),   0.068),
    ("upperarm_n", "chest",     P(-0.105, 0.295, -0.005), P(-0.105, 0.295, -0.005), P(-0.137, 0.150, -0.015), 0.036),
    ("forearm_n",  "upperarm_n", P(-0.137, 0.150, -0.015), P(-0.137, 0.150, -0.015), P(-0.183, 0.045, 0.008), 0.032),
    ("hand_n",     "forearm_n", P(-0.183, 0.045, 0.008), P(-0.183, 0.045, 0.008), P(-0.212, -0.030, 0.022), 0.036),
    ("upperarm_p", "chest",     P(0.098, 0.275, 0.0),  P(0.098, 0.275, 0.0),  P(0.148, 0.182, 0.035), 0.036),
    ("forearm_p",  "upperarm_p", P(0.148, 0.182, 0.035), P(0.148, 0.182, 0.035), P(0.188, 0.218, 0.160), 0.032),
    ("hand_p",     "forearm_p", P(0.188, 0.218, 0.160), P(0.188, 0.218, 0.160), P(0.240, 0.262, 0.215), 0.042),
    ("thigh_n",    "hips",      P(-0.048, -0.005, 0.025), P(-0.048, -0.005, 0.025), P(-0.037, -0.170, 0.045), 0.052),
    ("shin_n",     "thigh_n",   P(-0.037, -0.170, 0.045), P(-0.037, -0.170, 0.045), P(-0.020, -0.395, -0.028), 0.040),
    ("foot_n",     "shin_n",    P(-0.020, -0.395, -0.028), P(-0.020, -0.395, -0.028), P(-0.015, -0.495, -0.010), 0.032),
    ("thigh_p",    "hips",      P(0.048, -0.005, 0.030), P(0.048, -0.005, 0.030), P(0.047, -0.170, 0.095), 0.052),
    ("shin_p",     "thigh_p",   P(0.047, -0.170, 0.095), P(0.047, -0.170, 0.095), P(0.036, -0.380, -0.033), 0.040),
    ("foot_p",     "shin_p",    P(0.036, -0.380, -0.033), P(0.036, -0.380, -0.033), P(0.015, -0.490, -0.008), 0.032),
]


def seg_dist(points, a, b):
    """Distance de chaque point au segment [a, b]."""
    ab = b - a
    t = np.clip(((points - a) @ ab) / max(ab @ ab, 1e-12), 0.0, 1.0)
    return np.linalg.norm(points - (a + t[:, None] * ab), axis=1)


body_scores = np.stack([seg_dist(V, a, b) / r for (_, _, _, a, b, r) in BODY], axis=1)
min_score = body_scores.min(axis=1)
# Câble : nettement hors de toute capsule du corps.
is_cable = min_score > 1.35
print(f"sommets {len(V)} — corps {np.sum(~is_cable)}, câbles {np.sum(is_cable)}")

# ── Chaînes des câbles ─────────────────────────────────────────────────────
bones = [(n, p, o) for (n, p, o, *_rest) in BODY]
bone_index = {n: i for i, (n, _, _) in enumerate(bones)}
cable_segments = []  # (indice d'os, a, b, rayon)
SECTORS = 5
D_BINS = [0.0, 0.09, 0.18, 0.27, 0.36, 0.60]
for side, sname in ((-1, "n"), (1, "p")):
    anchor = P(side * 0.07, 0.25, -0.06)
    sel = is_cable & (np.sign(V[:, 0]) == side)
    pts = V[sel]
    rel = pts[:, :2] - anchor[:2]
    d = np.linalg.norm(rel, axis=1)
    # Angle dans le plan de face, vers l'extérieur : 0 = horizontal, -90 = vers le bas.
    theta = np.degrees(np.arctan2(rel[:, 1], np.abs(rel[:, 0]) + 1e-9))
    edges = np.quantile(theta, np.linspace(0, 1, SECTORS + 1))
    for s in range(SECTORS):
        in_sector = (theta >= edges[s]) & (theta <= edges[s + 1])
        chain = [anchor]
        for k in range(1, len(D_BINS)):
            m = in_sector & (d >= D_BINS[k - 1]) & (d < D_BINS[k])
            if m.sum() >= 30:
                chain.append(pts[m].mean(axis=0))
        if len(chain) < 3:
            continue
        parent = "chest"
        for k in range(len(chain) - 1):
            name = f"cable_{sname}{s}_{k}"
            bones.append((name, parent, chain[k]))
            bone_index[name] = len(bones) - 1
            cable_segments.append((bone_index[name], chain[k], chain[k + 1], 0.05))
            parent = name
print(f"os : {len(bones)} (corps {len(BODY)}, câbles {len(bones) - len(BODY)})")

# ── Poids ────────────────────────────────────────────────────────────────
n = len(V)
B = np.zeros((n, 4), np.int32)
W = np.zeros((n, 4), np.float32)
body_ids = np.array([bone_index[b[0]] for b in BODY])
# Corps : 3 os les plus proches, poids ∝ score^-6 (transitions courtes aux
# jointures, pas de « mollesse » sur tout le membre).
order = np.argsort(body_scores, axis=1)[:, :3]
sc = np.take_along_axis(body_scores, order, axis=1)
w = 1.0 / np.maximum(sc, 0.05) ** 6
w /= w.sum(axis=1, keepdims=True)
B[:, :3] = body_ids[order]
W[:, :3] = w
# Tête rigide : le casque ne doit jamais se déformer (l'œil y est fixé).
head = bone_index["head"]
rigid_head = body_scores[:, head] < 1.15
B[rigid_head] = [head, 0, 0, 0]
W[rigid_head] = [1, 0, 0, 0]
# Câbles : 2 segments de câble les plus proches + l'os du tronc le plus proche
# (près de l'attache, le câble suit le dos).
if cable_segments:
    cpts = V[is_cable]
    cs = np.stack([seg_dist(cpts, a, b) / r for (_, a, b, r) in cable_segments], axis=1)
    cids = np.array([i for (i, *_r) in cable_segments])
    corder = np.argsort(cs, axis=1)[:, :2]
    csc = np.take_along_axis(cs, corder, axis=1)
    cw = 1.0 / np.maximum(csc, 0.05) ** 4
    cw /= cw.sum(axis=1, keepdims=True)
    Bc = np.zeros((len(cpts), 4), np.int32)
    Wc = np.zeros((len(cpts), 4), np.float32)
    Bc[:, :2] = cids[corder]
    Wc[:, :2] = cw
    B[is_cable] = Bc
    W[is_cable] = Wc

# ── Lissage des poids le long du maillage ────────────────────────────────
# Deux sommets voisins d'un même câble rattachés à des chaînes différentes
# déchirent la surface (plaques étirées). On diffuse les poids entre voisins
# (sommets confondus aux coutures UV fusionnés), davantage sur les câbles.
import scipy.sparse as sp
from scipy.sparse.csgraph import laplacian  # noqa: F401  (import de contrôle)
nb = len(bones)
dense = np.zeros((n, nb), np.float32)
np.add.at(dense, (np.repeat(np.arange(n), 4), B.ravel()), W.ravel())
_, uid = np.unique(np.round(V, 6), axis=0, return_inverse=True)
uid = uid.ravel()
nu = uid.max() + 1
faces = uid[np.asarray(mesh.faces)]
rows = np.concatenate([faces[:, 0], faces[:, 1], faces[:, 2], faces[:, 1], faces[:, 2], faces[:, 0]])
cols = np.concatenate([faces[:, 1], faces[:, 2], faces[:, 0], faces[:, 0], faces[:, 1], faces[:, 2]])
A = sp.coo_matrix((np.ones(len(rows), np.float32), (rows, cols)), shape=(nu, nu)).tocsr()
A.data[:] = 1.0
deg = np.asarray(A.sum(axis=1)).ravel()
Dinv = sp.diags(1.0 / np.maximum(deg, 1.0))
M = (Dinv @ A).tocsr()
# Poids par sommet unique (moyenne des doublons).
U = np.zeros((nu, nb), np.float32)
np.add.at(U, uid, dense)
U /= np.bincount(uid, minlength=nu)[:, None]
cable_u = np.zeros(nu, bool)
cable_u[uid[is_cable]] = True
head_u = np.zeros(nu, bool)
head_u[uid[rigid_head]] = True
# Masques : un câble ne suit que les câbles et le tronc ; un membre ne suit
# jamais un câble (sinon un bout de câble s'étire derrière une jambe).
names = [b[0] for b in bones]
trunk = {"hips", "spine", "chest"}
is_cable_bone = np.array([nm.startswith("cable_") for nm in names])
cable_allowed = is_cable_bone | np.array([nm in trunk for nm in names])
body_allowed = ~is_cable_bone
for it in range(14):
    S2 = M @ U
    # Câbles : lissage fort ; corps : seulement les 4 premières passes
    # (jointures adoucies sans que les membres « fondent »).
    blend = np.where(cable_u, 0.7, 0.5 if it < 4 else 0.0)[:, None]
    U = (1 - blend) * U + blend * S2
    U[head_u] = 0.0
    U[head_u, head] = 1.0
    U[cable_u] *= cable_allowed
    U[~cable_u] *= body_allowed
    U /= np.maximum(U.sum(axis=1, keepdims=True), 1e-8)
U /= U.sum(axis=1, keepdims=True)
dense = U[uid]
top = np.argsort(-dense, axis=1)[:, :4]
tw = np.take_along_axis(dense, top, axis=1)
tw /= tw.sum(axis=1, keepdims=True)
B = top.astype(np.int32)
W = tw.astype(np.float32)
print("poids lissés")

# ── Écriture ─────────────────────────────────────────────────────────────
header = {
    "bones": [{"name": nme, "parent": (bone_index[p] if p is not None else -1),
               "origin": [float(c) for c in o]} for (nme, p, o) in bones],
    "count": int(n),
}
raw = json.dumps(header).encode("utf-8")
with open(out_rig, "wb") as f:
    f.write(b"JRIG")
    f.write(struct.pack("<I", len(raw)))
    f.write(raw)
    f.write(V.astype("<f4").tobytes())
    f.write(B.astype("<i4").tobytes())
    f.write(W.astype("<f4").tobytes())
print(f"rig écrit : {out_rig}")

# ── Vues de contrôle : couleur = os dominant ─────────────────────────────
if views:
    rng = np.random.default_rng(7)
    palette = (rng.random((len(bones), 3)) * 200 + 55).astype(np.uint8)
    dom = B[:, 0]
    N, lo, hi = 900, -0.55, 0.55
    def px(a): return ((a - lo) / (hi - lo) * N).astype(int).clip(0, N - 1)
    for name, ax, ay, depth in (("os-face", V[:, 0], V[:, 1], V[:, 2]), ("os-profil", V[:, 2], V[:, 1], -np.abs(V[:, 0]))):
        img = np.zeros((N, N, 3), np.uint8)
        o = np.argsort(depth)
        img[px(-ay[o]), px(ax[o])] = palette[dom[o]]
        Image.fromarray(img).save(f"{views}/{name}.png")
    print("vues écrites")
