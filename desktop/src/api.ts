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
    | "synaptiq"
    | "skill"
    | "final"
    | "notice"
    | "failed";
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
}

export interface Status {
  version: string;
  dev: boolean;
  data_dir: string;
  workspace: string;
  llm: { model: string; base_url: string; has_key: boolean };
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
    language: string;
    wake_word: string;
    models: { id: string; label: string; note: string }[];
  };
  avatar: {
    enabled: boolean;
    skin: string;
    skins: { id: string; label: string }[];
    quality: string;
    host: string;
    port: number;
    running: boolean;
  };
  memory: { enabled: boolean; count: number; has_fts: boolean };
  synaptiq: { enabled: boolean; configured: boolean; base_url: string };
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
}

export interface ChatMessage {
  role: string;
  content: string;
}

export interface ModelInfo {
  id: string;
  model: string;
  name: string;
  context: number;
  free: boolean;
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
}

export interface VoiceSettings {
  enabled: boolean;
  volume: number;
  input_sample_rate: number;
  vad_threshold: number;
  end_of_speech_ms: number;
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
}

export interface MemorySettings {
  enabled: boolean;
  recall_limit: number;
  min_score: number;
  auto_learn_every: number;
  auto_learn: boolean;
}

export interface SynaptiqSettings {
  enabled: boolean;
  base_url: string;
  min_request_chars: number;
  timeout_ms: number;
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
  synaptiq: SynaptiqSettings;
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

export const api = {
  bootstrap: () =>
    invoke<{
      status: Status;
      voices: { id: string; label: string; description: string }[];
      wake_words: string[];
      startup_modes: StartupMode[];
    }>("bootstrap"),
  status: () => invoke<Status>("status"),
  chat: (sessionId: string | null, message: string) =>
    invoke<string>("chat", { request: { sessionId, message } }),
  sessions: () => invoke<Session[]>("sessions"),
  sessionMessages: (sessionId: string) =>
    invoke<ChatMessage[]>("session_messages", { sessionId }),
  deleteSession: (sessionId: string) => invoke<void>("delete_session", { sessionId }),
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<void>("save_settings", { settings }),
  listModels: () => invoke<ModelInfo[]>("list_models"),
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
  ttsPreview: (text: string) =>
    invoke<{ bytes: number; durationMs: number }>("tts_preview", { text }),
  voiceDevices: () => invoke<string[]>("voice_devices"),
  voiceStart: () => invoke<void>("voice_start"),
  voiceStop: () => invoke<void>("voice_stop"),
  sttTranscribe: (wav: number[]) => invoke<string>("stt_transcribe", { wav }),
  avatarStart: () => invoke<void>("avatar_start"),
  avatarStop: () => invoke<void>("avatar_stop"),
  avatarState: (avatarState: AvatarState, detail: string) =>
    invoke<void>("avatar_state", { avatarState, detail }),
  avatarSay: (text: string) => invoke<void>("avatar_say", { text }),
  avatarQuality: (level: string) => invoke<void>("avatar_quality", { level }),
  avatarSkin: (skin: string) => invoke<void>("avatar_skin", { skin }),
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