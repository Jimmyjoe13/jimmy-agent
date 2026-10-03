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

## Profils graphiques. La fenêtre fait 560×620 : le rendu à pleine résolution
## coûte peu, d'où `scale_3d = 1.0` dès `medium` (0,85 rendait l'avatar flou).
## `ssao` : occlusion ambiante, ce qui « soude » les primitives entre elles.
## `shadows` : ombre de la lumière clé, projetée au sol par le capteur d'ombre.
## `detail` : multiplicateur de tessellation des primitives du personnage.
const QUALITY_PRESETS := {
	"low": {"scale_3d": 0.75, "shadows": false, "msaa": Viewport.MSAA_DISABLED, "glow": false, "ssao": false, "detail": 1.0},
	"medium": {"scale_3d": 1.0, "shadows": true, "msaa": Viewport.MSAA_2X, "glow": false, "ssao": true, "detail": 1.5},
	"high": {"scale_3d": 1.0, "shadows": true, "msaa": Viewport.MSAA_4X, "glow": true, "ssao": true, "detail": 2.0},
}

## Taille constante : la bulle s'affiche par-dessus l'avatar, la fenêtre ne
## « saute » donc pas sous le curseur pendant un glissement.
const WINDOW_SIZE := Vector2i(560, 620)
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
	var w := WINDOW_SIZE.x
	var h := WINDOW_SIZE.y
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
	# AgX plutôt que FILMIC : FILMIC grisait les aplats, AgX conserve la teinte
	# des couleurs vives (l'orange du renard) quand elles s'éclaircissent.
	env.tonemap_mode = Environment.TONE_MAPPER_AGX
	# AgX désature légèrement : on compense, avec un peu de contraste.
	env.adjustment_enabled = true
	env.adjustment_saturation = 1.18
	env.adjustment_contrast = 1.06
	# Occlusion ambiante réglée à l'échelle du personnage (~1,6 unité de haut) :
	# un rayon de 1,0 par défaut assombrirait tout le corps uniformément.
	env.ssao_radius = 0.22
	env.ssao_intensity = 2.4
	env.ssao_power = 1.6
	env.ssao_detail = 0.6
	env.ssao_light_affect = 0.15

	var world_env := WorldEnvironment.new()
	world_env.name = "WorldEnvironment"
	world_env.environment = env
	add_child(world_env)

	var key_light := DirectionalLight3D.new()
	key_light.name = "KeyLight"
	# -50° plutôt que -38° : à -38° l'ombre portée s'étirait hors de la fenêtre.
	key_light.rotation_degrees = Vector3(-50.0, -40.0, 0.0)
	key_light.light_energy = 1.35
	key_light.light_color = Color(1.0, 0.96, 0.90)
	key_light.shadow_enabled = true
	key_light.shadow_blur = 1.6
	key_light.shadow_bias = 0.03
	key_light.shadow_normal_bias = 1.2
	# Une seule cascade : la scène tient dans 3 unités, plusieurs cascades
	# gaspilleraient la résolution de la carte d'ombre.
	key_light.directional_shadow_mode = DirectionalLight3D.SHADOW_ORTHOGONAL
	key_light.directional_shadow_max_distance = 12.0
	add_child(key_light)

	var rim_light := OmniLight3D.new()
	rim_light.name = "RimLight"
	rim_light.position = Vector3(-1.1, 1.5, -0.9)
	rim_light.light_energy = 2.2
	rim_light.omni_range = 4.0
	rim_light.light_color = Color(0.55, 0.72, 1.0)
	# N'éclaire pas le capteur d'ombre (calque 2), voir `_build_ground`.
	rim_light.light_cull_mask = 1
	add_child(rim_light)

	var fill_light := OmniLight3D.new()
	fill_light.name = "FillLight"
	fill_light.position = Vector3(1.0, 0.9, 1.1)
	fill_light.light_energy = 0.9
	fill_light.omni_range = 3.5
	fill_light.light_color = Color(1.0, 0.85, 0.7)
	fill_light.light_cull_mask = 1
	add_child(fill_light)

	var camera := Camera3D.new()
	camera.name = "Camera"
	# Cadrage : le renard (~1,6 unité) occupe la moitié basse de la fenêtre,
	# la bulle garde le haut. La légère plongée (8°) rend le sol lisible : sans
	# elle, l'ombre au sol est vue par la tranche et disparaît.
	camera.fov = 34.0
	camera.position = Vector3(0.0, 1.92, 4.69)
	camera.rotation_degrees = Vector3(-8.0, 0.0, 0.0)
	camera.current = true
	add_child(camera)
	_camera = camera

	_jimmy = Jimmy.new()
	_jimmy.name = "Jimmy"
	add_child(_jimmy)

	_build_ground()

	_bridge = HTTPRequest.new()
	_bridge.name = "Bridge"
	_bridge.timeout = 3.0
	add_child(_bridge)

	_apply_quality(_quality)


## Le fond est transparent : il n'y a pas de sol pour recevoir une ombre, et le
## personnage paraît flotter. Deux couches l'ancrent sur le bureau :
##   - une tache sombre et douce sous les pieds (ombre de contact, tous profils) ;
##   - un « capteur d'ombre » : un plan invisible qui ne devient opaque que là
##     où l'ombre de la lumière clé tombe (`shadow_to_opacity`).
func _build_ground() -> void:
	var gradient := Gradient.new()
	gradient.set_color(0, Color(0.0, 0.0, 0.0, 0.42))
	gradient.set_color(1, Color(0.0, 0.0, 0.0, 0.0))
	var blob_tex := GradientTexture2D.new()
	blob_tex.gradient = gradient
	blob_tex.fill = GradientTexture2D.FILL_RADIAL
	blob_tex.fill_from = Vector2(0.5, 0.5)
	blob_tex.fill_to = Vector2(1.0, 0.5)
	blob_tex.width = 128
	blob_tex.height = 128

	var blob_mat := StandardMaterial3D.new()
	blob_mat.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	blob_mat.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
	blob_mat.albedo_texture = blob_tex
	var blob_mesh := PlaneMesh.new()
	blob_mesh.size = Vector2(0.62, 0.42)
	blob_mesh.material = blob_mat
	var blob := MeshInstance3D.new()
	blob.name = "ContactShadow"
	blob.mesh = blob_mesh
	blob.position = Vector3(0.0, -0.025, 0.02)
	blob.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	add_child(blob)

	# Réglage contre-intuitif, lu dans le shader de Godot 4.5
	# (scene_forward_clustered.glsl) : l'alpha est plafonné par
	# `length(ambient_light * albedo)`. Un albedo noir donne donc un plan
	# entièrement transparent. On garde un albedo blanc pour l'alpha, et
	# `metallic = 1` annule l'ambiante dans la couleur finale (appliqué après
	# le calcul de l'alpha) : l'ombre reste sombre au lieu de virer au gris.
	var catcher_mat := StandardMaterial3D.new()
	catcher_mat.albedo_color = Color(1.0, 1.0, 1.0, 0.38)
	catcher_mat.metallic = 1.0
	catcher_mat.metallic_specular = 0.0
	catcher_mat.roughness = 1.0
	catcher_mat.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
	catcher_mat.shadow_to_opacity = true
	# L'ombre s'estompe avec la distance aux pieds : pas de bord net du plan.
	var fade := Gradient.new()
	fade.set_color(0, Color(1.0, 1.0, 1.0, 1.0))
	fade.set_color(1, Color(1.0, 1.0, 1.0, 0.0))
	var fade_tex := GradientTexture2D.new()
	fade_tex.gradient = fade
	fade_tex.fill = GradientTexture2D.FILL_RADIAL
	fade_tex.fill_from = Vector2(0.5, 0.5)
	fade_tex.fill_to = Vector2(1.0, 0.5)
	catcher_mat.albedo_texture = fade_tex
	var catcher_mesh := PlaneMesh.new()
	catcher_mesh.size = Vector2(1.6, 1.6)
	catcher_mesh.material = catcher_mat
	var catcher := MeshInstance3D.new()
	catcher.name = "ShadowCatcher"
	catcher.mesh = catcher_mesh
	catcher.position = Vector3(0.0, -0.03, 0.0)
	catcher.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	# Piège : `shadow_to_opacity` rend le plan transparent partout où une
	# lumière éclaire *sans ombre*. Les deux omni n'ont pas d'ombre : elles
	# effaceraient tout. Le capteur vit donc sur le calque 2, que seule la
	# lumière clé (masque par défaut = tous les calques) atteint.
	catcher.layers = 2
	add_child(catcher)


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
			var seconds := _jimmy.speak_for(duration if duration > 0.0 else _estimate_speech(text))
			_show_bubble(text, seconds)
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
		"/snapshot":
			# Outil de diagnostic : enregistre le rendu (alpha compris) en PNG.
			# Sert aux comparaisons avant/après sans passer par le premier plan.
			_save_snapshot(str(body.get("path", "user://snapshot.png")))
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


func _hide_bubble() -> void:
	_bubble.visible = false
	_bubble_timer = 0.0


func _save_snapshot(path: String) -> void:
	# On attend la fin de l'image en cours pour capturer un rendu complet.
	await RenderingServer.frame_post_draw
	var img := get_viewport().get_texture().get_image()
	var err := img.save_png(path)
	print("[godot/main] snapshot %s → %s" % [path, error_string(err)])


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

	# Seule la lumière clé projette une ombre : les deux omni n'en avaient
	# aucune utile (contre-jour et remplissage), c'était un coût sans gain.
	var key_light := get_node_or_null("KeyLight") as DirectionalLight3D
	if key_light != null:
		key_light.shadow_enabled = preset["shadows"]
	# Sans ombre portée, le capteur ne sert à rien : on le retire du rendu.
	var catcher := get_node_or_null("ShadowCatcher") as MeshInstance3D
	if catcher != null:
		catcher.visible = preset["shadows"]

	var env_node := get_node_or_null("WorldEnvironment")
	if env_node != null:
		env_node.environment.ssao_enabled = preset["ssao"]
		env_node.environment.glow_enabled = preset["glow"]
		if preset["glow"]:
			env_node.environment.glow_intensity = 0.5
			env_node.environment.glow_bloom = 0.15

	if _jimmy != null:
		_jimmy.set_detail(preset["detail"])

	Engine.max_fps = 60 if level != "low" else 30
	print("[godot/main] profil graphique : %s" % level)