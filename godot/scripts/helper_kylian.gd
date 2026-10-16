extends Node3D

## Mini Kylian : compagnon des tâches de fond du skin premium Thibault
## (chaque skin premium a le sien, le lapin reste aux skins de base).
## Un petit footballeur chauve en maillot bleu marine, avec son ballon.
##
## Même interface que `helper_rabbit.gd` (route `/helper`) :
##   - `hidden`  : invisible ;
##   - `working` : il jongle (ballon qui roule d'un pied à l'autre, frappes
##     alternées, tête qui suit le ballon) ;
##   - `success` : il saute, bras en V, ballon levé, puis il part ;
##   - `error`   : tête basse, bras ballants, ballon immobile, puis il part.
##
## Tout est généré (primitives, matériaux) : seul le modèle de Thibault est
## un fichier, le compagnon n'en a pas besoin.

const HIDDEN := "hidden"
const WORKING := "working"
const SUCCESS := "success"
const ERROR := "error"

const OUTRO_SECONDS := 2.6
const POP_SECONDS := 0.35
## Le ballon roule entre les pieds (±0,09 autour de sa place).
const DRIBBLE_X := 0.09

## Peau mate, maillot bleu marine, ballon blanc à pastilles noires.
const SKIN := Color(0.52, 0.35, 0.25)
const SHIRT := Color(0.06, 0.10, 0.30)
const SHORT := Color(0.05, 0.08, 0.22)
const BALL := Color(0.95, 0.95, 0.96)
const PATCH := Color(0.05, 0.05, 0.06)

var theme := ""

var state: String = HIDDEN
var _t := 0.0
var _state_t := 0.0
## Échelle d'apparition (0 = invisible, 1 = présent).
var _pop := 0.0

var _root: Node3D
var _body: Node3D
var _head: Node3D
var _arm_l: Node3D
var _arm_r: Node3D
var _leg_l: Node3D
var _leg_r: Node3D
var _ball: Node3D
var _ball_home := Vector3(0.17, 0.075, 0.06)


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
	print("[godot/kylian] état : %s" % state)
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
	# Jongle : le ballon roule d'un pied à l'autre, le pied proche frappe.
	var phase := _t * 5.2
	var side := sin(phase)
	_ball.position = Vector3(_ball_home.x + DRIBBLE_X * side, _ball_home.y + 0.02 * absf(cos(phase)), _ball_home.z)
	_ball.rotation.z -= 0.12 * cos(phase)
	_leg_l.rotation.x = deg_to_rad(-30.0) + (0.5 * maxf(0.0, -side) if side < 0.0 else 0.0)
	_leg_r.rotation.x = deg_to_rad(-30.0) + (0.5 * maxf(0.0, side) if side > 0.0 else 0.0)
	# La tête suit le ballon, le corps respire.
	_head.rotation.y = 0.35 * side
	_head.rotation.x = deg_to_rad(10.0)
	_body.position.y = 0.012 * sin(_t * 3.4)
	_arm_l.rotation.z = deg_to_rad(-12.0) + 0.06 * sin(_t * 2.1)
	_arm_r.rotation.z = deg_to_rad(12.0) - 0.06 * sin(_t * 2.1)
	_root.position.y = 0.0


func _animate_success() -> void:
	# But ! Saut, bras en V, ballon levé au-dessus de la tête.
	var hop := absf(sin(minf(_state_t, 1.6) * PI / 0.8))
	_root.position.y = 0.14 * hop if _state_t < 1.6 else 0.0
	_arm_l.rotation = Vector3(0.0, 0.0, deg_to_rad(-140.0))
	_arm_r.rotation = Vector3(0.0, 0.0, deg_to_rad(140.0))
	_head.rotation.x = deg_to_rad(-8.0)
	_head.rotation.y = 0.0
	_leg_l.rotation.x = deg_to_rad(-10.0)
	_leg_r.rotation.x = deg_to_rad(-10.0)
	_ball.position = Vector3(0.0, 0.78 + 0.10 * hop, 0.05)
	_ball.rotation.y += 0.08


func _animate_error() -> void:
	# Occasion manquée : tête basse, bras ballants, ballon immobile.
	var k := minf(_state_t / 0.6, 1.0)
	_head.rotation.x = lerpf(deg_to_rad(10.0), deg_to_rad(34.0), k)
	_head.rotation.y = 0.0
	_arm_l.rotation = Vector3(0.0, 0.0, lerpf(deg_to_rad(-12.0), deg_to_rad(-4.0), k))
	_arm_r.rotation = Vector3(0.0, 0.0, lerpf(deg_to_rad(12.0), deg_to_rad(4.0), k))
	_leg_l.rotation.x = deg_to_rad(-30.0)
	_leg_r.rotation.x = deg_to_rad(-30.0)
	_ball.position = _ball_home
	_root.position.y = 0.0


# ── Construction ────────────────────────────────────────────────────────────

func _material(color: Color, rough: float = 0.7) -> StandardMaterial3D:
	var mat := StandardMaterial3D.new()
	mat.albedo_color = color
	mat.roughness = rough
	return mat


func _part_box(parent: Node3D, mat: Material, center: Vector3, size: Vector3) -> MeshInstance3D:
	var mesh := BoxMesh.new()
	mesh.size = size
	mesh.material = mat
	var mi := MeshInstance3D.new()
	mi.mesh = mesh
	mi.position = center
	parent.add_child(mi)
	return mi


func _part_ball(parent: Node3D, mat: Material, center: Vector3, radius: float) -> MeshInstance3D:
	var mesh := SphereMesh.new()
	mesh.radius = radius
	mesh.height = radius * 2.0
	mesh.radial_segments = 20
	mesh.rings = 12
	mesh.material = mat
	var mi := MeshInstance3D.new()
	mi.mesh = mesh
	mi.position = center
	parent.add_child(mi)
	return mi


func _build() -> void:
	var skin := _material(SKIN)
	var shirt := _material(SHIRT)
	var short := _material(SHORT)
	var ball_mat := _material(BALL, 0.5)
	var patch_mat := _material(PATCH, 0.6)
	var dark := _material(Color(0.03, 0.03, 0.04), 0.6)

	_root = Node3D.new()
	_root.name = "Root"
	add_child(_root)

	# Corps : torse + short.
	_body = Node3D.new()
	_body.name = "Body"
	_root.add_child(_body)
	_part_box(_body, shirt, Vector3(0.0, 0.40, 0.0), Vector3(0.17, 0.20, 0.11))
	_part_box(_body, short, Vector3(0.0, 0.26, 0.0), Vector3(0.15, 0.09, 0.10))

	# Jambes (pivots aux hanches, pour les frappes).
	for side in [-1.0, 1.0]:
		var leg := Node3D.new()
		leg.name = "LegL" if side < 0.0 else "LegR"
		leg.position = Vector3(side * 0.055, 0.22, 0.0)
		_body.add_child(leg)
		_part_box(leg, skin, Vector3(0.0, -0.09, 0.0), Vector3(0.055, 0.16, 0.06))
		_part_box(leg, dark, Vector3(0.0, -0.185, 0.02), Vector3(0.06, 0.045, 0.11))
		leg.rotation.x = deg_to_rad(-30.0)
		if side < 0.0:
			_leg_l = leg
		else:
			_leg_r = leg

	# Bras (pivots aux épaules).
	for side in [-1.0, 1.0]:
		var arm := Node3D.new()
		arm.name = "ArmL" if side < 0.0 else "ArmR"
		arm.position = Vector3(side * 0.115, 0.485, 0.0)
		_body.add_child(arm)
		_part_box(arm, shirt, Vector3(0.0, -0.035, 0.0), Vector3(0.05, 0.07, 0.055))
		_part_box(arm, skin, Vector3(0.0, -0.115, 0.0), Vector3(0.042, 0.10, 0.045))
		if side < 0.0:
			_arm_l = arm
		else:
			_arm_r = arm

	# Tête chauve + yeux.
	_head = Node3D.new()
	_head.name = "Head"
	_head.position = Vector3(0.0, 0.56, 0.0)
	_body.add_child(_head)
	_part_ball(_head, skin, Vector3.ZERO, 0.075)
	_part_ball(_head, dark, Vector3(-0.028, 0.012, 0.066), 0.009)
	_part_ball(_head, dark, Vector3(0.028, 0.012, 0.066), 0.009)

	# Ballon + pastilles.
	_ball = Node3D.new()
	_ball.name = "Ball"
	_ball.position = _ball_home
	_root.add_child(_ball)
	_part_ball(_ball, ball_mat, Vector3.ZERO, 0.075)
	_part_ball(_ball, patch_mat, Vector3(0.0, 0.075, 0.0), 0.022)
	_part_ball(_ball, patch_mat, Vector3(0.065, -0.02, 0.03), 0.022)
	_part_ball(_ball, patch_mat, Vector3(-0.06, -0.03, -0.035), 0.022)
	_part_ball(_ball, patch_mat, Vector3(0.01, 0.02, -0.07), 0.022)
