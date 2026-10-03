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
const POSES := {
	"idle": {
		"head_pitch": 0.0, "head_yaw": 0.0, "head_roll": 0.0,
		"body_pitch": 0.0, "body_yaw": 0.0,
		"arm_l": 0.10, "arm_r": -0.10,
		"jaw": 0.0, "ear": 0.0, "lean": 0.0, "bounce": 0.5,
	},
	"listening": {
		"head_pitch": -0.14, "head_yaw": 0.10, "head_roll": 0.12,
		"body_pitch": -0.05, "body_yaw": 0.0,
		"arm_l": 0.05, "arm_r": -0.05,
		"jaw": 0.04, "ear": -0.25, "lean": 0.10, "bounce": 0.7,
	},
	"thinking": {
		"head_pitch": -0.20, "head_yaw": -0.28, "head_roll": -0.05,
		"body_pitch": -0.04, "body_yaw": 0.05,
		"arm_l": 0.20, "arm_r": -1.15,
		"jaw": 0.0, "ear": 0.05, "lean": 0.04, "bounce": 0.35,
	},
	"speaking": {
		"head_pitch": 0.02, "head_yaw": 0.0, "head_roll": 0.0,
		"body_pitch": 0.02, "body_yaw": 0.0,
		"arm_l": 0.18, "arm_r": -0.35,
		"jaw": 0.55, "ear": 0.12, "lean": 0.12, "bounce": 0.9,
	},
	# États additionnels (extension trivial, non utilisés par défaut en V1).
	"executing": {
		"head_pitch": -0.08, "head_yaw": 0.18, "head_roll": 0.0,
		"body_pitch": 0.05, "body_yaw": -0.10,
		"arm_l": 0.85, "arm_r": -0.85,
		"jaw": 0.10, "ear": -0.15, "lean": 0.16, "bounce": 1.0,
	},
	"success": {
		"head_pitch": 0.10, "head_yaw": 0.0, "head_roll": 0.0,
		"body_pitch": -0.08, "body_yaw": 0.0,
		"arm_l": 1.30, "arm_r": -1.30,
		"jaw": 0.35, "ear": -0.30, "lean": -0.06, "bounce": 1.2,
	},
	"error": {
		"head_pitch": 0.28, "head_yaw": 0.0, "head_roll": 0.16,
		"body_pitch": 0.12, "body_yaw": 0.0,
		"arm_l": 0.30, "arm_r": -0.30,
		"jaw": 0.18, "ear": 0.35, "lean": 0.22, "bounce": 0.4,
	},
	"waiting": {
		"head_pitch": -0.06, "head_yaw": 0.22, "head_roll": -0.08,
		"body_pitch": 0.0, "body_yaw": 0.0,
		"arm_l": 0.12, "arm_r": -0.12,
		"jaw": 0.0, "ear": 0.0, "lean": 0.05, "bounce": 0.45,
	},
}

## Skins : une palette + quelques paramètres de forme. Ajouter un skin =
## ajouter une entrée ici (et son libellé dans `providers::avatar::SKINS`,
## côté Rust, pour l'interface). `ear_scale` agrandit les oreilles.
const SKINS := {
	"renard": {
		"fur": Color(0.80, 0.42, 0.13), "cream": Color(0.95, 0.89, 0.79),
		"dark": Color(0.11, 0.08, 0.07), "shirt": Color(0.16, 0.36, 0.72),
		"accent": Color(0.95, 0.62, 0.16), "ear_scale": 1.0,
	},
	"arctique": {
		"fur": Color(0.86, 0.88, 0.92), "cream": Color(0.99, 0.99, 1.0),
		"dark": Color(0.18, 0.20, 0.26), "shirt": Color(0.08, 0.46, 0.50),
		"accent": Color(0.36, 0.74, 0.94), "ear_scale": 0.9,
	},
	"fennec": {
		"fur": Color(0.87, 0.70, 0.47), "cream": Color(0.97, 0.92, 0.82),
		"dark": Color(0.28, 0.18, 0.12), "shirt": Color(0.22, 0.46, 0.28),
		"accent": Color(0.93, 0.78, 0.30), "ear_scale": 1.55,
	},
}
const SKIN_DEFAULT := "renard"

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

var state: String = IDLE
## Skin courant. Positionné avant l'entrée dans l'arbre, ou via `set_skin`.
var skin: String = SKIN_DEFAULT
var _t := 0.0
var _blink_t := 0.0
var _blink := 0.0
var _look_at := Vector2.ZERO

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
	if not SKINS.has(name):
		return false
	if name == skin:
		return true
	skin = name
	if is_inside_tree():
		_make_materials()
		_rebuild()
	return true


func _rebuild() -> void:
	if _body != null:
		remove_child(_body)
		_body.queue_free()
	_tail.clear()
	_eyes.clear()
	_pupils.clear()
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


## Durée forcée de l'animation « speaking » (le temps de lire la bulle).
## Renvoie la durée réellement retenue.
func speak_for(seconds: float) -> float:
	set_state(SPEAKING)
	var effective := maxf(0.4, seconds)
	_say_until = _t + effective
	return effective


## Direction du regard, normalisée dans [-1, 1] (suit le curseur).
func set_look_at(v: Vector2) -> void:
	_look_at = Vector2(clampf(v.x, -1.0, 1.0), clampf(v.y, -1.0, 1.0))


func _process(delta: float) -> void:
	_t += delta

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

	# Corps : respiration + balancement de vie.
	_body.position.y = 0.02 * breathe * bounce
	_body.rotation.z = deg_to_rad(1.2) * sway * bounce
	_body.rotation.y = deg_to_rad(1.5) * sin(_t * 0.45)

	_torso.rotation.x = _pose["body_pitch"] + deg_to_rad(1.0) * bounce * sin(_t * 1.6 + 0.6)
	_torso.rotation.y = _pose["body_yaw"] + deg_to_rad(2.0) * bounce * sway
	_torso.position.z = _pose["lean"] * 0.05

	# Tête : posture + suivi du curseur.
	_head.rotation.x = _pose["head_pitch"] - _look_at.y * 0.22
	_head.rotation.y = _pose["head_yaw"] + _look_at.x * 0.35
	_head.rotation.z = _pose["head_roll"]
	_head.position.y = HEAD_Y + 0.012 * breathe * bounce

	# Mâchoire : ouverture pendant la parole.
	var jaw := float(_pose["jaw"])
	if state == SPEAKING:
		jaw *= 0.55 + 0.45 * absf(sin(_t * 11.0))
	_jaw.rotation.x = jaw * 0.42

	# Bras.
	_arm_l.rotation.z = -_pose["arm_l"] - deg_to_rad(4.0) * breathe * bounce
	_arm_r.rotation.z = _pose["arm_r"] + deg_to_rad(4.0) * breathe * bounce

	# Oreilles : mouvement Fin.
	var ear: float = _pose["ear"]
	var twitch := deg_to_rad(9.0) * maxf(0.0, sin(_t * 0.9)) if fmod(_t, 5.3) < 0.4 else 0.0
	_ear_l.rotation.x = ear + twitch
	_ear_r.rotation.x = ear - twitch * 0.6

	# Yeux : clignement + suivi du regard.
	var lid := 1.0 - _blink
	for i in _eyes.size():
		_eyes[i].scale.y = maxf(0.08, lid)
		var pupil := _pupils[i]
		pupil.position.x = lerpf(pupil.position.x, _look_at.x * 0.012, clampf(delta * 8.0, 0.0, 1.0))
		pupil.position.y = lerpf(pupil.position.y, -_look_at.y * 0.008, clampf(delta * 8.0, 0.0, 1.0))

	# Queue : remuage en cascade, plus rapide quand Jimmy est enthousiaste.
	var speed := 1.6 + 2.4 * bounce
	for i in _tail.size():
		var phase := _t * speed - i * 0.42
		var amount := (i + 1) * 0.13
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
	_outline.albedo_color = OUTLINE_COLOR
	_outline.grow = true
	_outline.grow_amount = OUTLINE_WIDTH

	var palette: Dictionary = SKINS.get(skin, SKINS[SKIN_DEFAULT])
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
	var fur_normal := _fur_normal_map()
	for mat in [_mat_fur, _mat_cream]:
		mat.rim_enabled = true
		mat.rim = 0.35
		mat.rim_tint = 0.6
		mat.normal_enabled = true
		mat.normal_texture = fur_normal
		mat.normal_scale = 0.45
		mat.uv1_scale = Vector3(3.0, 3.0, 1.0)


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

	_build_legs()
	_build_torso()
	_build_tail()


func _build_tail() -> void:
	# Queue en chaîne de sphères : chaque maillon reçoit le mouvement du
	# précédent avec un léger retard, ce qui donne un remuage naturel.
	var parent: Node3D = _body
	var offset := Vector3(0.0, 0.34, -0.14)
	for i in 4:
		var seg := Node3D.new()
		seg.name = "Tail%d" % (i + 1)
		seg.position = Vector3(0.0, 0.0, -0.10) if i > 0 else offset
		seg.rotation.x = deg_to_rad(-18.0) if i == 0 else deg_to_rad(6.0)
		parent.add_child(seg)

		var radius := 0.085 - i * 0.016
		var fluff := _sphere(radius, _mat_fur, 14)
		fluff.scale = Vector3(1.0, 1.0, 1.15)
		seg.add_child(fluff)

		if i == 3:
			var tip := _cone(radius * 0.95, 0.004, 0.11, _mat_cream)
			tip.rotation.x = deg_to_rad(-90.0)
			tip.position = Vector3(0.0, 0.0, -0.10)
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
		ear.scale = Vector3.ONE * float(SKINS.get(skin, SKINS[SKIN_DEFAULT])["ear_scale"])
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