/**
 * Accès aux commandes Tauri.
 *
 * Toute la surface Rust passe par ici. Aucun `invoke` n'est appelé ailleurs :
 * si une commande change de nom ou de signature, il n'y a qu'un fichier à
 * corriger.
 */
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type AvatarState =
  | "idle"
  | "listening"
  | "thinking"
  | "speaking"
  | "executing"
  | "success"
  | "error"
  | "waiting";

export interface AgentEvent {
  type:
    | "state"
    | "toolStart"
    | "toolEnd"
    | "memory"
    | "vault"
    | "skill"
    | "delta"
    | "approval"
    | "approvalResolved"
    | "final"
    | "notice"
    | "heard"
    | "spoken"
    | "progress"
    | "listen"
    | "failed"
    | "detached"
    | "background";
  state?: AvatarState;
  detail?: string;
  callId?: string;
  name?: string;
  arguments?: unknown;
  ok?: boolean;
  summary?: string;
  durationMs?: number;
  action?: string;
  text?: string;
  message?: string;
  /** `heard` : le mot d'activation a été reconnu dans la transcription. */
  matched?: boolean;
  /** `listen` : étape de l'écoute (idle, capturing, transcribing, your_turn,
   *  thinking, speaking) et temps restant en ms pour `your_turn`. */
  phase?: string;
  remaining?: number;
  /** `spoken` : session de la conversation vocale (adoptée par le Chat). */
  sessionId?: string;
  /** `approval` : demande d'autorisation (fichier sensible) ; `approvalResolved` : sa clôture. */
  id?: string;
  target?: string;
  approved?: boolean;
  /** `detached` / `background` : la tâche de fond concernée (`sessionId` =
   *  sa conversation). `title` : début de sa demande. */
  taskId?: string;
  title?: string;
  /** `background` : l'événement de la tâche de fond, enveloppé. */
  event?: AgentEvent;
}

/** Capture de la fenêtre active en attente d'envoi (`screen_capture`). */
export interface ScreenCapture {
  id: string;
  label: string;
  app: string;
  width: number;
  height: number;
  /** Image en URL `data:` pour la vignette. */
  preview: string;
}

/** Tâche de l'agent en cours (`tasks_list`). */
export interface TaskInfo {
  id: string;
  sessionId: string;
  title: string;
  step: string;
  background: boolean;
  elapsedSecs: number;
}

/** État réel de l'écoute, lu côté Rust (`voice_status`). */
export interface VoiceStatus {
  running: boolean;
  deviceRate: number | null;
  wakeWord: string;
  wakeModel: string;
  commandModel: string;
  wakeReady: boolean;
  commandReady: boolean;
  listenOnStart: boolean;
  /** Niveau RMS du micro (300 dernières ms). */
  level: number;
}

export interface Status {
  version: string;
  dev: boolean;
  data_dir: string;
  workspace: string;
  llm: { model: string; voice_model?: string; base_url: string; has_key: boolean };
  tts: {
    enabled: boolean;
    model: string;
    voice: string;
    has_key: boolean;
    voices: { id: string; label: string; description: string }[];
  };
  stt: {
    enabled: boolean;
    model: string;
    command_model: string;
    language: string;
    wake_word: string;
    models: { id: string; label: string; note: string }[];
  };
  avatar: {
    enabled: boolean;
    skin: string;
    skins: { id: string; label: string }[];
    quality: string;
    dodge: boolean;
    host: string;
    port: number;
    running: boolean;
  };
  memory: { enabled: boolean; count: number; has_fts: boolean; semantic_model: string | null };
  vault: { enabled: boolean; path: string; folder: string; notes: number };
  permissions: { capability: string; granted: boolean }[];
  tools: string[];
  skills: number;
  first_run_done: boolean;
}

export interface Session {
  id: string;
  title: string;
  createdAt: string;
  updatedAt: string;
  messageCount: number;
  /** Dossier du projet de la conversation (onglet Chat). */
  project?: string | null;
}

/** Un projet proposé dans le sélecteur du Chat. */
export interface ProjectInfo {
  path: string;
  name: string;
  exists: boolean;
  /** Lisible avec les permissions de Jimmy. */
  readable: boolean;
}

export interface FsEntry {
  name: string;
  path: string;
  dir: boolean;
  size: number;
}

/** Un candidat du menu « @ » du Chat : chemin relatif au projet. */
export interface MentionEntry {
  display: string;
  name: string;
  dir: boolean;
}

export interface MentionSearch {
  entries: MentionEntry[];
  truncated: boolean;
  rootInvalid: boolean;
}

export interface FsPreview {
  path: string;
  size: number;
  binary: boolean;
  text: string;
  truncated: boolean;
  executable: boolean;
}

export interface ChatMessage {
  role: string;
  content: string;
}

export interface ModelInfo {
  /** Identifiant complet `fournisseur/modèle` (compatibilité onboarding). */
  id: string;
  /** Identifiant à stocker et à envoyer au fournisseur. */
  model: string;
  full_id: string;
  name: string;
  description: string;
  family: string;
  /** Fenêtre de contexte en jetons (0 = inconnue). */
  context: number;
  free: boolean;
  reasoning: boolean;
  /** `null` : absent du catalogue public, capacité inconnue. */
  tool_call: boolean | null;
  vision: boolean;
  /** Dollars par million de jetons. */
  cost_input: number;
  cost_output: number;
  released: string;
  in_catalog: boolean;
}

/** Résultat du test fonctionnel d'un modèle (requête simple, puis avec outils). */
export interface ModelTest {
  model: string;
  ok: boolean;
  tools: boolean;
  latency_ms: number;
  tools_latency_ms: number;
  reply: string;
  error: string;
  tested_at: string;
}

export interface DoctorCheck {
  name: string;
  ok: boolean;
  hint: string;
}

export interface DoctorReport {
  ok: boolean;
  passed: number;
  total: number;
  checks: DoctorCheck[];
}

export interface LlmSettings {
  model: string;
  /** Modèle des échanges vocaux (vide = le modèle principal). */
  voice_model: string;
  base_url: string;
  temperature: number | null;
  max_tokens: number;
  max_iterations: number;
  session_prefix: string;
}

export interface TtsSettings {
  enabled: boolean;
  provider: string;
  model: string;
  voice: string;
  chars_per_minute: number;
  cues: boolean;
  library?: VoiceInfo[];
}

/** Une voix Fish Audio (bibliothèque de voix). */
export interface VoiceInfo {
  id: string;
  label: string;
  description: string;
  languages: string[];
  uses: number;
}

export interface SttSettings {
  enabled: boolean;
  language: string;
  model: string;
  server_exe: string;
  port: number;
  wake_window_ms: number;
  wake_word: string;
  threads: number;
  command_model: string;
  command_port: number;
  command_audio_ctx: number;
}

export interface VoiceSettings {
  enabled: boolean;
  volume: number;
  input_sample_rate: number;
  vad_threshold: number;
  end_of_speech_ms: number;
  follow_up_ms: number;
  debug_audio: boolean;
  listen_on_start: boolean;
}

export interface AvatarSettings {
  enabled: boolean;
  skin: string;
  quality: string;
  host: string;
  port: number;
  bridge_host: string;
  bridge_port: number;
  godot_exe: string;
  autostart: boolean;
  dodge: boolean;
}

export interface MemorySettings {
  enabled: boolean;
  recall_limit: number;
  min_score: number;
  auto_learn_every: number;
  auto_learn: boolean;
  vault_path: string;
  vault_enabled: boolean;
  vault_folder: string;
  vault_min_request_chars: number;
}

export interface UiSettings {
  theme: string;
  first_run_done: boolean;
  user_name: string;
  window: { x: number | null; y: number | null; width: number; height: number };
}

export type StartupMode = "manual" | "with_windows" | "with_windows_hidden";

export interface McpServerConfig {
  name: string;
  transport: string;
  command: string[];
  url: string;
  env: Record<string, string>;
  enabled: boolean;
}

export interface Settings {
  llm: LlmSettings;
  tts: TtsSettings;
  stt: SttSettings;
  voice: VoiceSettings;
  avatar: AvatarSettings;
  memory: MemorySettings;
  ui: UiSettings;
  startup: StartupMode;
  mcp_servers: McpServerConfig[];
  workspace: string;
}

export interface AccessRule {
  granted: boolean;
  allow_paths: string[];
  allow_commands: string[];
  deny_commands: string[];
}

export interface Permissions {
  read: AccessRule;
  write: AccessRule;
  execute: AccessRule;
  network: AccessRule;
}

export interface Memory {
  id: string;
  kind: string;
  content: string;
  importance: number;
  createdAt: string;
  useCount: number;
}

export interface Skill {
  name: string;
  description: string;
  body: string;
}

/** Serveur MCP vu par l'interface (commande `mcp_servers`, secrets masqués). */
export interface McpServerStatus {
  name: string;
  transport: string;
  launch: string;
  envKeys: string[];
  state: "connected" | "stopped" | "busy" | "disabled";
  tools: { name: string; description: string }[];
}

export const api = {
  bootstrap: () =>
    invoke<{
      status: Status;
      voices: { id: string; label: string; description: string }[];
      wake_words: string[];
      startup_modes: StartupMode[];
    }>("bootstrap"),
  status: () => invoke<Status>("status"),
  /** `project` : seulement pour le premier message d'une conversation neuve. */
  chat: (sessionId: string | null, message: string, project?: string | null, captureId?: string | null) =>
    invoke<string>("chat", { request: { sessionId, message, project: project ?? null, captureId: captureId ?? null } }),
  screenCapture: () => invoke<ScreenCapture>("screen_capture"),
  screenDiscard: (id: string) => invoke<boolean>("screen_discard", { id }),
  pickFolder: () => invoke<string | null>("pick_folder"),
  projectsRecent: () => invoke<{ recent: ProjectInfo[]; default: ProjectInfo }>("projects_recent"),
  sessionSetProject: (sessionId: string, project: string | null) =>
    invoke<void>("session_set_project", { sessionId, project }),
  fsList: (path: string) => invoke<{ path: string; entries: FsEntry[]; truncated: boolean }>("fs_list", { path }),
  fsSearch: (root: string, query: string) => invoke<MentionSearch>("fs_search", { root, query }),
  fsPreview: (path: string) => invoke<FsPreview>("fs_preview", { path }),
  fsDiff: (file: string) => invoke<{ diff: string; reason: string }>("fs_diff", { file }),
  fsOpen: (path: string) => invoke<"opened" | "revealed">("fs_open", { path }),
  fsReveal: (path: string) => invoke<void>("fs_reveal", { path }),
  sessions: () => invoke<Session[]>("sessions"),
  sessionMessages: (sessionId: string) =>
    invoke<ChatMessage[]>("session_messages", { sessionId }),
  deleteSession: (sessionId: string) => invoke<void>("delete_session", { sessionId }),
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<void>("save_settings", { settings }),
  listModels: (refresh = false) => invoke<ModelInfo[]>("list_models", { refresh }),
  llmTestModel: (model: string) => invoke<ModelTest>("llm_test_model", { model }),
  setLlmModel: (role: "main" | "voice", model: string) => invoke<void>("set_llm_model", { role, model }),
  completeOnboarding: (payload: {
    userName: string;
    model: string;
    voice: string;
    quality: string;
    language: string;
  }) => invoke<Status>("complete_onboarding", payload),
  setStartup: (mode: StartupMode) => invoke<void>("set_startup", { mode }),
  getPermissions: () => invoke<Permissions>("get_permissions"),
  savePermissions: (permissions: Permissions) =>
    invoke<void>("save_permissions", { permissions }),
  memoryList: () => invoke<Memory[]>("memory_list"),
  memoryForget: (id: string) => invoke<void>("memory_forget", { id }),
  skillsList: () => invoke<Skill[]>("skills_list"),
  skillRead: (name: string) => invoke<string>("skill_read", { name }),
  mcpServers: () => invoke<McpServerStatus[]>("mcp_servers"),
  /** Arrêt d'urgence de la tâche en cours (et de la voix de Jimmy). */
  agentStop: () => invoke<boolean>("agent_stop"),
  tasksList: () => invoke<TaskInfo[]>("tasks_list"),
  taskStop: (id: string) => invoke<boolean>("task_stop", { id }),
  ttsVoices: () => invoke<{ current: string; presets: VoiceInfo[]; library: VoiceInfo[] }>("tts_voices"),
  ttsSearchVoices: (query: string) => invoke<VoiceInfo[]>("tts_search_voices", { query }),
  ttsSetVoice: (voice: VoiceInfo) => invoke<void>("tts_set_voice", { voice }),
  ttsRemoveVoice: (id: string) => invoke<string>("tts_remove_voice", { id }),
  /** `voice` : écouter une autre voix que celle de Jimmy (bibliothèque). */
  ttsPreview: (text: string, voice?: string) =>
    invoke<{ bytes: number; durationMs: number }>("tts_preview", { text, voice: voice ?? null }),
  voiceDevices: () => invoke<string[]>("voice_devices"),
  voiceStart: () => invoke<void>("voice_start"),
  voiceStop: () => invoke<void>("voice_stop"),
  /** Accord ou refus d'une modification de fichier sensible ; `false` = demande expirée. */
  approvalRespond: (id: string, approved: boolean) => invoke<boolean>("approval_respond", { id, approved }),
  voiceStatus: () => invoke<VoiceStatus>("voice_status"),
  sttTranscribe: (wav: number[]) => invoke<string>("stt_transcribe", { wav }),
  avatarStart: () => invoke<void>("avatar_start"),
  avatarStop: () => invoke<void>("avatar_stop"),
  avatarState: (avatarState: AvatarState, detail: string) =>
    invoke<void>("avatar_state", { avatarState, detail }),
  avatarSay: (text: string) => invoke<void>("avatar_say", { text }),
  avatarQuality: (level: string) => invoke<void>("avatar_quality", { level }),
  avatarSkin: (skin: string) => invoke<void>("avatar_skin", { skin }),
  avatarDodge: (enabled: boolean) => invoke<void>("avatar_dodge", { enabled }),
  wakeWordTest: (transcript: string, wakeWord: string) =>
    invoke<boolean>("wake_word_test", { transcript, wakeWord }),
  wakeWordStrip: (transcript: string, wakeWord: string) =>
    invoke<{ command: string; matched: boolean }>("wake_word_strip", { transcript, wakeWord }),
  doctor: () => invoke<DoctorReport>("doctor"),
  pathsInfo: () =>
    invoke<{ data: string; app: string; dev: boolean; skills: string; components: string; config: string }>(
      "paths_info",
    ),
  reloadSecrets: () => invoke<{ missing: string[] }>("reload_secrets"),
  showMain: () => invoke<void>("show_main"),
};

export function onAgentEvent(handler: (event: AgentEvent) => void) {
  return listen<AgentEvent>("agent-event", (event) => handler(event.payload));
}