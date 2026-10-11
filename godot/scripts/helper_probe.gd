extends Node3D

## La sonde : compagnon des tâches de fond du skin premium JEVA (chaque skin
## premium a le sien, le lapin reste aux skins de base). Une petite sphère
## chromée qui flotte, avec un œil cyan cerclé de rose comme le casque de
## JEVA, un anneau en orbite et un écran holographique.
##
## Même interface que `helper_rabbit.gd` (route `/helper`) :
##   - `hidden`  : invisible ;
##   - `working` : elle flotte, l'œil balaie l'écran holographique, l'anneau
##     tourne, l'écran scintille ;
##   - `success` : deux boucles vers le haut, anneau rapide, puis elle part ;
##   - `error`   : l'œil vire au rouge, elle s'affaisse, puis elle part.
##
## Tout est généré (primitives, matériaux) : seul le modèle de JEVA est un
## fichier, la sonde n'en a pas besoin.
##
## Thème « ark » (compagnon du skin ARK, `sonde_ark`) : coque de métal sombre,
## œil vert à pupille fendue, et trois petits tentacules articulés qui
## ondulent sous la sphère à la place des ailettes.

const HIDDEN := "hidden"
const WORKING := "working"
const SUCCESS := "success"
const ERROR := "error"

const OUTRO_SECONDS := 2.6
const POP_SECONDS := 0.35
## Hauteur de vol (centre de la sphère) : dans la boîte de `_helper_rect`
## (main.gd : y de -0,02 à 0,88).
const HOVER_Y := 0.56
## Lacet de base du corps : le compagnon est tourné de -28° vers le
## personnage (main.gd) ; +22° ramène l'œil presque face à l'utilisateur.
const FACE_YAW := 22.0
## Tangage de base : la caméra voit la sonde d'environ 16° au-dessus ; l'œil
## se lève vers elle (sinon on ne voit que le dessus de la sphère).
const FACE_PITCH := -16.0

const IRIS := Color(0.20, 0.90, 1.00)
const RING := Color(1.00, 0.22, 0.75)
const ALERT := Color(1.00, 0.18, 0.12)
const ARK_GREEN := Color(0.30, 1.00, 0.38)

## Thème (posé avant l'entrée dans l'arbre) : "" = sonde de JEVA, "ark".
var theme := ""
## Couleurs de l'œil selon le thème (voir `_build`).
var _iris := IRIS
## Petits tentacules du thème « ark » : une chaîne de maillons chacun.
var _legs: Array = []

var state: String = HIDDEN
var _t := 0.0
var _state_t := 0.0
var _pop := 0.0

var _root: Node3D
## Corps flottant (sphère, œil, ailettes) : porte le vol et l'inclinaison.
var _core: Node3D
var _eye: Node3D
var _pupil: Node3D
var _orbit: Node3D
var _iris_mat: StandardMaterial3D
var _screen_mat: StandardMaterial3D
var _screen: Node3D
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
	print("[godot/sonde] état : %s" % state)
	return true


func _process(delta: float) -> void:
	if not visible:
		return
	_t += delta
	_state_t += delta

	# Apparition / disparition : même « pop » d'échelle que le lapin.
	var wanted_pop := 1.0
	if state == HIDDEN or ((state == SUCCESS or state == ERROR) and _state_t > OUTRO_SECONDS):
		wanted_pop = 0.0
	_pop = move_toward(_pop, wanted_pop, delta / POP_SECONDS)
	var eased := _pop * (1.0 + 0.25 * sin(_pop * PI))
	_root.scale = Vector3.ONE * maxf(eased, 0.001)
	if _pop <= 0.0 and wanted_pop == 0.0:
		visible = false
		state = HIDDEN
		return

	_animate_legs()
	match state:
		WORKING:
			_animate_working()
		SUCCESS:
			_animate_success()
		ERROR:
			_animate_error()


## Tentacules du thème « ark » : onde qui descend vers la pointe, plus vive
## au travail, pendants et mous après un échec.
func _animate_legs() -> void:
	var speed := 3.2 if state == WORKING else 2.0
	var amp := 0.10 if state == ERROR else 0.28
	for l in _legs.size():
		var chain: Array = _legs[l]
		for i in chain.size():
			var seg: Node3D = chain[i]
			var phase := _t * speed - i * 0.8 + l * 2.1
			seg.rotation.x = amp * sin(phase) * (0.5 + 0.2 * i)
			seg.rotation.z = amp * 0.6 * cos(phase * 0.9) * (0.5 + 0.2 * i)


func _animate_working() -> void:
	# Vol stationnaire, l'œil balaie l'écran de gauche à droite.
	_core.position.y = HOVER_Y + 0.025 * sin(_t * 2.1)
	_core.rotation = Vector3(deg_to_rad(FACE_PITCH), deg_to_rad(FACE_YAW) + 0.22 * sin(_t * 1.4), 0.03 * sin(_t * 1.1))
	_pupil.position.x = 0.012 * sin(_t * 3.2)
	_orbit.rotation.y += 0.045
	_orbit.rotation.x = deg_to_rad(18.0)
	_iris_mat.albedo_color = _iris
	_iris_mat.emission = _iris
	_iris_mat.emission_energy_multiplier = 0.9 + 0.3 * absf(sin(_t * 4.0))
	_screen.visible = true
	_screen_mat.emission_energy_multiplier = 0.9 + 0.5 * absf(sin(_t * 7.3)) * absf(sin(_t * 2.9))


func _animate_success() -> void:
	# Deux boucles vers le haut, anneau qui s'emballe, œil vif.
	var k := minf(_state_t, 1.6)
	_core.position.y = HOVER_Y + 0.12 * absf(sin(k * PI / 0.8))
	_core.rotation = Vector3(deg_to_rad(FACE_PITCH), deg_to_rad(FACE_YAW) + (k * TAU / 0.8 if _state_t < 1.6 else 0.0), 0.0)
	_pupil.position.x = 0.0
	_orbit.rotation.y += 0.16
	_iris_mat.emission_energy_multiplier = 1.6
	_screen.visible = false


func _animate_error() -> void:
	# Échec : l'œil rougit et vacille, la sonde s'affaisse et penche.
	var k := minf(_state_t / 0.6, 1.0)
	_core.position.y = lerpf(HOVER_Y, HOVER_Y - 0.12, k)
	# Penche sans cacher l'œil rouge : c'est lui qui dit « raté ».
	_core.rotation = Vector3(deg_to_rad(FACE_PITCH + 22.0 * k), deg_to_rad(FACE_YAW), deg_to_rad(-16.0) * k)
	_pupil.position.x = 0.0
	_iris_mat.albedo_color = ALERT
	_iris_mat.emission = ALERT
	# Grésillement : la lumière saute par moments.
	_iris_mat.emission_energy_multiplier = 0.3 + (0.9 if fmod(_state_t * 9.0, 1.0) < 0.6 else 0.0)
	_screen.visible = false


# ── Construction ────────────────────────────────────────────────────────────

func _build() -> void:
	# Contour fin et sombre, comme le lapin : lisible sur un bureau chargé.
	_outline = StandardMaterial3D.new()
	_outline.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	_outline.cull_mode = BaseMaterial3D.CULL_FRONT
	_outline.albedo_color = Color(0.06, 0.08, 0.14)
	_outline.grow = true
	_outline.grow_amount = 0.005

	var ark := theme == "ark"
	_iris = ARK_GREEN if ark else IRIS
	var ring_color := Color(0.20, 0.95, 0.35) if ark else RING
	var chrome := _material(Color(0.16, 0.17, 0.16) if ark else Color(0.80, 0.84, 0.90), 0.32 if ark else 0.22)
	chrome.metallic = 0.55
	chrome.rim_enabled = true
	chrome.rim = 0.5
	chrome.rim_tint = 0.2
	var dark := _material(Color(0.04, 0.045, 0.04) if ark else Color(0.07, 0.08, 0.13), 0.4)
	# Boîtes sans contour « inverted hull » (piège 18 : il les déforme).
	var dark_plain := _material(Color(0.07, 0.08, 0.13), 0.4, false)
	var pupil := _material(Color(0.03, 0.03, 0.05), 0.3, false)
	var ring := _glow(ring_color, 1.2)
	_iris_mat = _glow(_iris, 0.9)
	var orbit := _glow(_iris, 0.8)
	_screen_mat = _glow(_iris, 1.0)
	_screen_mat.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
	_screen_mat.albedo_color = Color(_iris.r, _iris.g, _iris.b, 0.45)

	_root = Node3D.new()
	_root.name = "Probe"
	add_child(_root)

	_core = Node3D.new()
	_core.name = "Core"
	_core.position.y = HOVER_Y
	_root.add_child(_core)

	# Sphère chromée et sa bande sombre (la « visière »).
	var shell := _sphere(0.13, chrome)
	_core.add_child(shell)
	var band := _sphere(0.118, dark)
	band.position = Vector3(0.0, 0.0, 0.035)
	band.scale = Vector3(1.0, 0.72, 1.0)
	_core.add_child(band)

	# Œil : anneau rose, iris cyan, pupille qui balaie.
	_eye = Node3D.new()
	_eye.name = "Eye"
	# Devant la bande sombre (qui avance jusqu'à z = 0,153), sinon elle le cache.
	_eye.position = Vector3(0.0, 0.0, 0.158)
	_core.add_child(_eye)
	var torus := TorusMesh.new()
	torus.inner_radius = 0.050
	torus.outer_radius = 0.064
	torus.rings = 24
	torus.ring_segments = 8
	torus.material = ring
	var ring_node := MeshInstance3D.new()
	ring_node.mesh = torus
	ring_node.rotation.x = deg_to_rad(90.0)
	_eye.add_child(ring_node)
	var iris := _sphere(0.048, _iris_mat)
	iris.scale = Vector3(1.0, 1.0, 0.35)
	_eye.add_child(iris)
	_pupil = _sphere(0.017, pupil)
	_pupil.position = Vector3(0.0, 0.0, 0.016)
	_pupil.scale = Vector3(0.32, 1.3, 0.5) if ark else Vector3(1.0, 1.0, 0.5)
	_eye.add_child(_pupil)

	if ark:
		_build_legs(dark_plain, ring)
	# Ailettes latérales : stabilisateurs de vol (sonde de JEVA).
	for side in ([] if ark else [-1.0, 1.0]):
		var fin := _box(Vector3(0.10, 0.018, 0.06), dark_plain)
		fin.position = Vector3(0.15 * side, -0.01, -0.01)
		fin.rotation.z = deg_to_rad(-12.0 * side)
		_core.add_child(fin)
		var tip := _sphere(0.014, ring)
		tip.position = Vector3(0.20 * side, -0.02, -0.01)
		_core.add_child(tip)

	# Anneau en orbite autour de la sphère (incliné, il tourne).
	_orbit = Node3D.new()
	_orbit.name = "Orbit"
	_core.add_child(_orbit)
	var halo := TorusMesh.new()
	halo.inner_radius = 0.185
	halo.outer_radius = 0.196
	halo.rings = 40
	halo.ring_segments = 6
	halo.material = orbit
	var halo_node := MeshInstance3D.new()
	halo_node.mesh = halo
	_orbit.add_child(halo_node)

	# Écran holographique devant elle, un peu sur le côté (comme l'ordinateur
	# du lapin) : il ne s'affiche qu'au travail.
	_screen = Node3D.new()
	_screen.name = "Screen"
	_screen.position = Vector3(-0.12, 0.40, 0.24)
	_screen.rotation = Vector3(deg_to_rad(-10.0), deg_to_rad(25.0), 0.0)
	_root.add_child(_screen)
	var pane := _box(Vector3(0.22, 0.14, 0.004), _screen_mat)
	_screen.add_child(pane)
	for i in 4:
		var line := _box(Vector3(0.05 + 0.03 * (i % 3), 0.008, 0.002), ring if i == 1 else orbit)
		line.position = Vector3(-0.05, 0.045 - i * 0.03, 0.004)
		_screen.add_child(line)


## Trois tentacules sous la sphère (thème « ark ») : chaque maillon est
## l'enfant du précédent, la rotation se cumule jusqu'à la pointe lumineuse.
func _build_legs(metal: StandardMaterial3D, glow: StandardMaterial3D) -> void:
	for l in 3:
		var a := TAU * l / 3.0 + 0.5
		var parent: Node3D = Node3D.new()
		parent.position = Vector3(0.09 * cos(a), -0.10, 0.09 * sin(a))
		parent.rotation = Vector3(0.35 * sin(a), 0.0, -0.35 * cos(a))
		_core.add_child(parent)
		var chain: Array = []
		for i in 4:
			var seg := Node3D.new()
			seg.position = Vector3(0.0, -0.045 if i > 0 else 0.0, 0.0)
			parent.add_child(seg)
			seg.add_child(_sphere(0.016 - i * 0.002, metal))
			var link := _box(Vector3(0.016 - i * 0.002, 0.04, 0.016 - i * 0.002), metal)
			link.position.y = -0.022
			seg.add_child(link)
			chain.append(seg)
			parent = seg
		var tip := _sphere(0.010, glow)
		tip.position.y = -0.045
		parent.add_child(tip)
		_legs.append(chain)


# ── Primitives (même principe que helper_rabbit.gd) ─────────────────────────

func _material(color: Color, roughness: float, outlined := true) -> StandardMaterial3D:
	var mat := StandardMaterial3D.new()
	mat.albedo_color = color
	mat.roughness = roughness
	mat.metallic_specular = 0.4
	if outlined:
		mat.next_pass = _outline
	return mat


func _glow(color: Color, energy: float) -> StandardMaterial3D:
	var mat := StandardMaterial3D.new()
	mat.albedo_color = color
	mat.roughness = 0.3
	mat.emission_enabled = true
	mat.emission = color
	mat.emission_energy_multiplier = energy
	return mat


func _sphere(radius: float, mat: Material) -> MeshInstance3D:
	var mesh := SphereMesh.new()
	mesh.radius = radius
	mesh.height = radius * 2.0
	mesh.radial_segments = 24
	mesh.rings = 12
	mesh.material = mat
	var node := MeshInstance3D.new()
	node.mesh = mesh
	return node


func _box(size: Vector3, mat: Material) -> MeshInstance3D:
	var mesh := BoxMesh.new()
	mesh.size = size
	mesh.material = mat
	var node := MeshInstance3D.new()
	node.mesh = mesh
	return node
