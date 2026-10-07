extends Node3D
class_name HelperRabbit

## Le lapin assistant : il apparaît à côté du renard pendant qu'une tâche
## tourne **en arrière-plan** (route `/helper`, décision du 7 octobre au soir).
##
## États :
##   - `hidden`  : invisible (rien n'est rendu) ;
##   - `working` : assis devant son mini-ordinateur, il tape, les oreilles
##     bougent, un engrenage tourne au-dessus de lui, l'écran scintille ;
##   - `success` : deux sauts de joie, bras levés, puis il disparaît ;
##   - `error`   : oreilles et tête basses (échec ou arrêt), puis il disparaît.
##
## Comme le renard : aucune ressource externe, tout est généré (primitives,
## matériaux, contour « inverted hull » sauf sur les cônes, piège 18).

const HIDDEN := "hidden"
const WORKING := "working"
const SUCCESS := "success"
const ERROR := "error"

## Durée de la fin (joie ou déception) avant de disparaître.
const OUTRO_SECONDS := 2.6
## Durée de l'apparition / disparition (échelle 0 → 1).
const POP_SECONDS := 0.35

const OUTLINE_WIDTH := 0.006
const OUTLINE_COLOR := Color(0.16, 0.10, 0.10)

var state: String = HIDDEN
var _t := 0.0
var _state_t := 0.0
## Échelle d'apparition (0 = invisible, 1 = présent).
var _pop := 0.0

var _root: Node3D
var _body: Node3D
var _head: Node3D
var _ear_l: Node3D
var _ear_r: Node3D
var _paw_l: Node3D
var _paw_r: Node3D
var _gear: Node3D
## Rotor de l'engrenage (tourne sur son axe ; `_gear` porte l'inclinaison).
var _gear_spin: Node3D
var _screen_mat: StandardMaterial3D
var _outline: StandardMaterial3D


func _ready() -> void:
	_build()
	visible = false


## Change d'état. Renvoie `false` pour un état inconnu.
func set_helper_state(new_state: String) -> bool:
	if not [HIDDEN, WORKING, SUCCESS, ERROR].has(new_state):
		return false
	if new_state == state:
		return true
	# Une fin sans tâche affichée n'a rien à montrer.
	if (new_state == SUCCESS or new_state == ERROR) and state == HIDDEN:
		return true
	state = new_state
	_state_t = 0.0
	if state != HIDDEN:
		visible = true
	print("[godot/lapin] état : %s" % state)
	return true


func _process(delta: float) -> void:
	if not visible:
		return
	_t += delta
	_state_t += delta

	# Apparition / disparition : pop d'échelle, puis plus rien n'est rendu.
	var wanted_pop := 1.0
	if state == HIDDEN or ((state == SUCCESS or state == ERROR) and _state_t > OUTRO_SECONDS):
		wanted_pop = 0.0
	_pop = move_toward(_pop, wanted_pop, delta / POP_SECONDS)
	# Léger dépassement à l'apparition : un « pop » plutôt qu'un fondu.
	var eased := _pop * (1.0 + 0.25 * sin(_pop * PI))
	_root.scale = Vector3.ONE * maxf(eased, 0.001)
	if _pop <= 0.0 and wanted_pop == 0.0:
		visible = false
		state = HIDDEN
		return

	match state:
		WORKING:
			_animate_working()
		SUCCESS:
			_animate_success()
		ERROR:
			_animate_error()


func _animate_working() -> void:
	# Frappe au clavier : pattes en alternance, rapide et irrégulière.
	var tap := _t * 11.0
	_paw_l.rotation = Vector3(deg_to_rad(-55.0) + 0.22 * maxf(0.0, sin(tap)), 0.0, 0.0)
	_paw_r.rotation = Vector3(deg_to_rad(-55.0) + 0.22 * maxf(0.0, sin(tap + PI * 0.9)), 0.0, 0.0)
	# Tête qui hoche en lisant l'écran, corps qui respire.
	_head.rotation.x = deg_to_rad(14.0) + 0.05 * sin(_t * 2.3)
	_head.rotation.z = 0.04 * sin(_t * 0.9)
	_body.position.y = 0.012 * sin(_t * 3.1)
	# Oreilles : balancement décalé, et un frémissement de temps en temps.
	var twitch := 0.25 * maxf(0.0, sin(_t * 0.7) - 0.92) * 12.0
	_ear_l.rotation.z = deg_to_rad(8.0) + 0.08 * sin(_t * 1.7) + twitch
	_ear_r.rotation.z = deg_to_rad(-8.0) + 0.08 * sin(_t * 1.9 + 1.0)
	_ear_l.rotation.x = 0.0
	_ear_r.rotation.x = 0.0
	# Engrenage qui tourne, écran qui scintille.
	_gear.visible = true
	_gear_spin.rotation.y += 0.05
	_gear.position.y = 0.60 + 0.015 * sin(_t * 2.0)
	_screen_mat.emission_energy_multiplier = 1.4 + 0.5 * absf(sin(_t * 7.3)) * absf(sin(_t * 2.9))
	_root.position.y = 0.0


func _animate_success() -> void:
	# Deux sauts, bras levés, oreilles droites.
	var hop := absf(sin(minf(_state_t, 1.6) * PI / 0.8))
	_root.position.y = 0.14 * hop if _state_t < 1.6 else 0.0
	# Bras en V, écartés sur les côtés : levés droit devant, la tête les
	# cachait (vérifié sur snapshot).
	_paw_l.rotation = Vector3(0.0, 0.0, deg_to_rad(-135.0) + 0.15 * sin(_state_t * 9.0))
	_paw_r.rotation = Vector3(0.0, 0.0, deg_to_rad(135.0) - 0.15 * sin(_state_t * 9.0))
	_head.rotation.x = deg_to_rad(-6.0)
	_ear_l.rotation.z = deg_to_rad(4.0)
	_ear_r.rotation.z = deg_to_rad(-4.0)
	_gear.visible = false
	_screen_mat.emission_energy_multiplier = 0.6


func _animate_error() -> void:
	# Déception : oreilles qui tombent sur les côtés (vers l'arrière, vu de
	# face, ça ne se voyait pas), tête basse, pattes posées.
	var k := minf(_state_t / 0.6, 1.0)
	_ear_l.rotation = Vector3(deg_to_rad(-20.0) * k, 0.0, lerpf(deg_to_rad(8.0), deg_to_rad(75.0), k))
	_ear_r.rotation = Vector3(deg_to_rad(-20.0) * k, 0.0, lerpf(deg_to_rad(-8.0), deg_to_rad(-75.0), k))
	_head.rotation.x = lerpf(deg_to_rad(14.0), deg_to_rad(32.0), k)
	_paw_l.rotation = Vector3(deg_to_rad(-40.0), 0.0, 0.0)
	_paw_r.rotation = Vector3(deg_to_rad(-40.0), 0.0, 0.0)
	_root.position.y = 0.0
	_gear.visible = false
	_screen_mat.emission_energy_multiplier = 0.2


# ── Construction ────────────────────────────────────────────────────────────

func _build() -> void:
	_outline = StandardMaterial3D.new()
	_outline.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	_outline.cull_mode = BaseMaterial3D.CULL_FRONT
	_outline.albedo_color = OUTLINE_COLOR
	_outline.grow = true
	_outline.grow_amount = OUTLINE_WIDTH

	var fur := _material(Color(0.93, 0.92, 0.90), 0.95)
	fur.rim_enabled = true
	fur.rim = 0.35
	fur.rim_tint = 0.6
	var belly := _material(Color(1.0, 0.98, 0.95), 0.95)
	var pink := _material(Color(0.97, 0.62, 0.70), 0.8)
	var eye := _material(Color(0.05, 0.04, 0.05), 0.3, false)
	var shine := _material(Color(1, 1, 1), 0.2, false)
	var laptop := _material(Color(0.22, 0.24, 0.28), 0.5)
	laptop.metallic = 0.4
	var keys := _material(Color(0.12, 0.13, 0.15), 0.7, false)
	var gear := _material(Color(0.96, 0.62, 0.04), 0.4, false)
	gear.emission_enabled = true
	gear.emission = Color(0.96, 0.62, 0.04)
	gear.emission_energy_multiplier = 0.5
	_screen_mat = _material(Color(0.35, 0.75, 1.0), 0.2, false)
	_screen_mat.emission_enabled = true
	_screen_mat.emission = Color(0.35, 0.75, 1.0)
	_screen_mat.emission_energy_multiplier = 1.5

	# Racine : sert à l'apparition (échelle) et aux sauts (hauteur).
	_root = Node3D.new()
	_root.name = "Rabbit"
	add_child(_root)

	_body = Node3D.new()
	_body.name = "Body"
	_root.add_child(_body)

	# Corps assis : une poire (sphère étirée) et un ventre clair.
	var trunk := _sphere(0.17, fur)
	trunk.position = Vector3(0.0, 0.17, 0.0)
	trunk.scale = Vector3(1.0, 1.05, 0.95)
	_body.add_child(trunk)
	var tummy := _sphere(0.12, belly)
	tummy.position = Vector3(0.0, 0.16, 0.075)
	tummy.scale = Vector3(1.0, 1.1, 0.7)
	_body.add_child(tummy)
	# Pattes arrière, posées au sol de part et d'autre.
	for side in [-1.0, 1.0]:
		var foot := _sphere(0.065, fur)
		foot.position = Vector3(0.11 * side, 0.04, 0.09)
		foot.scale = Vector3(0.8, 0.55, 1.5)
		_body.add_child(foot)
	# Queue en pompon.
	var tail := _sphere(0.06, belly)
	tail.position = Vector3(0.0, 0.10, -0.16)
	_body.add_child(tail)

	# Tête.
	_head = Node3D.new()
	_head.name = "Head"
	_head.position = Vector3(0.0, 0.36, 0.02)
	_body.add_child(_head)
	var skull := _sphere(0.13, fur)
	skull.scale = Vector3(1.05, 0.95, 1.0)
	_head.add_child(skull)
	for side in [-1.0, 1.0]:
		var cheek := _sphere(0.055, belly)
		cheek.position = Vector3(0.04 * side, -0.045, 0.095)
		_head.add_child(cheek)
		var e := _sphere(0.026, eye)
		e.position = Vector3(0.055 * side, 0.02, 0.112)
		e.scale = Vector3(0.85, 1.1, 0.6)
		_head.add_child(e)
		var glint := _sphere(0.008, shine)
		glint.position = Vector3(0.055 * side + 0.008, 0.032, 0.126)
		_head.add_child(glint)
	var nose := _sphere(0.016, pink)
	nose.position = Vector3(0.0, -0.025, 0.135)
	_head.add_child(nose)

	# Oreilles : longues capsules, intérieur rose, pivot à la base.
	_ear_l = _make_ear(-1.0, fur, pink)
	_ear_r = _make_ear(1.0, fur, pink)

	# Pattes avant : pivot à l'épaule, elles tapent sur le clavier.
	_paw_l = _make_paw(-1.0, fur)
	_paw_r = _make_paw(1.0, fur)

	# Mini-ordinateur posé devant lui : clavier à plat, écran tourné vers lui.
	var computer := Node3D.new()
	computer.name = "Laptop"
	computer.position = Vector3(0.0, 0.0, 0.26)
	_root.add_child(computer)
	var base := _box(Vector3(0.30, 0.018, 0.18), laptop)
	base.position = Vector3(0.0, 0.009, 0.0)
	computer.add_child(base)
	var keyboard := _box(Vector3(0.25, 0.004, 0.10), keys)
	keyboard.position = Vector3(0.0, 0.020, -0.015)
	computer.add_child(keyboard)
	var lid := Node3D.new()
	lid.position = Vector3(0.0, 0.018, 0.09)
	# Charnière côté opposé au lapin : l'écran se relève et lui fait face,
	# légèrement incliné vers l'arrière.
	lid.rotation.x = deg_to_rad(105.0)
	computer.add_child(lid)
	var lid_shell := _box(Vector3(0.30, 0.012, 0.19), laptop)
	lid_shell.position = Vector3(0.0, 0.0, -0.095)
	lid.add_child(lid_shell)
	var screen := _box(Vector3(0.26, 0.004, 0.15), _screen_mat)
	screen.position = Vector3(0.0, -0.008, -0.095)
	lid.add_child(screen)
	# Logo lumineux au dos du capot : c'est la face que voit l'utilisateur,
	# il scintille avec l'écran (l'ordinateur « tourne »).
	var logo := _cylinder(0.022, 0.004, _screen_mat)
	logo.position = Vector3(0.0, 0.008, -0.095)
	lid.add_child(logo)

	# Engrenage flottant, au-dessus et à côté de la tête (pas derrière une
	# oreille), de face : un disque et ses dents qui tournent sur leur axe.
	_gear = Node3D.new()
	_gear.name = "Gear"
	_gear.position = Vector3(0.20, 0.60, 0.0)
	_gear.rotation.x = deg_to_rad(90.0)
	_root.add_child(_gear)
	_gear_spin = Node3D.new()
	_gear.add_child(_gear_spin)
	var disc := _cylinder(0.05, 0.02, gear)
	_gear_spin.add_child(disc)
	for i in 8:
		var tooth := _box(Vector3(0.024, 0.02, 0.03), gear)
		var angle := TAU * i / 8.0
		tooth.position = Vector3(cos(angle) * 0.058, 0.0, sin(angle) * 0.058)
		tooth.rotation.y = -angle
		_gear_spin.add_child(tooth)
	var hub := _cylinder(0.018, 0.026, laptop)
	_gear_spin.add_child(hub)


func _make_ear(side: float, fur: Material, pink: Material) -> Node3D:
	var pivot := Node3D.new()
	pivot.name = "Ear%s" % ("L" if side < 0 else "R")
	pivot.position = Vector3(0.05 * side, 0.10, -0.01)
	_head.add_child(pivot)
	var outer := _capsule(0.04, 0.30, fur)
	outer.position = Vector3(0.0, 0.14, 0.0)
	outer.scale = Vector3(1.0, 1.0, 0.55)
	pivot.add_child(outer)
	var inner := _capsule(0.024, 0.24, pink)
	inner.position = Vector3(0.0, 0.14, 0.016)
	inner.scale = Vector3(1.0, 1.0, 0.4)
	pivot.add_child(inner)
	return pivot


func _make_paw(side: float, fur: Material) -> Node3D:
	var shoulder := Node3D.new()
	shoulder.name = "Paw%s" % ("L" if side < 0 else "R")
	shoulder.position = Vector3(0.10 * side, 0.25, 0.06)
	_body.add_child(shoulder)
	var arm := _capsule(0.032, 0.15, fur)
	arm.position = Vector3(0.0, -0.065, 0.0)
	shoulder.add_child(arm)
	return shoulder


# ── Primitives (même principe que jimmy.gd) ─────────────────────────────────

func _material(color: Color, roughness: float, outlined := true) -> StandardMaterial3D:
	var mat := StandardMaterial3D.new()
	mat.albedo_color = color
	mat.roughness = roughness
	mat.metallic_specular = 0.35
	if outlined:
		mat.next_pass = _outline
	return mat


func _sphere(radius: float, mat: Material) -> MeshInstance3D:
	var mesh := SphereMesh.new()
	mesh.radius = radius
	mesh.height = radius * 2.0
	mesh.radial_segments = 18
	mesh.rings = 9
	mesh.material = mat
	var node := MeshInstance3D.new()
	node.mesh = mesh
	return node


func _capsule(radius: float, height: float, mat: Material) -> MeshInstance3D:
	var mesh := CapsuleMesh.new()
	mesh.radius = radius
	mesh.height = maxf(height, radius * 2.05)
	mesh.radial_segments = 14
	mesh.rings = 6
	mesh.material = mat
	var node := MeshInstance3D.new()
	node.mesh = mesh
	return node


## Boîtes et cylindres : sans contour (arêtes vives, voir piège 18).
func _box(size: Vector3, mat: StandardMaterial3D) -> MeshInstance3D:
	var mesh := BoxMesh.new()
	mesh.size = size
	mesh.material = _plain(mat)
	var node := MeshInstance3D.new()
	node.mesh = mesh
	return node


func _cylinder(radius: float, height: float, mat: StandardMaterial3D) -> MeshInstance3D:
	var mesh := CylinderMesh.new()
	mesh.top_radius = radius
	mesh.bottom_radius = radius
	mesh.height = height
	mesh.radial_segments = 16
	mesh.material = _plain(mat)
	var node := MeshInstance3D.new()
	node.mesh = mesh
	return node


func _plain(mat: StandardMaterial3D) -> StandardMaterial3D:
	if mat.next_pass == null:
		return mat
	var plain := mat.duplicate() as StandardMaterial3D
	plain.next_pass = null
	return plain
