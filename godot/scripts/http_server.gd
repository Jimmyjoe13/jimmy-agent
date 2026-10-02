extends Node
class_name JimmyHttpServer
## Serveur HTTP/1.1 minimal, sans dépendance.
##
## Godot n'embarque pas de serveur HTTP : on l'implémente ici sur `TCPServer`.
## Le périmètre est volontairement restreint à ce dont Tauri a besoin :
## quelques routes JSON en POST/GET, corps de requête court, réponse 200.
##
## Routes :
##   GET  /health          -> {"ok":true,"skin":"renard","state":"idle"}
##   POST /state           -> {"state":"idle|listening|thinking|speaking|..."}
##   POST /say             -> {"text":"...","duration_ms":1200}
##   POST /quality         -> {"level":"low|medium|high"}
##   POST /skin            -> {"skin":"renard"}
##   POST /position        -> {"x":120,"y":240}
##   GET  /stats           -> fps + état courant

const MAX_BODY := 65536
const READ_SLICE := 4096

signal request_received(method: String, path: String, body: Dictionary)

var _server := TCPServer.new()
var _peers: Array[StreamPeerTCP] = []
var _buffers: Array[String] = []
var _port: int = 0
var _running := false


## Démarre le serveur. Retourne OK ou un code d'erreur.
func start(port: int, host: String = "127.0.0.1") -> int:
	var err := _server.listen(port, host)
	if err != OK:
		push_error("[godot/http] impossible d'écouter sur %s:%d (%d)" % [host, port, err])
		return err
	_port = port
	_running = true
	set_process(true)
	print("[godot/http] serveur HTTP local sur http://%s:%d" % [host, port])
	return OK


func stop() -> void:
	_running = false
	for peer in _peers:
		peer.disconnect_from_host()
	_peers.clear()
	_buffers.clear()
	_server.stop()


func is_running() -> bool:
	return _running


func _process(_delta: float) -> void:
	if not _running:
		return
	while _server.is_connection_available():
		var peer := _server.take_connection()
		if peer != null:
			_peers.append(peer)
			_buffers.append("")

	for i in range(_peers.size() - 1, -1, -1):
		var peer := _peers[i]
		peer.poll()
		var status := peer.get_status()
		if status == StreamPeerTCP.STATUS_ERROR or status == StreamPeerTCP.STATUS_NONE:
			_peers.remove_at(i)
			_buffers.remove_at(i)
			continue
		if status != StreamPeerTCP.STATUS_CONNECTED:
			continue

		if peer.get_available_bytes() > 0:
			var res := peer.get_data(min(peer.get_available_bytes(), READ_SLICE))
			if res[0] == OK:
				_buffers[i] += res[1].get_string_from_utf8()
			if _buffers[i].length() > MAX_BODY:
				_respond(peer, 413, {"error": "corps de requête trop grand"})
				_peers.remove_at(i)
				_buffers.remove_at(i)
				continue

		var raw := _buffers[i]
		var header_end := raw.find("\r\n\r\n")
		if header_end == -1:
			continue # requête incomplète : on attend la suite

		var head := raw.substr(0, header_end)
		var rest := raw.substr(header_end + 4)
		var lines := head.split("\r\n")
		var request_line := lines[0].split(" ")
		if request_line.size() < 2:
			_respond(peer, 400, {"error": "requête mal formée"})
			_peers.remove_at(i)
			_buffers.remove_at(i)
			continue

		var method := request_line[0]
		var path := request_line[1]
		var content_length := 0
		for j in range(1, lines.size()):
			var colon := lines[j].find(":")
			if colon == -1:
				continue
			var name := lines[j].substr(0, colon).strip_edges().to_lower()
			var value := lines[j].substr(colon + 1).strip_edges()
			if name == "content-length":
				content_length = int(value)

		if rest.length() < content_length:
			continue # corps incomplet

		var body_text := rest.substr(0, content_length)
		var body: Dictionary = {}
		if body_text.length() > 0:
			var parsed = JSON.parse_string(body_text)
			if typeof(parsed) == TYPE_DICTIONARY:
				body = parsed

		request_received.emit(method, path, body)
		_respond(peer, 200, {"ok": true})
		_peers.remove_at(i)
		_buffers.remove_at(i)


func _respond(peer: StreamPeerTCP, code: int, payload: Dictionary) -> void:
	var json := JSON.stringify(payload)
	var reason := "OK" if code == 200 else "Error"
	var head := "HTTP/1.1 %d %s\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: %d\r\nConnection: close\r\n\r\n" % [code, reason, json.to_utf8_buffer().size()]
	peer.put_data(head.to_utf8_buffer())
	peer.put_data(json.to_utf8_buffer())