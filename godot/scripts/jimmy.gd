extends Node3D
class_name Jimmy
## Le renard humanoïde de Jimmy, construit de façon procédurale.
##
## Choix V1 : aucun modèle 3D externe. Le personnage est assemblé à partir de
## primitives Godot et animé par code. Avantages : zéro asset à maintenir,
## silhouette immédiatement modifiable, et coût de rendu très faible sur les
## trois profils graphiques. Si un skin plus riche est ajouté plus tard, il
## suffit de remplacer `_build_body()` par l'instanciation d'une scène.
##
## Hiérarchie :
##   Jimmy
##    └ Body            (respiration, balancement)
##       ├ LegL / LegR
##       ├ Torso         (torse, bras, tête, queue)
##       │  ├ ArmL / ArmR
##       │  ├ Neck → Head (crâne, museau, mâchoire, oreilles, yeux)
##       │  └ Tail1 → Tail2 → Tail3 → Tail4 (chaîne, remuage)
##       └ ...

const IDLE := "idle"
const LISTENING := "listening"
const THINKING := "thinking"
const SPEAKING := "speaking"

## Posture cible par état. Toute nouvelle entrée ajoute un état sans toucher
## au reste du code : c'est le point d'extension prévu par le PLAN.
## Réglage « gestes marqués » (oct. 2026) : les écarts idle→état sont ×2 à ×3
## par rapport à la V1 (quelques degrés = invisible à 4 m de caméra). Chaque
## valeur a été poussée jusqu'à être lisible en un coup d'œil sur snapshot.
const POSES := {
	## Convention bras : valeurs positives = vers l'extérieur, en miroir
	## (`_arm_l` nie, `_arm_r` garde — voir `_apply_pose`). Un `arm_r`
	## négatif envoie la main droite VERS le centre, à travers le torse
	## (même plan z = 0) : le bras disparaît dans le corps. Donc toujours
	## positif des deux côtés ; l'asymétrie se joue sur les montants.
	"idle": {
		"head_pitch": 0.0, "head_yaw": 0.0, "head_roll": 0.0,
		"body_pitch": 0.0, "body_yaw": 0.0,
		"arm_l": 0.10, "arm_r": 0.10,
		"jaw": 0.0, "ear": 0.0, "lean": 0.0, "bounce": 0.5,
	},
	"listening": {
		"head_pitch": -0.30, "head_yaw": 0.22, "head_roll": 0.20,
		"body_pitch": -0.08, "body_yaw": 0.0,
		"arm_l": 0.35, "arm_r": 0.35,
		"jaw": 0.06, "ear": -0.45, "lean": 0.22, "bounce": 1.1,
	},
	"thinking": {
		"head_pitch": -0.38, "head_yaw": -0.55, "head_roll": -0.10,
		"body_pitch": -0.06, "body_yaw": 0.12,
		"arm_l": 0.30, "arm_r": 0.45,
		"jaw": 0.0, "ear": 0.10, "lean": 0.08, "bounce": 0.5,
	},
	"speaking": {
		"head_pitch": 0.06, "head_yaw": 0.0, "head_roll": 0.0,
		"body_pitch": 0.04, "body_yaw": 0.0,
		"arm_l": 0.30, "arm_r": 0.55,
		"jaw": 0.75, "ear": 0.18, "lean": 0.20, "bounce": 1.3,
	},
	# États additionnels : bras en V franc et menton relevé (victoire lisible),
	# tête basse et oreilles plaquées (échec lisible). `success`/`error` sont
	# émis par les relais Tauri en fin de tâche outillée (sinon jamais vus).
	"executing": {
		"head_pitch": -0.12, "head_yaw": 0.28, "head_roll": 0.0,
		"body_pitch": 0.08, "body_yaw": -0.16,
		"arm_l": 1.10, "arm_r": 1.10,
		"jaw": 0.12, "ear": -0.25, "lean": 0.24, "bounce": 1.4,
	},
	"success": {
		"head_pitch": -0.18, "head_yaw": 0.0, "head_roll": 0.0,
		"body_pitch": -0.12, "body_yaw": 0.0,
		"arm_l": 1.90, "arm_r": 1.90,
		"jaw": 0.45, "ear": -0.45, "lean": -0.10, "bounce": 1.6,
	},
	"error": {
		"head_pitch": 0.45, "head_yaw": 0.0, "head_roll": 0.22,
		"body_pitch": 0.18, "body_yaw": 0.0,
		"arm_l": 0.45, "arm_r": 0.45,
		"jaw": 0.22, "ear": 0.60, "lean": 0.35, "bounce": 0.25,
	},
	"waiting": {
		"head_pitch": -0.10, "head_yaw": 0.35, "head_roll": -0.12,
		"body_pitch": 0.0, "body_yaw": 0.0,
		"arm_l": 0.18, "arm_r": 0.18,
		"jaw": 0.0, "ear": 0.0, "lean": 0.08, "bounce": 0.6,
	},
}

## Skins : une palette + paramètres de forme. Ajouter un skin = ajouter une
## entrée ici (et son libellé dans `providers::avatar::SKINS`, côté Rust, pour
## l'interface). Paramètres de forme :
##   `ear_scale`   — taille des oreilles (1 = normal)
##   `tail_scale`  — taille de la queue (0 = pas de queue)
##   `metallic`    — 0 = mat, 1 = métallique (robot)
##   `has_fur`     — 1 = fourrure (normal map + rim), 0 = surface lisse
const SKINS := {
	"renard": {
		"fur": Color(0.80, 0.42, 0.13), "cream": Color(0.95, 0.89, 0.79),
		"dark": Color(0.11, 0.08, 0.07), "shirt": Color(0.16, 0.36, 0.72),
		"accent": Color(0.95, 0.62, 0.16), "ear_scale": 1.0,
		"tail_scale": 1.0, "metallic": 0.0, "has_fur": 1.0,
	},
	"arctique": {
		"fur": Color(0.86, 0.88, 0.92), "cream": Color(0.99, 0.99, 1.0),
		"dark": Color(0.18, 0.20, 0.26), "shirt": Color(0.08, 0.46, 0.50),
		"accent": Color(0.36, 0.74, 0.94), "ear_scale": 0.9,
		"tail_scale": 1.0, "metallic": 0.0, "has_fur": 1.0,
	},
	"fennec": {
		"fur": Color(0.87, 0.70, 0.47), "cream": Color(0.97, 0.92, 0.82),
		"dark": Color(0.28, 0.18, 0.12), "shirt": Color(0.22, 0.46, 0.28),
		"accent": Color(0.93, 0.78, 0.30), "ear_scale": 1.55,
		"tail_scale": 1.0, "metallic": 0.0, "has_fur": 1.0,
	},
	"ours": {
		"fur": Color(0.35, 0.22, 0.12), "cream": Color(0.75, 0.65, 0.50),
		"dark": Color(0.15, 0.10, 0.08), "shirt": Color(0.25, 0.35, 0.45),
		"accent": Color(0.60, 0.45, 0.30), "ear_scale": 0.7,
		"tail_scale": 0.3, "metallic": 0.0, "has_fur": 1.0,
	},
	"robot": {
		"fur": Color(0.55, 0.58, 0.62), "cream": Color(0.75, 0.78, 0.82),
		"dark": Color(0.20, 0.22, 0.25), "shirt": Color(0.30, 0.35, 0.40),
		"accent": Color(0.20, 0.80, 0.90), "ear_scale": 0.4,
		"tail_scale": 0.0, "metallic": 0.9, "has_fur": 0.0,
	},
}
const SKIN_DEFAULT := "renard"

## Skins premium : données sous licence propriétaire, **hors du dépôt
## public** (dépôt privé `jimy-premium`, livrées dans `<données>/premium/`) ;
## ce code de chargement, lui, reste MIT. Un fichier JSON par skin
## (nom du fichier = identifiant), mêmes clés que `SKINS`, couleurs en
## [r, g, b]. Paramètres en plus : `glow` (émission, 0 = aucune),
## `outline` (couleur du contour, [r, g, b]) et `outline_scale` (épaisseur
## du contour, 1 = normal) — le contour lumineux fait le skin Néon.
## L'appli ne les propose pas encore : Tauri n'accepte que les skins de
## `providers::avatar::SKINS` (Rust) ; l'activation par licence viendra ensuite.
const SKIN_COLOR_KEYS := ["fur", "cream", "dark", "shirt", "accent"]
## Couleurs facultatives d'un skin premium (absentes = valeurs par défaut).
## `eye_ring` : anneau de l'œil, `rim` : liseré lumineux d'un modèle 3D.
const SKIN_OPTIONAL_COLOR_KEYS := ["outline", "eye_ring", "rim"]

## Skin premium à **modèle 3D** (`"model": "jeva.glb"` dans son JSON) : le
## fichier `.glb` n'est pas dans le dépôt public, il vit dans
## `<données>/premium/models/` (Tauri passe `--premium-dir`). Le modèle est
## figé dans sa pose ; l'animation garde les mêmes nœuds que le renard
## (torse, tête, bras, mâchoire vides) pour que `_apply_pose` serve tel quel,
## et l'œil est procédural, fixé sur la tête. Clés du JSON :
##   `model_height` (hauteur à l'écran), `model_lift` (lévitation),
##   `model_min_y` (bas du modèle, unités du .glb), `head_center` et
##   `eye_offset` (unités du .glb), `eye_radius`, `companion`.
## Liseré lumineux additif posé sur le modèle (aucun fichier : code GLSL).
const RIM_SHADER := """
shader_type spatial;
render_mode blend_add, unshaded, depth_draw_never, cull_back;
uniform vec4 rim_color : source_color = vec4(0.25, 0.85, 1.0, 1.0);
uniform float rim_power = 2.6;
uniform float rim_strength = 0.8;
void fragment() {
	float f = pow(1.0 - clamp(dot(NORMAL, VIEW), 0.0, 1.0), rim_power);
	ALBEDO = rim_color.rgb * f * rim_strength;
	ALPHA = f * rim_strength;
}
"""

## Tentacules d'un modèle mécanique (skin ARK), cible par état : enroulement
## (rad par os, positif = plus enroulé), pente vers le bas, amplitude et
## vitesse de l'onde qui court vers la pointe, part de visée vers
## l'utilisateur, ouverture des pinces (rad).
const TENTACLE_MOODS := {
	"idle": {"curl": 0.0, "droop": 0.0, "amp": 0.075, "speed": 1.3, "aim": 0.0, "grip": 0.12},
	"listening": {"curl": -0.03, "droop": -0.02, "amp": 0.055, "speed": 1.1, "aim": 1.0, "grip": 0.22},
	"thinking": {"curl": 0.07, "droop": 0.0, "amp": 0.05, "speed": 0.8, "aim": 0.0, "grip": 0.0},
	"speaking": {"curl": 0.0, "droop": -0.01, "amp": 0.10, "speed": 1.9, "aim": 0.5, "grip": 0.18},
	"executing": {"curl": 0.02, "droop": 0.0, "amp": 0.13, "speed": 2.6, "aim": 0.0, "grip": 0.25},
	"success": {"curl": -0.08, "droop": -0.05, "amp": 0.11, "speed": 2.4, "aim": 0.3, "grip": 0.35},
	"error": {"curl": 0.03, "droop": 0.09, "amp": 0.03, "speed": 0.7, "aim": 0.0, "grip": 0.30},
	"waiting": {"curl": 0.02, "droop": 0.0, "amp": 0.06, "speed": 1.0, "aim": 0.2, "grip": 0.10},
}
## Largeur d'une pupille fendue par état (skin premium `pupil_slit`) : elle
## s'ouvre pour écouter, se pince pour réfléchir ou en cas d'échec.
const PUPIL_SLIT := {"listening": 0.5, "speaking": 0.4, "thinking": 0.2, "error": 0.16}

## Halo lumineux derrière un modèle (clé `aura` du skin, couleur [r, g, b]) :
## disque toujours face à la caméra, qui s'efface vers le bord. Mélange alpha,
## pas additif : sur la fenêtre transparente, un ajout sur un fond d'alpha 0
## reste invisible (vérifié par capture).
const AURA_SHADER := """
shader_type spatial;
render_mode blend_mix, unshaded, depth_draw_never, cull_disabled, shadows_disabled;
uniform vec4 aura_color : source_color = vec4(0.3, 1.0, 0.4, 1.0);
uniform float strength = 0.5;
void vertex() {
	MODELVIEW_MATRIX = VIEW_MATRIX * mat4(INV_VIEW_MATRIX[0], INV_VIEW_MATRIX[1], INV_VIEW_MATRIX[2], MODEL_MATRIX[3]);
}
void fragment() {
	float f = pow(clamp(1.0 - length(UV - vec2(0.5)) * 2.0, 0.0, 1.0), 1.8);
	ALBEDO = aura_color.rgb;
	ALPHA = f * strength;
}
"""

## Hauteur de la tête au-dessus du cou. 0,42 donnait un cou de girafe : la
## tête est rapprochée du col pour une silhouette plus compacte.
const HEAD_Y := 0.30
## Épaisseur du contour (unités monde, ~1,5 px au cadrage actuel).
const OUTLINE_WIDTH := 0.0075
const OUTLINE_COLOR := Color(0.16, 0.08, 0.04)
## Vitesse de convergence vers la posture cible (plus élevé = plus réactif).
const POSE_SPEED := 7.0
## Amplitude du clignement des yeux.
const BLINK_PERIOD := 4.2

## Regard. La caméra tient lieu d'yeux de l'utilisateur : un personnage qui
## regarde la caméra semble regarder la personne devant l'écran, où qu'elle
## soit (effet Joconde). Avant, Jimmy suivait le curseur — et regardait de
## côté dès que la souris quittait sa fenêtre.
## Contact visuel par état (1 = yeux dans les yeux ; défaut 1).
const EYE_CONTACT := {"thinking": 0.0, "executing": 0.2, "error": 0.5}
## Où porte le regard quand il n'y a pas contact : (lacet, tangage) en radians,
## tangage positif = vers le bas. On lève les yeux pour réfléchir, on baisse
## les yeux sur son travail.
const GAZE_AWAY := {"thinking": Vector2(-0.12, -0.10), "executing": Vector2(0.15, 0.22)}
## Débattement de la tête vers l'utilisateur ; les yeux font le reste.
const GAZE_YAW_MAX := 0.6
const GAZE_PITCH_MAX := 0.45

var state: String = IDLE
## Skin courant. Positionné avant l'entrée dans l'arbre, ou via `set_skin`.
var skin: String = SKIN_DEFAULT
## Skins premium lus sur disque, chargés au premier besoin : `set_skin` peut
## être appelé avant l'entrée dans l'arbre.
var _premium_cache: Dictionary = {}
var _premium_loaded := false
## Dossier des données premium (`<données>/premium`), posé par main.gd.
var premium_dir := ""
## Modèles .glb déjà lus (chemin → scène) : une reconstruction (qualité,
## skin) ne relit pas 6 Mo à chaque fois.
var _model_cache: Dictionary = {}
## Nœuds propres à un skin à modèle 3D (null pour le renard).
var _float: Node3D = null
var _iris_mat: StandardMaterial3D = null
## Part du regard appliquée à la tête : l'œil d'un casque glisse sur sa
## surface, un regard complet le ferait sortir de la visière.
var _gaze_scale := 1.0
## Squelette d'un skin à modèle 3D (fichier `.rig`, voir `_attach_rig`) :
## null si le modèle n'en a pas (il bouge alors d'un bloc).
var _rig: Skeleton3D = null
var _rig_ids: Dictionary = {}
## Os des câbles : [indice, côté (-1/+1), secteur, rang dans la chaîne].
var _cable_bones: Array = []
## Tentacules d'un modèle mécanique (os `tent_<k>_<i>`, griffes
## `claw_<k>_<f>`) : une entrée par tentacule, voir `_setup_tentacles`.
var _tentacles: Array = []
## Réglages d'humeur lissés (`TENTACLE_MOODS`) et phase de l'onde, intégrée :
## un changement de vitesse ne fait jamais sauter l'onde.
var _tent_mix: Dictionary = {}
var _tent_phase := 0.0
## Rotation de la tête et hauteur à l'image précédente (inertie).
var _last_head_rot := Vector3.ZERO
var _last_lift := 0.0
## Matériaux lumineux d'un modèle qui déclare `model_glow` (runes, liserés) :
## ils pulsent doucement et s'allument avec la parole.
var _glow_mats: Array = []
var _glow_base := 1.0
var _aura_mat: ShaderMaterial = null
## Pupille fendue (`pupil_slit`) et débattement des pupilles (unités de
## l'œil, proportionnel à son rayon ; 0,012 pour le renard).
var _slit := false
var _pupil_range := 0.012
## Part de « parole » (0 → 1), lissée : les gestes de main en dépendent.
var _talk := 0.0
var _t := 0.0
var _blink_t := 0.0
var _blink := 0.0
## Nœud dont la position est « l'œil de l'utilisateur » (la caméra).
var _viewer: Node3D = null
## Regard lissé (lacet, tangage) appliqué à la tête.
var _gaze := Vector2.ZERO
## Coup d'œil furtif ailleurs : direction, poids (0 → 1) et calendrier.
var _glance := Vector2.ZERO
var _glance_w := 0.0
var _glance_until := 0.0
var _next_glance := 6.0
## Micro-saccades : de minuscules mouvements des pupilles, signe de vie.
var _saccade := Vector2.ZERO
var _next_saccade := 0.5

# Nœuds animés.
var _body: Node3D
var _torso: Node3D
var _head: Node3D
var _jaw: Node3D
var _ear_l: Node3D
var _ear_r: Node3D
var _arm_l: Node3D
var _arm_r: Node3D
var _tail: Array[Node3D] = []
var _eyes: Array[Node3D] = []
var _pupils: Array[Node3D] = []
var _pose: Dictionary = {}
## Multiplicateur de tessellation, piloté par le profil graphique (1 = low).
var _detail := 1.0
var _outline: StandardMaterial3D
var _plain_cache: Dictionary = {}
var _say_until := 0.0
var _previous_state := ""
## Geste ponctuel (« one-shot ») joué par-dessus la pose de fond : c'est lui
## qui rend l'animation marquante, la pose seule restant lisible mais statique.
## Le geste survit aux changements d'état (ex. `success` puis `speaking`) :
## il est additif et s'éteint seul à la fin de sa durée.
var _gesture := ""
var _gesture_t := 0.0
var _gesture_dur := 0.0

var _mat_fur: StandardMaterial3D
var _mat_cream: StandardMaterial3D
var _mat_dark: StandardMaterial3D
var _mat_eye: StandardMaterial3D
var _mat_pupil: StandardMaterial3D
var _mat_shirt: StandardMaterial3D
var _mat_accent: StandardMaterial3D


func _ready() -> void:
	_pose = (POSES[IDLE] as Dictionary).duplicate()
	_make_materials()
	_build_body()


## Change la finesse du maillage et reconstruit le personnage. L'état et la
## posture courante sont conservés : seule la géométrie est remplacée.
func set_detail(detail: float) -> void:
	if is_equal_approx(detail, _detail):
		return
	_detail = detail
	if is_inside_tree():
		_rebuild()


## Change de skin : nouveaux matériaux, puis reconstruction du personnage.
## Renvoie `false` (et ne change rien) si le skin est inconnu.
func set_skin(name: String) -> bool:
	if not has_skin(name):
		return false
	if name == skin:
		return true
	# Skin à modèle 3D sans son fichier (pas encore téléchargé) : refusé
	# plutôt qu'un personnage vide.
	var wanted := _palette_of(name)
	if wanted.has("model") and _load_model(wanted) == null:
		push_warning("[godot/jimmy] modèle absent pour le skin %s" % name)
		return false
	skin = name
	if is_inside_tree():
		_make_materials()
		_rebuild()
	return true


## Vrai si le skin existe, de base ou premium.
func has_skin(name: String) -> bool:
	if SKINS.has(name) or _premium_skins().has(name):
		return true
	# Skin premium livré après le lancement (achat, premier téléchargement) :
	# on relit le dossier une fois avant de conclure qu'il n'existe pas.
	_premium_loaded = false
	_premium_cache = {}
	return _premium_skins().has(name)


## Palette du skin courant (base, sinon premium, sinon skin par défaut).
func _palette() -> Dictionary:
	return _palette_of(skin)


func _palette_of(name: String) -> Dictionary:
	if SKINS.has(name):
		return SKINS[name]
	return _premium_skins().get(name, SKINS[SKIN_DEFAULT])


## Dossier des données premium : celui passé par Tauri, sinon (lancement
## direct de Godot, tests) `data/premium` du dépôt.
func _premium_root() -> String:
	if premium_dir != "":
		return premium_dir
	return ProjectSettings.globalize_path("res://").path_join("../data/premium").simplify_path()


## Compagnon des tâches de fond du skin courant : le lapin pour les skins de
## base, celui que le skin premium déclare sinon (`companion`).
func companion() -> String:
	return str(_palette().get("companion", "lapin"))


## Scène du modèle 3D d'un skin (lue une fois, puis en cache), ou null si le
## fichier manque ou ne se lit pas.
func _load_model(palette: Dictionary) -> Node3D:
	var dir := _premium_root()
	var path := dir.path_join("models").path_join(str(palette.get("model", "")))
	if _model_cache.has(path):
		return _model_cache[path]
	if not FileAccess.file_exists(path):
		return null
	var doc := GLTFDocument.new()
	var state := GLTFState.new()
	if doc.append_from_file(path, state) != OK:
		push_warning("[godot/jimmy] modèle illisible : %s" % path)
		return null
	var scene := doc.generate_scene(state) as Node3D
	if palette.has("rig"):
		_attach_rig(scene, dir.path_join("models").path_join(str(palette["rig"])))
	_model_cache[path] = scene
	print("[godot/jimmy] modèle chargé : %s" % path.get_file())
	return scene


## Pose un squelette sur un modèle d'un seul bloc, à partir d'un fichier
## `.rig` calculé hors ligne (outil d'auto-squelette) : « JRIG », taille de
## l'en-tête (u32), en-tête JSON (os : nom, parent, origine ; nombre de
## sommets), puis par sommet sa position (3 float32), ses 4 os (int32) et
## ses 4 poids (float32). Correspondance des sommets : par indice quand le
## nombre concorde (même ordre que le fichier), contrôlée par la distance ;
## sinon par position arrondie au millième. Pas d'égalité exacte : l'import
## glTF de Godot **compresse** les positions (écarts ~1e-5).
## Les os ont une base identité (seule l'origine compte) : leurs rotations
## s'expriment donc dans les axes du modèle.
func _attach_rig(scene: Node3D, path: String) -> bool:
	var f := FileAccess.open(path, FileAccess.READ)
	if f == null:
		push_warning("[godot/jimmy] squelette absent : %s" % path)
		return false
	if f.get_buffer(4).get_string_from_ascii() != "JRIG":
		push_warning("[godot/jimmy] squelette illisible : %s" % path)
		return false
	var header = JSON.parse_string(f.get_buffer(f.get_32()).get_string_from_utf8())
	if typeof(header) != TYPE_DICTIONARY:
		return false
	var n := int(header["count"])
	var pos := f.get_buffer(n * 12).to_float32_array()
	var ids := f.get_buffer(n * 16).to_int32_array()
	var wts := f.get_buffer(n * 16).to_float32_array()
	if pos.size() != n * 3 or ids.size() != n * 4 or wts.size() != n * 4:
		push_warning("[godot/jimmy] squelette tronqué : %s" % path)
		return false
	var lookup := {}
	for i in n:
		lookup[_position_key(Vector3(pos[i * 3], pos[i * 3 + 1], pos[i * 3 + 2]))] = i
	var rig_pos := func(j: int) -> Vector3: return Vector3(pos[j * 3], pos[j * 3 + 1], pos[j * 3 + 2])

	var skel := Skeleton3D.new()
	skel.name = "Skeleton"
	var skin := Skin.new()
	var bones: Array = header["bones"]
	for b in bones:
		var origin := Vector3(float(b["origin"][0]), float(b["origin"][1]), float(b["origin"][2]))
		var idx := skel.add_bone(str(b["name"]))
		var parent := int(b["parent"])
		var rest := origin
		if parent >= 0:
			skel.set_bone_parent(idx, parent)
			var po: Array = bones[parent]["origin"]
			rest = origin - Vector3(float(po[0]), float(po[1]), float(po[2]))
		skel.set_bone_rest(idx, Transform3D(Basis(), rest))
		skin.add_bind(idx, Transform3D(Basis(), -origin))
	skel.reset_bone_poses()
	scene.add_child(skel)

	var chest := skel.find_bone("chest")
	var misses := 0
	for node in scene.find_children("*", "MeshInstance3D", true, false):
		var mi := node as MeshInstance3D
		# Le maillage passe dans le repère du modèle (celui du squelette).
		var xf := Transform3D()
		var walker: Node = mi
		while walker != scene and walker is Node3D:
			xf = (walker as Node3D).transform * xf
			walker = walker.get_parent()
		var skinned := ArrayMesh.new()
		for surface in mi.mesh.get_surface_count():
			var arrays := mi.mesh.surface_get_arrays(surface)
			var verts: PackedVector3Array = arrays[Mesh.ARRAY_VERTEX]
			var normals = arrays[Mesh.ARRAY_NORMAL]
			var bone_arr := PackedInt32Array()
			bone_arr.resize(verts.size() * 4)
			var weight_arr := PackedFloat32Array()
			weight_arr.resize(verts.size() * 4)
			var by_index := verts.size() == n
			for i in verts.size():
				verts[i] = xf * verts[i]
				if normals != null and i < normals.size():
					normals[i] = (xf.basis * normals[i]).normalized()
				var j := -1
				if by_index and verts[i].distance_squared_to(rig_pos.call(i)) < 1e-6:
					j = i
				else:
					j = lookup.get(_position_key(verts[i]), -1)
				if j < 0:
					misses += 1
					bone_arr[i * 4] = chest
					weight_arr[i * 4] = 1.0
					continue
				for k in 4:
					bone_arr[i * 4 + k] = ids[j * 4 + k]
					weight_arr[i * 4 + k] = wts[j * 4 + k]
			arrays[Mesh.ARRAY_VERTEX] = verts
			if normals != null:
				arrays[Mesh.ARRAY_NORMAL] = normals
			arrays[Mesh.ARRAY_BONES] = bone_arr
			arrays[Mesh.ARRAY_WEIGHTS] = weight_arr
			skinned.add_surface_from_arrays(Mesh.PRIMITIVE_TRIANGLES, arrays)
			skinned.surface_set_material(surface, mi.mesh.surface_get_material(surface))
		mi.mesh = skinned
		mi.transform = Transform3D()
		mi.skin = skin
		mi.skeleton = mi.get_path_to(skel)
	print("[godot/jimmy] squelette : %d os, %d sommets sans poids" % [bones.size(), misses])
	return true


func _position_key(p: Vector3) -> Vector3i:
	return Vector3i(roundi(p.x * 1000.0), roundi(p.y * 1000.0), roundi(p.z * 1000.0))


func _rig_rot(bone: String, euler: Vector3) -> void:
	var i: int = _rig_ids.get(bone, -1)
	if i >= 0:
		_rig.set_bone_pose_rotation(i, Quaternion.from_euler(euler))


## Mouvements du squelette, à partir des mêmes nœuds que le renard : la
## tête reprend `_head` (regard, hochements), les bras l'écart de `_arm_l` /
## `_arm_r` avec la pose de repos (gestes compris), le reste est de la vie :
## respiration, mains qui accompagnent la parole, jambes qui battent en
## lévitation, câbles qui ondulent chacun avec un décalage.
func _drive_rig(delta: float, breathe: float, sway: float, bounce: float) -> void:
	_talk = move_toward(_talk, 1.0 if state == SPEAKING else 0.0, delta * 3.0)
	_rig_rot("head", _head.rotation)
	_rig_rot("chest", Vector3(0.03 * breathe * bounce, 0.04 * sway, 0.0))
	_rig_rot("spine", Vector3(0.012 * breathe, 0.0, 0.02 * sway))
	if not _tentacles.is_empty():
		_drive_tentacles(delta)
		return

	# Bras : 0,10 = bras du renard au repos (POSES idle).
	var raise_n := _arm_l.rotation.z + 0.10
	var raise_p := _arm_r.rotation.z - 0.10
	_rig_rot("upperarm_n", Vector3(0.0, 0.0, raise_n * 0.8))
	_rig_rot("upperarm_p", Vector3(0.0, 0.0, raise_p * 0.6))
	var g := _talk
	_rig_rot("forearm_n", Vector3(0.10 * sin(_t * 1.3) + 0.18 * g * sin(_t * 2.7), 0.0, -0.15 * g * absf(sin(_t * 2.1))))
	_rig_rot("forearm_p", Vector3(0.28 * g * sin(_t * 3.1), 0.15 * g * sin(_t * 1.9), 0.06 * sin(_t * 1.1) + 0.12 * g * sin(_t * 2.3)))
	_rig_rot("hand_n", Vector3(0.15 * sin(_t * 1.7), 0.0, 0.1 * sin(_t * 1.2)))
	_rig_rot("hand_p", Vector3(0.12 * sin(_t * 1.5 + 1.0), 0.2 * g * sin(_t * 3.3), 0.1 * sin(_t * 1.4)))

	# Jambes : battement lent, décalé entre les deux, le tibia suit la cuisse.
	_rig_rot("thigh_n", Vector3(0.08 * sin(_t * 0.9), 0.0, 0.0))
	_rig_rot("shin_n", Vector3(0.10 * sin(_t * 0.9 - 0.7), 0.0, 0.0))
	_rig_rot("foot_n", Vector3(0.08 * sin(_t * 0.9 - 1.3), 0.0, 0.0))
	_rig_rot("thigh_p", Vector3(0.08 * sin(_t * 0.9 + PI), 0.0, 0.0))
	_rig_rot("shin_p", Vector3(0.10 * sin(_t * 0.9 + PI - 0.7), 0.0, 0.0))
	_rig_rot("foot_p", Vector3(0.08 * sin(_t * 0.9 + PI - 1.3), 0.0, 0.0))

	# Câbles : onde qui court vers la pointe ; plus vive quand Jimy travaille.
	var speed := 2.4 if state == "executing" or state == THINKING else 1.4
	for c in _cable_bones:
		var side: float = c[1]
		var sector: int = c[2]
		var rank: int = c[3]
		var amp := 0.05 + 0.03 * rank
		# Secteur 0 = chaîne la plus basse : elle pend derrière les jambes ;
		# une grande ondulation lui ferait traverser le tibia.
		var low := sector == 0
		if low:
			amp *= 0.3
		# Décalage faible entre secteurs voisins : des chaînes voisines qui
		# bougent en sens opposé étirent les brins qui les relient.
		var phase := _t * speed - rank * 0.9 + sector * 0.35 + (0.0 if side > 0.0 else 0.7)
		_rig.set_bone_pose_rotation(c[0], Quaternion.from_euler(Vector3(
			0.0 if low else 0.04 * rank * sin(phase * 0.7 + 0.5), 0.0, side * amp * sin(phase))))


## Prépare l'animation des tentacules d'un modèle mécanique (os
## `tent_<k>_<i>`, rang 0 à la racine ; griffes `claw_<k>_<f>`). Pour chaque
## os : sa direction de repos (vers l'os suivant) et ses axes de flexion,
## dans les axes du modèle (os à base identité, comme ceux des .rig) :
##   - `curl`  : plan de courbure naturel du tentacule (l'enrouler davantage) ;
##   - `side`  : hors de ce plan (balayage latéral) ;
##   - `droop` : vers le bas (pendre) ; `aim` : vers l'utilisateur (+Z).
## Une griffe s'ouvre autour de `pointe × écart` (vers l'extérieur).
func _setup_tentacles() -> void:
	var chains := {}
	var claws := {}
	for bone_name in _rig_ids:
		var parts := str(bone_name).split("_")
		if parts.size() != 3:
			continue
		if parts[0] == "tent":
			if not chains.has(parts[1]):
				chains[parts[1]] = {}
			chains[parts[1]][int(parts[2])] = _rig_ids[bone_name]
		elif parts[0] == "claw":
			if not claws.has(parts[1]):
				claws[parts[1]] = []
			claws[parts[1]].append(_rig_ids[bone_name])
	for key in chains:
		var ranks: Array = chains[key].keys()
		ranks.sort()
		var bones: Array = []
		var origins: Array = []
		for r in ranks:
			bones.append(chains[key][r])
			origins.append(_rig.get_bone_global_rest(chains[key][r]).origin)
		var fingers: Array = claws.get(key, [])
		# Le dernier os vise ses griffes ; celui d'une antenne prolonge l'avant-dernier.
		var dirs: Array = []
		for i in bones.size():
			var d := Vector3.UP
			if i + 1 < bones.size():
				d = origins[i + 1] - origins[i]
			elif not fingers.is_empty():
				var mid := Vector3.ZERO
				for f in fingers:
					mid += _rig.get_bone_global_rest(f).origin
				d = mid / fingers.size() - origins[i]
			elif i > 0:
				d = dirs[i - 1]
			dirs.append(d.normalized())
		# Courbure naturelle : somme des virages d'un os au suivant.
		var curl := Vector3.ZERO
		for i in range(1, dirs.size()):
			curl += (dirs[i - 1] as Vector3).cross(dirs[i])
		if curl.length() < 1e-4:
			curl = (dirs[0] as Vector3).cross(Vector3.UP)
		curl = curl.normalized()
		var links: Array = []
		for i in bones.size():
			var d: Vector3 = dirs[i]
			links.append({
				"bone": bones[i], "dir": d,
				"side": _axis_or_zero(d.cross(curl)),
				"droop": _axis_or_zero(d.cross(Vector3.DOWN)),
				"aim": _axis_or_zero(d.cross(Vector3.BACK)),
				"rot": Vector3.ZERO, "vel": Vector3.ZERO,
			})
		var tip: Vector3 = dirs[dirs.size() - 1]
		var grips: Array = []
		for f in fingers:
			var out: Vector3 = _rig.get_bone_global_rest(f).origin - origins[origins.size() - 1]
			out -= tip * out.dot(tip)
			grips.append({"bone": f, "axis": _axis_or_zero(tip.cross(out)), "open": 0.0})
		# Phase propre (angle d'or) : deux tentacules ne sont jamais synchrones.
		_tentacles.append({"links": links, "curl": curl, "grips": grips,
			"phase": _tentacles.size() * 2.399, "sensor": fingers.is_empty()})


func _axis_or_zero(v: Vector3) -> Vector3:
	return v.normalized() if v.length() > 1e-3 else Vector3.ZERO


## Anime les tentacules. Chaque os suit une **cible** (humeur de l'état, onde
## qui court vers la pointe, dérive lente, gestes) au travers d'un ressort
## amorti, plus souple vers la pointe : le mouvement se propage en cascade.
## L'inertie déplace l'état du ressort (pas la cible) quand la sphère tourne
## ou monte : les tentacules restent un instant en arrière, puis rattrapent
## en dépassant un peu. C'est ce qui rend le mouvement fluide.
func _drive_tentacles(delta: float) -> void:
	var dt := minf(delta, 1.0 / 30.0)
	var mood: Dictionary = TENTACLE_MOODS.get(state, TENTACLE_MOODS["idle"])
	if _tent_mix.is_empty():
		_tent_mix = mood.duplicate()
		_last_head_rot = _head.rotation
		_last_lift = _float.position.y + _body.position.y
	for key in mood:
		_tent_mix[key] = lerpf(_tent_mix[key], mood[key], clampf(dt * 2.5, 0.0, 1.0))
	_tent_phase += dt * float(_tent_mix["speed"])
	var amp: float = _tent_mix["amp"]

	# Mouvement de la sphère depuis l'image précédente (un saut = reconstruction).
	var spin := _head.rotation - _last_head_rot
	_last_head_rot = _head.rotation
	if spin.length() > 0.3:
		spin = Vector3.ZERO
	var lift := _float.position.y + _body.position.y
	var rise := lift - _last_lift
	_last_lift = lift
	if absf(rise) > 0.05:
		rise = 0.0

	var env := _gesture_env()
	var gk := clampf(_gesture_t / _gesture_dur, 0.0, 1.0) if _gesture != "" else 0.0
	var jaw := float(_pose["jaw"])
	for chain in _tentacles:
		var links: Array = chain["links"]
		var curl_axis: Vector3 = chain["curl"]
		var ph: float = chain["phase"]
		var n := links.size()
		# Dérive lente propre à chaque tentacule : jamais deux fois le même geste.
		var drift := 0.035 * sin(_t * 0.37 + ph * 2.1) + 0.025 * sin(_t * 0.61 + ph * 3.3)
		var aim: float = float(_tent_mix["aim"]) * (1.0 if chain["sensor"] else 0.5)
		for i in n:
			var link: Dictionary = links[i]
			var u := float(i) / maxf(1.0, n - 1.0)
			var w := 0.45 + 0.55 * u
			var curl: float = float(_tent_mix["curl"]) + drift * w \
				+ amp * w * sin(_tent_phase * 1.6 - i * 0.75 + ph)
			var side := amp * 0.8 * w * sin(_tent_phase * 1.3 - i * 0.6 + ph * 1.7 + 1.0)
			var droop: float = float(_tent_mix["droop"]) * (1.0 - 0.5 * u)
			match _gesture:
				"nod":
					curl += 0.12 * sin(gk * TAU) * env * w
				"perk":
					droop -= 0.08 * env
				"cheer":
					curl -= 0.10 * env
				"shake":
					side += 0.09 * sin(_gesture_t * 22.0 + i * 0.9 + ph) * env
			var target: Vector3 = curl_axis * curl + link["side"] * side + link["droop"] * droop
			if aim > 0.0 and i < 4:
				target += link["aim"] * (0.07 * aim)
			# Inertie, en cascade : ~1× le mouvement de la sphère au total.
			var d: Vector3 = link["dir"]
			var lag := 0.11 * (0.5 + u)
			var rot: Vector3 = link["rot"] - (spin - d * spin.dot(d)) * lag \
				+ link["droop"] * (rise * 1.5 * (0.5 + u))
			var k := lerpf(140.0, 45.0, u)
			var vel: Vector3 = link["vel"]
			vel += (k * (target - rot) - 1.2 * sqrt(k) * vel) * dt
			rot += vel * dt
			link["rot"] = rot
			link["vel"] = vel
			var angle := rot.length()
			_rig.set_bone_pose_rotation(link["bone"],
				Quaternion(rot / angle, angle) if angle > 1e-5 else Quaternion.IDENTITY)

		# Pinces : entrouvertes au repos, elles « parlent » avec la voix et
		# claquent pendant la joie.
		for g in chain["grips"]:
			var want: float = float(_tent_mix["grip"]) + 0.10 * maxf(0.0, sin(_t * 0.5 + ph))
			if state == SPEAKING:
				want += 0.45 * jaw * (0.5 + 0.5 * sin(_t * 8.0 + ph * 3.0))
			if _gesture == "cheer":
				want += 0.45 * absf(sin(gk * PI * 3.0)) * env
			g["open"] = lerpf(g["open"], want, clampf(dt * 10.0, 0.0, 1.0))
			_rig.set_bone_pose_rotation(g["bone"], Quaternion(g["axis"], g["open"]))


## Lit les skins premium (un JSON par skin). Un fichier invalide est ignoré
## avec un avertissement : il ne doit jamais empêcher l'avatar de s'afficher
## (piège 72 : un skin cassé = avatar invisible).
func _premium_skins() -> Dictionary:
	if _premium_loaded:
		return _premium_cache
	_premium_loaded = true
	var skins_dir := _premium_root().path_join("skins")
	var dir := DirAccess.open(skins_dir)
	if dir == null:
		return _premium_cache
	for file in dir.get_files():
		if not file.ends_with(".json"):
			continue
		var data = JSON.parse_string(FileAccess.get_file_as_string(skins_dir.path_join(file)))
		if typeof(data) != TYPE_DICTIONARY:
			push_warning("[godot/jimmy] skin premium illisible : %s" % file)
			continue
		var palette := {}
		var valid := true
		for key in data:
			# `_licence`, `label` : informatifs, pas des paramètres de rendu.
			if str(key).begins_with("_") or key == "label":
				continue
			var value = data[key]
			if key in SKIN_COLOR_KEYS or key in SKIN_OPTIONAL_COLOR_KEYS:
				if typeof(value) != TYPE_ARRAY or value.size() < 3:
					valid = false
					break
				palette[key] = Color(float(value[0]), float(value[1]), float(value[2]))
			elif typeof(value) == TYPE_STRING:
				# `model`, `companion` : des noms, pas des nombres.
				palette[key] = value
			elif typeof(value) == TYPE_ARRAY and value.size() >= 3:
				# `head_center`, `eye_offset` : positions dans le modèle.
				palette[key] = Vector3(float(value[0]), float(value[1]), float(value[2]))
			else:
				palette[key] = float(value)
		for key in SKIN_COLOR_KEYS:
			if not palette.has(key):
				valid = false
		if valid:
			_premium_cache[file.get_basename()] = palette
		else:
			push_warning("[godot/jimmy] skin premium incomplet : %s" % file)
	return _premium_cache


func _rebuild() -> void:
	if _body != null:
		remove_child(_body)
		_body.queue_free()
	_tail.clear()
	_eyes.clear()
	_pupils.clear()
	_float = null
	_iris_mat = null
	_rig = null
	_rig_ids.clear()
	_cable_bones.clear()
	_tentacles.clear()
	_tent_mix = {}
	_glow_mats.clear()
	_aura_mat = null
	_slit = false
	_pupil_range = 0.012
	_build_body()


func set_state(new_state: String) -> void:
	var key := new_state.to_lower()
	if not POSES.has(key):
		push_warning("[godot/jimmy] état inconnu : %s (on reste sur %s)" % [new_state, state])
		return
	if key == state:
		return
	_previous_state = state
	state = key
	# Un geste franc à l'entrée de chaque état : sans lui, seule la pose
	# (statique) change et l'œil ne remarque rien.
	match key:
		LISTENING:
			_trigger_gesture("nod", 0.7)
		THINKING:
			_trigger_gesture("perk", 0.8)
		"success":
			_trigger_gesture("cheer", 1.6)
		"error":
			_trigger_gesture("shake", 1.8)


## Déclenche un geste ponctuel (remplace celui en cours, aucun empilement).
func _trigger_gesture(kind: String, duration: float) -> void:
	_gesture = kind
	_gesture_t = 0.0
	_gesture_dur = maxf(0.2, duration)


## Enveloppe du geste : 0 aux bords, 1 au milieu (montée/descente douces,
## pas de saut visible à l'apparition ni à la disparition).
func _gesture_env() -> float:
	if _gesture == "" or _gesture_dur <= 0.0:
		return 0.0
	return sin(clampf(_gesture_t / _gesture_dur, 0.0, 1.0) * PI)


## Durée forcée de l'animation « speaking » (le temps de lire la bulle).
## Renvoie la durée réellement retenue.
func speak_for(seconds: float) -> float:
	set_state(SPEAKING)
	# `success`/`error` sont transitoires : la parole qui suit ne doit pas y
	# revenir ensuite (bras en V figés après chaque réponse, sinon).
	if _previous_state == "success" or _previous_state == "error":
		_previous_state = IDLE
	var effective := maxf(0.4, seconds)
	_say_until = _t + effective
	return effective


## Désigne « l'œil de l'utilisateur » : Jimmy le regarde (la caméra).
func set_viewer(node: Node3D) -> void:
	_viewer = node


## Angles (lacet, tangage) qui orientent la tête vers le spectateur, dans le
## repère du cou : le balancement du corps est compensé, comme une vraie tête
## qui garde le regard fixé.
func _viewer_angles() -> Vector2:
	if _viewer == null or not is_instance_valid(_viewer):
		return Vector2.ZERO
	var neck := _head.get_parent() as Node3D
	var local := neck.to_local(_viewer.global_position) - _head.position
	var yaw := atan2(local.x, local.z)
	var pitch := -atan2(local.y, Vector2(local.x, local.z).length())
	return Vector2(clampf(yaw, -GAZE_YAW_MAX, GAZE_YAW_MAX), clampf(pitch, -GAZE_PITCH_MAX, GAZE_PITCH_MAX))


## Coups d'œil ailleurs et micro-saccades. Fixer sans jamais ciller du regard
## met mal à l'aise ; un bref regard de côté, puis le retour vers
## l'utilisateur, rend le contact crédible. Plus rares quand Jimmy écoute ou
## parle : il est alors attentif.
func _update_glance(delta: float) -> void:
	if _t >= _next_glance and _glance_w <= 0.01:
		var attentive := state == LISTENING or state == SPEAKING
		_glance = Vector2(randf_range(0.22, 0.4) * (1.0 if randf() < 0.5 else -1.0), randf_range(-0.12, 0.14))
		_glance_until = _t + randf_range(0.45, 1.1)
		_next_glance = _t + (randf_range(9.0, 15.0) if attentive else randf_range(4.5, 9.0))
	var want := 1.0 if _t < _glance_until else 0.0
	_glance_w = move_toward(_glance_w, want, delta * 5.0)
	if _t >= _next_saccade:
		_saccade = Vector2(randf_range(-0.1, 0.1), randf_range(-0.07, 0.07))
		_next_saccade = _t + randf_range(0.35, 0.9)


func _process(delta: float) -> void:
	_t += delta

	# Horloge du geste ponctuel : il s'éteint seul, même si l'état a changé.
	if _gesture != "":
		_gesture_t += delta
		if _gesture_t >= _gesture_dur:
			_gesture = ""
			_gesture_t = 0.0
			_gesture_dur = 0.0

	if state == SPEAKING and _t > _say_until:
		set_state(_previous_state if _previous_state != SPEAKING else IDLE)

	var target: Dictionary = POSES.get(state, POSES[IDLE])
	for key in target.keys():
		var speed := POSE_SPEED
		if key == "jaw":
			speed = 26.0 if state == SPEAKING else 12.0
		_pose[key] = lerpf(_pose[key], target[key], clampf(delta * speed, 0.0, 1.0))

	_update_blink(delta)
	_apply_pose(delta)


func _update_blink(delta: float) -> void:
	_blink_t += delta
	if _blink_t > BLINK_PERIOD:
		_blink_t = 0.0
		_blink = 1.0
	_blink = maxf(0.0, _blink - delta * 7.0)


func _apply_pose(delta: float) -> void:
	var breathe := sin(_t * 1.6)
	var sway := sin(_t * 0.7)
	var bounce: float = _pose["bounce"]
	# Pendant la joie, tout s'emballe : respiration, balancement et queue.
	if _gesture == "cheer":
		bounce = maxf(bounce, 1.6)

	# Corps : respiration + balancement de vie.
	_body.position.y = 0.02 * breathe * bounce
	_body.rotation.z = deg_to_rad(1.2) * sway * bounce
	_body.rotation.y = deg_to_rad(1.5) * sin(_t * 0.45)

	_torso.rotation.x = _pose["body_pitch"] + deg_to_rad(1.0) * bounce * sin(_t * 1.6 + 0.6)
	_torso.rotation.y = _pose["body_yaw"] + deg_to_rad(2.0) * bounce * sway
	_torso.position.z = _pose["lean"] * 0.05

	# Tête : vers l'utilisateur (ou ailleurs selon l'état), posture en plus.
	_update_glance(delta)
	var contact: float = EYE_CONTACT.get(state, 1.0)
	var away: Vector2 = GAZE_AWAY.get(state, Vector2.ZERO)
	var gaze_target := _viewer_angles() * contact + away + _glance * _glance_w
	_gaze = _gaze.lerp(gaze_target, clampf(delta * 6.0, 0.0, 1.0))
	_head.rotation.x = _pose["head_pitch"] + _gaze.y * _gaze_scale
	_head.rotation.y = _pose["head_yaw"] + _gaze.x * _gaze_scale
	_head.rotation.z = _pose["head_roll"]
	_head.position.y = HEAD_Y + 0.012 * breathe * bounce

	# Mâchoire : ouverture pendant la parole.
	var jaw := float(_pose["jaw"])
	if state == SPEAKING:
		jaw *= 0.55 + 0.45 * absf(sin(_t * 11.0))
	_jaw.rotation.x = jaw * 0.42

	# Skin à modèle 3D : lévitation lente, et l'œil (sans mâchoire) s'illumine
	# au rythme de la parole.
	if _float != null:
		_float.position.y = 0.035 * sin(_t * 1.3)
		_float.rotation.z = deg_to_rad(1.5) * sin(_t * 0.8)
	if _rig != null:
		_drive_rig(delta, breathe, sway, bounce)
	if _iris_mat != null:
		var listening := 0.35 if state == LISTENING else 0.0
		_iris_mat.emission_energy_multiplier = 0.9 + listening + jaw * 2.2
	if not _glow_mats.is_empty():
		var pulse := 1.0 + 0.18 * sin(_t * 2.2) + 0.5 * jaw * absf(sin(_t * 9.0))
		if state == "error":
			pulse *= 0.45
		for mat in _glow_mats:
			(mat as BaseMaterial3D).emission_energy_multiplier = _glow_base * pulse
		if _aura_mat != null:
			_aura_mat.set_shader_parameter("strength", 0.75 * pulse)

	# Bras.
	_arm_l.rotation.z = -_pose["arm_l"] - deg_to_rad(4.0) * breathe * bounce
	_arm_r.rotation.z = _pose["arm_r"] + deg_to_rad(4.0) * breathe * bounce

	# Oreilles : mouvement franc et fréquent (avant : 9° tous les ~5 s,
	# à peine visible). Une oreille qui se dresse se remarque.
	var ear: float = _pose["ear"]
	var twitch := deg_to_rad(16.0) * maxf(0.0, sin(_t * 1.1)) if fmod(_t, 3.1) < 0.5 else 0.0
	_ear_l.rotation.x = ear + twitch
	_ear_r.rotation.x = ear - twitch * 0.6

	# Yeux : clignement + regard. Chaque pupille vise l'utilisateur depuis son
	# propre œil (légère convergence, crédible de près) ; pendant un regard
	# détourné, elle part dans la direction du regard.
	var lid := 1.0 - _blink
	var eye_contact := contact * (1.0 - _glance_w)
	var aside := Vector2(away.x, -away.y) * 0.6 + Vector2(_glance.x, -_glance.y) * _glance_w * 0.6
	for i in _eyes.size():
		_eyes[i].scale.y = maxf(0.08, lid)
		var pupil := _pupils[i]
		var look := aside + _saccade
		if _viewer != null and is_instance_valid(_viewer) and eye_contact > 0.0:
			var d := _eyes[i].to_local(_viewer.global_position).normalized()
			look += Vector2(d.x, d.y) * eye_contact
		var follow := clampf(delta * 10.0, 0.0, 1.0)
		var reach := _pupil_range
		pupil.position.x = lerpf(pupil.position.x, clampf(look.x * reach * 2.5, -reach, reach), follow)
		pupil.position.y = lerpf(pupil.position.y, clampf(look.y * reach * 2.5, -reach * 0.67, reach * 0.67), follow)
		if _slit:
			pupil.scale.x = lerpf(pupil.scale.x, float(PUPIL_SLIT.get(state, 0.3)), clampf(delta * 6.0, 0.0, 1.0))

	# Geste ponctuel : couche additive par-dessus la pose de fond. Chaque
	# geste tient en une phrase lisible : hochement, redressement, saut,
	# secouement. L'enveloppe (`env`) évite tout saut aux bords.
	var env := _gesture_env()
	if env > 0.0:
		var k := clampf(_gesture_t / _gesture_dur, 0.0, 1.0)
		match _gesture:
			"nod":
				# Deux hochements francs (« bien reçu »).
				var nod := -0.22 * sin(k * PI * 4.0) * env
				_head.rotation.x += nod
				_torso.rotation.x += nod * 0.3
			"perk":
				# Redressement + oreilles dressées (« je réfléchis »).
				_head.rotation.x += -0.14 * env
				_ear_l.rotation.x += -0.30 * env
				_ear_r.rotation.x += -0.30 * env
			"cheer":
				# Trois petits sauts, bras levés en V ouvert (« c'est prêt ! »).
				# ±0,55 : avec la pose success (1,9), les mains montent sans se
				# toucher au-dessus de la tête.
				_body.position.y += 0.13 * absf(sin(k * PI * 3.0))
				_arm_l.rotation.z += -0.55 * env
				_arm_r.rotation.z += 0.55 * env
				_head.rotation.x += -0.12 * env
			"shake":
				# Secouement de tête + buste affaissé (« raté »).
				_head.rotation.y += 0.38 * sin(_gesture_t * 15.0) * env
				_head.rotation.x += 0.10 * env
				_torso.rotation.x += 0.08 * env
				_arm_l.rotation.z += -0.15 * env
				_arm_r.rotation.z += 0.15 * env

	# Queue : remuage en cascade, plus rapide quand Jimmy est enthousiaste.
	# Amplitude ×1,5 par rapport à la V1 : le remuage doit se voir.
	var speed := 1.2 + 3.4 * bounce
	for i in _tail.size():
		var phase := _t * speed - i * 0.42
		var amount := (i + 1) * 0.19
		_tail[i].rotation.y = sin(phase) * amount
		_tail[i].rotation.x = deg_to_rad(-3.0) + cos(phase * 0.8) * amount * 0.25


# ── Construction du personnage ───────────────────────────────────────────────

func _make_materials() -> void:
	# Contour « inverted hull » : une seconde passe, gonflée le long des
	# normales et rendue par l'intérieur (faces avant éliminées). Seul le bord
	# dépasse : c'est ce qui rend la silhouette lisible sur un bureau chargé.
	_outline = StandardMaterial3D.new()
	_outline.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	_outline.cull_mode = BaseMaterial3D.CULL_FRONT
	var palette: Dictionary = _palette()
	# Contour : brun foncé par défaut ; un skin premium peut le colorer et
	# l'épaissir (Néon : contour orange vif, sans ombrage, qui « brille »).
	_outline.albedo_color = palette.get("outline", OUTLINE_COLOR)
	_outline.grow = true
	_outline.grow_amount = OUTLINE_WIDTH * float(palette.get("outline_scale", 1.0))
	# Les variantes sans contour dérivent des matériaux : à régénérer aussi.
	_plain_cache.clear()

	_mat_fur = _material(palette["fur"], 0.92)
	_mat_cream = _material(palette["cream"], 0.95)
	_mat_dark = _material(palette["dark"], 0.75)
	# Yeux et pupilles sans contour : à cette taille, il les noircirait.
	_mat_eye = _material(Color(0.98, 0.98, 0.99), 0.35, false)
	_mat_pupil = _material(Color(0.05, 0.04, 0.04), 0.30, false)
	_mat_shirt = _material(palette["shirt"], 0.80)
	_mat_accent = _material(palette["accent"], 0.70)

	# Fourrure : reflet rasant (rim) qui imite le duvet éclairé par l'arrière,
	# et relief fin par une normal map générée (aucun fichier d'asset).
	# Le robot (fur = 0) a une surface lisse : pas de rim ni de normal map.
	var metallic := float(palette.get("metallic", 0.0))
	var has_fur := float(palette.get("has_fur", 1.0)) > 0.5
	for mat in [_mat_fur, _mat_cream, _mat_dark, _mat_shirt, _mat_accent]:
		mat.metallic = metallic
	if has_fur:
		var fur_normal := _fur_normal_map()
		for mat in [_mat_fur, _mat_cream]:
			mat.rim_enabled = true
			mat.rim = 0.35
			mat.rim_tint = 0.6
			mat.normal_enabled = true
			mat.normal_texture = fur_normal
			mat.normal_scale = 0.45
			mat.uv1_scale = Vector3(3.0, 3.0, 1.0)

	# Néon (skin premium) : fourrure, ventre et accents émettent leur propre
	# lumière ; `glow` règle l'intensité (absent ou 0 = aucun effet). Les
	# variantes sans contour sont des copies : elles héritent de l'émission.
	var glow := float(palette.get("glow", 0.0))
	if glow > 0.0:
		for pair in [[_mat_fur, 0.15], [_mat_cream, 0.6], [_mat_accent, 2.2]]:
			var mat: StandardMaterial3D = pair[0]
			mat.emission_enabled = true
			mat.emission = mat.albedo_color
			mat.emission_energy_multiplier = glow * float(pair[1])


## Normal map de fourrure générée à partir d'un bruit : grain fin, sans
## répétition visible grâce à `seamless`.
func _fur_normal_map() -> NoiseTexture2D:
	var noise := FastNoiseLite.new()
	noise.noise_type = FastNoiseLite.TYPE_SIMPLEX_SMOOTH
	noise.frequency = 0.09
	noise.fractal_octaves = 3
	var tex := NoiseTexture2D.new()
	tex.width = 256
	tex.height = 256
	tex.seamless = true
	tex.as_normal_map = true
	tex.bump_strength = 6.0
	tex.noise = noise
	return tex


func _material(color: Color, roughness: float, outlined := true) -> StandardMaterial3D:
	var mat := StandardMaterial3D.new()
	mat.albedo_color = color
	mat.roughness = roughness
	mat.metallic = 0.0
	mat.metallic_specular = 0.35
	if outlined:
		mat.next_pass = _outline
	return mat


## Nombre de segments ajusté au profil graphique.
func _segments(base: int) -> int:
	return int(round(base * _detail))


func _capsule(radius: float, height: float, mat: Material) -> MeshInstance3D:
	var mesh := CapsuleMesh.new()
	mesh.radius = radius
	mesh.height = maxf(height, radius * 2.05)
	mesh.radial_segments = _segments(16)
	mesh.rings = _segments(6)
	mesh.material = mat
	var node := MeshInstance3D.new()
	node.mesh = mesh
	return node


func _sphere(radius: float, mat: Material, segments: int = 20) -> MeshInstance3D:
	var mesh := SphereMesh.new()
	mesh.radius = radius
	mesh.height = radius * 2.0
	mesh.radial_segments = _segments(segments)
	mesh.rings = _segments(int(max(6, segments / 2)))
	mesh.material = mat
	var node := MeshInstance3D.new()
	node.mesh = mesh
	return node


func _cone(bottom: float, top: float, height: float, mat: Material) -> MeshInstance3D:
	var mesh := CylinderMesh.new()
	mesh.bottom_radius = bottom
	mesh.top_radius = top
	mesh.height = height
	mesh.radial_segments = _segments(16)
	mesh.rings = 4
	# Pas de contour sur les cônes : leurs arêtes vives (normales non lissées
	# entre flanc et base) font éclater la coque gonflée en éclats visibles.
	mesh.material = _without_outline(mat)
	var node := MeshInstance3D.new()
	node.mesh = mesh
	return node


## Variante du matériau sans passe de contour (mise en cache, partagée).
func _without_outline(mat: Material) -> Material:
	if mat.next_pass == null:
		return mat
	if not _plain_cache.has(mat):
		var plain := mat.duplicate() as Material
		plain.next_pass = null
		_plain_cache[mat] = plain
	return _plain_cache[mat]


func _box(size: Vector3, mat: Material) -> MeshInstance3D:
	var mesh := BoxMesh.new()
	mesh.size = size
	mesh.material = mat
	var node := MeshInstance3D.new()
	node.mesh = mesh
	return node


func _build_body() -> void:
	_body = Node3D.new()
	_body.name = "Body"
	add_child(_body)

	_gaze_scale = 1.0
	if _palette().has("model"):
		_build_model_body()
		return

	_build_legs()
	_build_torso()
	_build_tail()


## Skin à modèle 3D (voir `RIM_SHADER`) : le .glb remplace jambes, torse,
## tête et queue. Des nœuds vides gardent les points d'accroche de
## `_apply_pose` (bras, oreilles, mâchoire) ; l'œil est construit ici.
func _build_model_body() -> void:
	var p := _palette()
	var source := _load_model(p)
	# Lévitation sur un nœud à part : `_body` reste piloté par la respiration.
	_float = Node3D.new()
	_float.name = "Float"
	_body.add_child(_float)
	_torso = Node3D.new()
	_torso.name = "Torso"
	_torso.position = Vector3(0.0, 0.40, 0.0)
	_float.add_child(_torso)

	# Mise à l'échelle : le .glb fait 1 unité de haut, centré ; son bas est
	# posé à `model_lift` au-dessus du sol (il flotte).
	var s := float(p.get("model_height", 1.45))
	var lift := float(p.get("model_lift", 0.06))
	var min_y := float(p.get("model_min_y", -0.5))
	var model_pos := Vector3(0.0, lift - _torso.position.y - min_y * s, 0.0)
	if source != null:
		var model := source.duplicate() as Node3D
		model.name = "Model"
		model.scale = Vector3.ONE * s
		model.position = model_pos
		_torso.add_child(model)
		_dress_model(model, p)

	# Bras figés dans le modèle : points d'accroche sans géométrie.
	_arm_l = Node3D.new()
	_arm_l.name = "ArmL"
	_torso.add_child(_arm_l)
	_arm_r = Node3D.new()
	_arm_r.name = "ArmR"
	_torso.add_child(_arm_r)

	# Tête : pivot au centre du casque, pour que l'œil tourne sur sa surface.
	var center: Vector3 = p.get("head_center", Vector3(0.0, 0.437, 0.035))
	var neck := Node3D.new()
	neck.name = "Neck"
	neck.position = model_pos + center * s - Vector3(0.0, HEAD_Y, 0.0)
	_torso.add_child(neck)
	_head = Node3D.new()
	_head.name = "Head"
	_head.position = Vector3(0.0, HEAD_Y, 0.0)
	neck.add_child(_head)
	_jaw = Node3D.new()
	_jaw.name = "Jaw"
	_head.add_child(_jaw)
	_ear_l = Node3D.new()
	_head.add_child(_ear_l)
	_ear_r = Node3D.new()
	_head.add_child(_ear_r)
	_gaze_scale = 0.45

	# Avec squelette : les membres bougent chacun (`_drive_rig`), l'œil est
	# accroché à l'os de la tête. Sans : modèle d'un bloc, œil sur `_head`.
	var model_node := _torso.get_node_or_null("Model")
	if model_node != null:
		var skeletons := model_node.find_children("*", "Skeleton3D", true, false)
		if not skeletons.is_empty():
			_rig = skeletons[0] as Skeleton3D
	var eye_offset: Vector3 = p.get("eye_offset", Vector3(0.0, 0.003, 0.09))
	if _rig != null:
		for i in _rig.get_bone_count():
			var bone_name := _rig.get_bone_name(i)
			_rig_ids[bone_name] = i
			if bone_name.begins_with("cable_"):
				# cable_<n|p><secteur>_<rang>
				var tag := bone_name.trim_prefix("cable_")
				_cable_bones.append([i, -1.0 if tag[0] == "n" else 1.0, int(tag[1]), int(tag.get_slice("_", 1))])
		_setup_tentacles()
		var attach := BoneAttachment3D.new()
		attach.name = "HeadAttach"
		_rig.add_child(attach)
		attach.bone_name = "head"
		var head_origin := _rig.get_bone_global_rest(_rig_ids["head"]).origin
		_build_model_eye(p, attach, center + eye_offset - head_origin, 1.0)
		_gaze_scale = 0.8
	else:
		_build_model_eye(p, _head, eye_offset * s, s)
	_build_data_bits(p)
	if p.has("aura"):
		_build_aura(p, model_pos + center * s, s)


## Halo derrière la sphère (`AURA_SHADER`), un peu en retrait pour ne pas
## voiler l'œil ; il pulse avec les runes (`_apply_pose`).
func _build_aura(p: Dictionary, at: Vector3, s: float) -> void:
	_aura_mat = ShaderMaterial.new()
	var shader := Shader.new()
	shader.code = AURA_SHADER
	_aura_mat.shader = shader
	_aura_mat.set_shader_parameter("aura_color", p["aura"])
	var quad := QuadMesh.new()
	var size := float(p.get("aura_size", 0.8)) * s
	quad.size = Vector2(size, size)
	quad.material = _aura_mat
	var aura := MeshInstance3D.new()
	aura.name = "Aura"
	aura.mesh = quad
	aura.position = at - Vector3(0.0, 0.0, 0.12)
	aura.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	_torso.add_child(aura)


## Matériaux du modèle : copies (le modèle en cache reste intact), métal un
## peu adouci (la scène n'a pas de ciel à refléter : le chrome virait au
## noir) et liseré lumineux additif.
func _dress_model(model: Node3D, p: Dictionary) -> void:
	var rim := ShaderMaterial.new()
	var shader := Shader.new()
	shader.code = RIM_SHADER
	rim.shader = shader
	rim.set_shader_parameter("rim_color", p.get("rim", Color(0.25, 0.85, 1.0)))
	for node in model.find_children("*", "MeshInstance3D", true, false):
		var mi := node as MeshInstance3D
		for i in mi.mesh.get_surface_count():
			var source := mi.get_active_material(i)
			if source == null:
				continue
			var mat := source.duplicate() as Material
			if mat is BaseMaterial3D:
				var base := mat as BaseMaterial3D
				base.metallic = minf(base.metallic, float(p.get("model_metallic", 0.55)))
				base.roughness = maxf(base.roughness, 0.28)
				# Runes et liserés : intensité réglée par le skin, puis pulsation
				# (`_apply_pose`). Sans `model_glow`, l'émission du .glb est gardée.
				if base.emission_enabled and p.has("model_glow"):
					_glow_base = float(p["model_glow"])
					base.emission_energy_multiplier = _glow_base
					_glow_mats.append(base)
			mat.next_pass = rim
			mi.set_surface_override_material(i, mat)


## Œil unique : anneau, iris lumineux et pupille mobile, face à l'utilisateur.
## Il cligne (échelle) et regarde comme les yeux du renard (`_apply_pose`).
func _build_model_eye(p: Dictionary, parent: Node3D, at: Vector3, s: float) -> void:
	var r := float(p.get("eye_radius", 0.036)) * s
	var eye := Node3D.new()
	eye.name = "Eye"
	eye.position = at
	parent.add_child(eye)

	var torus := TorusMesh.new()
	torus.inner_radius = r * 0.80
	torus.outer_radius = r
	torus.rings = _segments(24)
	torus.ring_segments = 8
	torus.material = _glow_material(p.get("eye_ring", Color(1.0, 0.2, 0.7)), 1.2)
	var ring := MeshInstance3D.new()
	ring.mesh = torus
	# Le tore est couché dans le plan XZ : on le redresse face à la caméra.
	ring.rotation.x = deg_to_rad(90.0)
	eye.add_child(ring)

	# Émission modérée : au-delà, la lueur (profil « haute ») blanchit le cyan.
	_iris_mat = _glow_material(p["accent"], 0.9)
	var iris := _sphere(r * 0.76, _iris_mat, 20)
	iris.scale = Vector3(1.0, 1.0, 0.35)
	eye.add_child(iris)

	var pupil := _sphere(r * 0.30, _mat_pupil, 14)
	pupil.position = Vector3(0.0, 0.0, r * 0.27)
	pupil.scale = Vector3(1.0, 1.0, 0.5)
	if float(p.get("pupil_slit", 0.0)) > 0.0:
		_slit = true
		pupil.scale = Vector3(0.3, 1.3, 0.5)
	eye.add_child(pupil)
	_pupil_range = r * 0.33

	_eyes.append(eye)
	_pupils.append(pupil)


func _glow_material(color: Color, energy: float) -> StandardMaterial3D:
	var mat := StandardMaterial3D.new()
	mat.albedo_color = color
	mat.roughness = 0.3
	mat.emission_enabled = true
	mat.emission = color
	mat.emission_energy_multiplier = energy
	return mat


## Petits carrés de données lumineux qui montent autour du corps. Ils restent
## dans la zone cliquable (piège 85 : hors zone, rien n'est dessiné).
func _build_data_bits(p: Dictionary) -> void:
	var process := ParticleProcessMaterial.new()
	process.emission_shape = ParticleProcessMaterial.EMISSION_SHAPE_BOX
	process.emission_box_extents = Vector3(0.42, 0.55, 0.18)
	process.direction = Vector3(0.0, 1.0, 0.0)
	process.spread = 12.0
	process.initial_velocity_min = 0.03
	process.initial_velocity_max = 0.08
	process.gravity = Vector3.ZERO
	process.scale_min = 0.6
	process.scale_max = 1.3
	# Couleur tirée au hasard entre l'iris et l'anneau.
	var ramp := Gradient.new()
	ramp.set_color(0, p["accent"])
	ramp.set_color(1, p.get("eye_ring", Color(1.0, 0.2, 0.7)))
	var ramp_tex := GradientTexture1D.new()
	ramp_tex.gradient = ramp
	process.color_initial_ramp = ramp_tex

	var quad := QuadMesh.new()
	quad.size = Vector2(0.018, 0.018)
	var mat := StandardMaterial3D.new()
	mat.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	mat.billboard_mode = BaseMaterial3D.BILLBOARD_PARTICLES
	mat.vertex_color_use_as_albedo = true
	quad.material = mat

	var bits := GPUParticles3D.new()
	bits.name = "DataBits"
	bits.amount = int(10 * _detail) + 10
	bits.lifetime = 4.0
	bits.preprocess = 4.0
	bits.process_material = process
	bits.draw_pass_1 = quad
	bits.position = Vector3(0.0, 0.78, 0.0)
	bits.visibility_aabb = AABB(Vector3(-1.0, -1.0, -1.0), Vector3(2.0, 2.0, 2.0))
	_float.add_child(bits)


func _build_tail() -> void:
	# Queue en chaîne de sphères : chaque maillon reçoit le mouvement du
	# précédent avec un léger retard, ce qui donne un remuage naturel.
	# `tail_scale` = 0 (robot) : pas de queue.
	var tail_scale := float(_palette().get("tail_scale", 1.0))
	if tail_scale <= 0.0:
		return
	var parent: Node3D = _body
	var offset := Vector3(0.0, 0.34, -0.14) * tail_scale
	for i in 4:
		var seg := Node3D.new()
		seg.name = "Tail%d" % (i + 1)
		seg.position = (Vector3(0.0, 0.0, -0.10) if i > 0 else offset)
		seg.rotation.x = deg_to_rad(-18.0) if i == 0 else deg_to_rad(6.0)
		parent.add_child(seg)

		var radius := (0.085 - i * 0.016) * tail_scale
		var fluff := _sphere(radius, _mat_fur, 14)
		fluff.scale = Vector3(1.0, 1.0, 1.15)
		seg.add_child(fluff)

		if i == 3:
			var tip := _cone(radius * 0.95, 0.004, 0.11 * tail_scale, _mat_cream)
			tip.rotation.x = deg_to_rad(-90.0)
			tip.position = Vector3(0.0, 0.0, -0.10 * tail_scale)
			seg.add_child(tip)

		_tail.append(seg)
		parent = seg


func _build_legs() -> void:
	for side in [-1.0, 1.0]:
		var leg := Node3D.new()
		leg.name = "Leg%s" % ("L" if side < 0 else "R")
		leg.position = Vector3(0.10 * side, 0.30, 0.0)
		_body.add_child(leg)

		var thigh := _capsule(0.055, 0.30, _mat_fur)
		thigh.position = Vector3(0.0, -0.14, 0.0)
		leg.add_child(thigh)

		var foot := _sphere(0.062, _mat_dark, 14)
		foot.position = Vector3(0.0, -0.29, 0.02)
		foot.scale = Vector3(0.9, 0.6, 1.5)
		leg.add_child(foot)


func _build_torso() -> void:
	_torso = Node3D.new()
	_torso.name = "Torso"
	_torso.position = Vector3(0.0, 0.40, 0.0)
	_body.add_child(_torso)

	# Torse : robe de renard bleu (le shirt, pour distinguer le personnage
	# d'un simple renard quadrupède).
	var chest := _capsule(0.155, 0.40, _mat_shirt)
	chest.position = Vector3(0.0, 0.16, 0.0)
	chest.scale = Vector3(1.0, 1.0, 0.82)
	_torso.add_child(chest)

	# Bassin : comble le vide entre le bas du torse et le haut des jambes.
	var hips := _sphere(0.13, _mat_shirt, 20)
	hips.position = Vector3(0.0, -0.03, 0.0)
	hips.scale = Vector3(1.05, 0.72, 0.84)
	_torso.add_child(hips)

	# Ventre clair, enfoncé dans le torse plutôt que posé dessus.
	var belly := _sphere(0.115, _mat_cream, 16)
	belly.position = Vector3(0.0, 0.10, 0.082)
	belly.scale = Vector3(0.86, 1.30, 0.46)
	_torso.add_child(belly)

	# Col.
	var collar := _cone(0.135, 0.115, 0.07, _mat_accent)
	collar.position = Vector3(0.0, 0.34, 0.0)
	_torso.add_child(collar)

	_build_arms()
	_build_head()


func _build_arms() -> void:
	for side in [-1.0, 1.0]:
		var arm := Node3D.new()
		arm.name = "Arm%s" % ("L" if side < 0 else "R")
		# 0,135 et non 0,165 : à hauteur d'épaule, le torse ne fait que
		# ~0,12 de rayon, le bras flottait à côté du corps.
		arm.position = Vector3(0.135 * side, 0.30, 0.0)
		_torso.add_child(arm)

		# Épaule (manche) : raccorde le bras au torse.
		var shoulder := _sphere(0.058, _mat_shirt, 16)
		arm.add_child(shoulder)

		var limb := _capsule(0.045, 0.28, _mat_fur)
		limb.position = Vector3(0.0, -0.14, 0.0)
		arm.add_child(limb)

		var hand := _sphere(0.052, _mat_dark, 14)
		hand.position = Vector3(0.0, -0.28, 0.0)
		hand.scale = Vector3(1.0, 0.85, 1.1)
		arm.add_child(hand)

		if side < 0:
			_arm_l = arm
		else:
			_arm_r = arm


func _build_head() -> void:
	var neck := Node3D.new()
	neck.name = "Neck"
	neck.position = Vector3(0.0, 0.30, 0.0)
	_torso.add_child(neck)

	# Col du cou : sans cette pièce, la tête paraît détachée du buste.
	var neck_mesh := _capsule(0.066, 0.20, _mat_fur)
	neck_mesh.position = Vector3(0.0, 0.10, 0.0)
	neck.add_child(neck_mesh)

	_head = Node3D.new()
	_head.name = "Head"
	_head.position = Vector3(0.0, HEAD_Y, 0.0)
	neck.add_child(_head)

	# Crâne.
	var skull := _sphere(0.155, _mat_fur, 24)
	skull.scale = Vector3(1.0, 0.94, 0.96)
	_head.add_child(skull)

	# Joues et front clairs.
	var face := _sphere(0.120, _mat_cream, 20)
	face.position = Vector3(0.0, -0.040, 0.066)
	face.scale = Vector3(0.92, 0.72, 0.85)
	_head.add_child(face)

	# Museau (partie fixe).
	var snout := _sphere(0.064, _mat_fur, 18)
	snout.position = Vector3(0.0, -0.050, 0.148)
	snout.scale = Vector3(0.85, 0.72, 1.45)
	_head.add_child(snout)

	# Mâchoire (partie mobile).
	_jaw = Node3D.new()
	_jaw.name = "Jaw"
	_jaw.position = Vector3(0.0, -0.077, 0.040)
	_head.add_child(_jaw)
	var jaw_mesh := _sphere(0.052, _mat_cream, 16)
	jaw_mesh.position = Vector3(0.0, -0.013, 0.066)
	jaw_mesh.scale = Vector3(0.82, 0.52, 1.35)
	_jaw.add_child(jaw_mesh)

	# Nez.
	var nose := _sphere(0.025, _mat_dark, 14)
	nose.position = Vector3(0.0, -0.004, 0.225)
	nose.scale = Vector3(1.2, 0.85, 0.9)
	_head.add_child(nose)

	_build_ears()
	_build_eyes()


func _build_ears() -> void:
	for side in [-1.0, 1.0]:
		var ear := Node3D.new()
		ear.name = "Ear%s" % ("L" if side < 0 else "R")
		ear.position = Vector3(0.076 * side, 0.118, -0.005)
		ear.scale = Vector3.ONE * float(_palette().get("ear_scale", 1.0))
		_head.add_child(ear)

		var outer := _cone(0.050, 0.004, 0.145, _mat_fur)
		outer.position = Vector3(0.0, 0.08, 0.0)
		outer.rotation.z = deg_to_rad(-9.0 * side)
		ear.add_child(outer)

		var inner := _cone(0.032, 0.002, 0.104, _mat_cream)
		inner.position = Vector3(0.0, 0.075, 0.024)
		inner.rotation.z = deg_to_rad(-9.0 * side)
		ear.add_child(inner)

		if side < 0:
			_ear_l = ear
		else:
			_ear_r = ear


func _build_eyes() -> void:
	for side in [-1.0, 1.0]:
		var eye := Node3D.new()
		eye.name = "Eye%s" % ("L" if side < 0 else "R")
		eye.position = Vector3(0.064 * side, 0.014, 0.120)
		_head.add_child(eye)

		var white := _sphere(0.036, _mat_eye, 16)
		white.scale = Vector3(1.0, 0.88, 0.55)
		eye.add_child(white)

		var pupil := _sphere(0.017, _mat_pupil, 12)
		pupil.position = Vector3(0.0, 0.0, 0.028)
		pupil.scale = Vector3(1.0, 1.0, 0.6)
		eye.add_child(pupil)

		_eyes.append(eye)
		_pupils.append(pupil)