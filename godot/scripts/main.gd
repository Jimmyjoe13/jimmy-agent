extends Node
## Point d'entrée de l'avatar Godot.
##
## Responsabilités (et uniquement celles-ci) :
##   - construire la scène 3D (caméra, lumières, personnage) ;
##   - afficher la bulle de dialogue ;
##   - répondre aux commandes HTTP envoyées par Tauri ;
##   - signaler les clics sur l'avatar à Tauri.
##
## Aucune logique d'agent ici : Godot ne sait rien du LLM, de la mémoire ou des
## outils. Il ne fait qu'exécuter des ordres d'animation.

const SKIN_DEFAULT := "renard"

const QUALITY_PRESETS := {
	"low": {"scale_3d": 0.62, "shadows": false, "msaa": Viewport.MSAA_DISABLED, "glow": false},
	"medium": {"scale_3d": 0.85, "shadows": true, "msaa": Viewport.MSAA_2X, "glow": false},
	"high": {"scale_3d": 1.0, "shadows": true, "msaa": Viewport.MSAA_4X, "glow": true},
}

const SIZE_WITH_BUBBLE := Vector2i(560, 620)
const SIZE_AVATAR_ONLY := Vector2i(560, 620)
const BUBBLE_HIDE_DELAY := 12.0

var skin: String = SKIN_DEFAULT
var port: int = 8787
var bridge_host: String = "127.0.0.1"
var bridge_port: int = 8790

var _jimmy: Jimmy
var _camera: Camera3D
var _http: JimmyHttpServer
var _bubble: PanelContainer
var _bubble_text: Label
var _bubble_hint: Label
var _state_label: Label
var _bridge: HTTPRequest
var _bubble_timer := 0.0
var _dragging := false
var _drag_offset := Vector2i.ZERO
var _moved := false
var _look_smooth := Vector2.ZERO
var _quality := "medium"


func _ready() -> void:
	_parse_cmdline()
	_setup_window()
	_build_scene()
	_setup_bubble()
	_start_http()
	print("[godot/main] prêt — skin=%s port=%d état=%s" % [skin, port, _jimmy.state])


func _parse_cmdline() -> void:
	for arg in OS.get_cmdline_user_args():
		if arg.begins_with("--skin="):
			skin = arg.trim_prefix("--skin=")
		elif arg.begins_with("--port="):
			port = int(arg.trim_prefix("--port="))
		elif arg.begins_with("--bridge-port="):
			bridge_port = int(arg.trim_prefix("--bridge-port="))
		elif arg.begins_with("--bridge-host="):
			bridge_host = arg.trim_prefix("--bridge-host=")
		elif arg.begins_with("--quality="):
			_quality = arg.trim_prefix("--quality=").to_lower()
	# Variables d'environnement (positionnées par Tauri au lancement)
	var env_port := OS.get_environment("JIMMY_GODOT_PORT")
	if env_port != "":
		port = int(env_port)
	var env_bridge := OS.get_environment("JIMMY_BRIDGE_PORT")
	if env_bridge != "":
		bridge_port = int(env_bridge)
	var env_skin := OS.get_environment("JIMMY_SKIN")
	if env_skin != "":
		skin = env_skin


func _setup_window() -> void:
	var screen := DisplayServer.screen_get_usable_rect(DisplayServer.window_get_current_screen())
	var w := SIZE_AVATAR_ONLY.x
	var h := SIZE_AVATAR_ONLY.y
	DisplayServer.window_set_size(Vector2i(w, h))
	# Position initiale : bas à droite du bureau, comme un assistant.
	var pos := Vector2i(screen.position.x + screen.size.x - w - 40, screen.position.y + screen.size.y - h - 20)
	DisplayServer.window_set_position(pos)


func _build_scene() -> void:
	# Environnement transparent : le personnage flotte sur le bureau.
	var env := Environment.new()
	env.background_mode = Environment.BG_COLOR
	env.background_color = Color(0, 0, 0, 0)
	env.ambient_light_source = Environment.AMBIENT_SOURCE_COLOR
	env.ambient_light_color = Color(0.62, 0.66, 0.74)
	env.ambient_light_energy = 0.85
	env.tonemap_mode = Environment.TONE_MAPPER_FILMIC

	var world_env := WorldEnvironment.new()
	world_env.name = "WorldEnvironment"
	world_env.environment = env
	add_child(world_env)

	var key_light := DirectionalLight3D.new()
	key_light.name = "KeyLight"
	key_light.rotation_degrees = Vector3(-38.0, -52.0, 0.0)
	key_light.light_energy = 1.35
	key_light.light_color = Color(1.0, 0.96, 0.90)
	key_light.shadow_enabled = true
	add_child(key_light)

	var rim_light := OmniLight3D.new()
	rim_light.name = "RimLight"
	rim_light.position = Vector3(-1.1, 1.5, -0.9)
	rim_light.light_energy = 2.2
	rim_light.omni_range = 4.0
	rim_light.light_color = Color(0.55, 0.72, 1.0)
	add_child(rim_light)

	var fill_light := OmniLight3D.new()
	fill_light.name = "FillLight"
	fill_light.position = Vector3(1.0, 0.9, 1.1)
	fill_light.light_energy = 0.9
	fill_light.omni_range = 3.5
	fill_light.light_color = Color(1.0, 0.85, 0.7)
	add_child(fill_light)

	var camera := Camera3D.new()
	camera.name = "Camera"
	camera.fov = 34.0
	camera.position = Vector3(0.0, 0.75, 6.60)
	camera.v_offset = 0.62
	camera.current = true
	add_child(camera)
	_camera = camera

	_jimmy = Jimmy.new()
	_jimmy.name = "Jimmy"
	add_child(_jimmy)

	_bridge = HTTPRequest.new()
	_bridge.name = "Bridge"
	_bridge.timeout = 3.0
	add_child(_bridge)

	_apply_quality(_quality)


func _setup_bubble() -> void:
	var layer := CanvasLayer.new()
	layer.name = "UiLayer"
	add_child(layer)

	_bubble = PanelContainer.new()
	_bubble.name = "Bubble"
	_bubble.set_anchors_preset(Control.PRESET_TOP_WIDE)
	_bubble.offset_left = 24
	_bubble.offset_right = -24
	_bubble.offset_top = 16
	_bubble.offset_bottom = 244
	_bubble.mouse_filter = Control.MOUSE_FILTER_IGNORE
	layer.add_child(_bubble)

	var style := StyleBoxFlat.new()
	# Fond volontairement opaque : la bulle doit rester lisible devant n'importe
	# quoi sur le bureau.
	style.bg_color = Color(0.09, 0.10, 0.13, 0.985)
	style.set_corner_radius_all(18)
	style.border_width_bottom = 3
	style.border_color = Color(0.95, 0.62, 0.16, 0.85)
	style.content_margin_left = 18
	style.content_margin_right = 18
	style.content_margin_top = 14
	style.content_margin_bottom = 14
	_bubble.add_theme_stylebox_override("panel", style)

	var box := VBoxContainer.new()
	box.add_theme_constant_override("separation", 8)
	_bubble.add_child(box)

	_state_label = Label.new()
	_state_label.text = "idle"
	_state_label.add_theme_font_size_override("font_size", 12)
	_state_label.add_theme_color_override("font_color", Color(0.95, 0.62, 0.16))
	box.add_child(_state_label)

	_bubble_text = Label.new()
	_bubble_text.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_bubble_text.custom_minimum_size = Vector2(0, 96)
	_bubble_text.vertical_alignment = VERTICAL_ALIGNMENT_TOP
	_bubble_text.add_theme_font_size_override("font_size", 15)
	_bubble_text.add_theme_color_override("font_color", Color(0.92, 0.93, 0.96))
	_bubble_text.add_theme_constant_override("line_spacing", 4)
	box.add_child(_bubble_text)

	_bubble_hint = Label.new()
	_bubble_hint.text = "Clic pour ouvrir l'interface  ·  glissez pour déplacer Jimmy"
	_bubble_hint.add_theme_font_size_override("font_size", 11)
	_bubble_hint.add_theme_color_override("font_color", Color(0.55, 0.58, 0.65))
	box.add_child(_bubble_hint)

	_bubble.visible = false
	_apply_bubble_visibility()


func _start_http() -> void:
	_http = JimmyHttpServer.new()
	_http.name = "HttpServer"
	_http.request_received.connect(_on_http_request)
	add_child(_http)
	var err := _http.start(port)
	if err != OK:
		push_error("[godot/main] serveur HTTP indisponible sur le port %d" % port)


# ── Commandes HTTP reçues de Tauri ─────────────────────────────────────────

func _on_http_request(method: String, path: String, body: Dictionary) -> void:
	print("[godot/http] %s %s" % [method, path])
	match path:
		"/health":
			# La réponse a déjà été envoyée par le serveur ; rien à faire.
			pass
		"/state":
			var new_state := str(body.get("state", "idle"))
			_jimmy.set_state(new_state)
			_state_label.text = new_state
			if new_state == "listening":
				_show_bubble("J'écoute…", 6.0)
		"/say":
			var text := str(body.get("text", ""))
			var duration := float(body.get("duration_ms", 0)) / 1000.0
			_jimmy.speak_for(duration if duration > 0.0 else _estimate_speech(text))
			_show_bubble(text, _jimmy._say_until - _jimmy._t)
		"/quality":
			_apply_quality(str(body.get("level", "medium")).to_lower())
		"/skin":
			var wanted := str(body.get("skin", SKIN_DEFAULT))
			if wanted != skin:
				skin = wanted
				print("[godot/main] skin demandé : %s (V1 : seul « renard » est implémenté)" % skin)
		"/position":
			DisplayServer.window_set_position(Vector2i(int(body.get("x", 0)), int(body.get("y", 0))))
		"/hide":
			_hide_bubble()
		_:
			print("[godot/main] route inconnue : %s %s" % [method, path])


func _estimate_speech(text: String) -> float:
	# Débit retenu lors des essais : environ 1 000 caractères par minute.
	return clampf(text.length() / 1000.0 * 60.0, 1.2, 45.0)


# ── Bulle ───────────────────────────────────────────────────────────────────

func _show_bubble(text: String, seconds: float) -> void:
	_bubble_text.text = text
	_bubble.visible = true
	_bubble_timer = maxf(BUBBLE_HIDE_DELAY, seconds + 2.0)
	print("[godot/bubble] affichée : « %s » (visible=%s, taille=%s)" % [
		text.substr(0, 40), str(_bubble.visible), str(_bubble.size)])
	_apply_bubble_visibility()


func _hide_bubble() -> void:
	_bubble.visible = false
	_bubble_timer = 0.0
	_apply_bubble_visibility()


func _apply_bubble_visibility() -> void:
	# La fenêtre garde désormais une taille constante : la bulle apparaît et
	# disparaît par-dessus l'avatar, ce qui évite que la fenêtre « saute » sous
	# le curseur et rend le glissement bien plus agréable.
	pass


# ── Interaction ─────────────────────────────────────────────────────────────

func _process(delta: float) -> void:
	# Suivi du regard : Jimmy regarde le curseur quand il est dans sa fenêtre.
	var local := get_viewport().get_mouse_position()
	var center := get_viewport().get_visible_rect().size * 0.5
	_look_smooth = _look_smooth.lerp((local - center) / (center * 0.75), clampf(delta * 5.0, 0.0, 1.0))
	_jimmy.set_look_at(_look_smooth)

	if _bubble.visible and _bubble_timer > 0.0:
		_bubble_timer -= delta
		if _bubble_timer <= 0.0:
			_hide_bubble()


func _unhandled_input(event: InputEvent) -> void:
	if event is InputEventMouseButton:
		var mb := event as InputEventMouseButton
		if mb.button_index == MOUSE_BUTTON_LEFT:
			if mb.pressed:
				_dragging = true
				_moved = false
				var win := DisplayServer.window_get_position()
				_drag_offset = Vector2i(int(mb.position.x) - win.x, int(mb.position.y) - win.y)
			else:
				if _dragging and not _moved:
					_on_avatar_clicked()
				_dragging = false
	elif event is InputEventMouseMotion and _dragging:
		var mm := event as InputEventMouseMotion
		var target := Vector2i(int(mm.position.x) - _drag_offset.x, int(mm.position.y) - _drag_offset.y)
		if absi(target.x - DisplayServer.window_get_position().x) > 2 or absi(target.y - DisplayServer.window_get_position().y) > 2:
			_moved = true
		DisplayServer.window_set_position(target)


func _on_avatar_clicked() -> void:
	print("[godot/main] clic sur l'avatar → ouverture de l'interface Tauri")
	_bridge.request("http://%s:%d/ui/open" % [bridge_host, bridge_port],
		PackedStringArray(["Content-Type: application/json"]),
		HTTPClient.METHOD_POST, JSON.stringify({}))


# ── Profils graphiques ──────────────────────────────────────────────────────

func _apply_quality(level: String) -> void:
	var preset: Dictionary = QUALITY_PRESETS.get(level, QUALITY_PRESETS["medium"])
	_quality = level
	var vp := get_viewport()
	vp.scaling_3d_scale = preset["scale_3d"]
	vp.msaa_3d = preset["msaa"]
	vp.use_taa = false

	for light in [get_node_or_null("KeyLight"), get_node_or_null("RimLight"), get_node_or_null("FillLight")]:
		if light != null:
			light.shadow_enabled = preset["shadows"]

	var env_node := get_node_or_null("WorldEnvironment")
	if env_node != null:
		env_node.environment.glow_enabled = preset["glow"]
		if preset["glow"]:
			env_node.environment.glow_intensity = 0.5
			env_node.environment.glow_bloom = 0.15

	Engine.max_fps = 60 if level != "low" else 30
	print("[godot/main] profil graphique : %s" % level)