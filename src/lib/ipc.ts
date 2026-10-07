import { invoke } from "@tauri-apps/api/core";
import type { ContextUsage } from "./context";
import type { DiscordPresence, DiscordStatus } from "./discord";
import type { VoiceConfig, VoiceModel } from "./voice";
import type { AgentSession, AppState, ShellInfo, WorkspaceRoot } from "../types";

export const ptySpawn = (args: {
  id: string;
  cwd: string;
  shell?: ShellInfo | null;
  cols: number;
  rows: number;
  /** The pane's name, which the hangar-bridge mod reports edits under. */
  name?: string;
  /** False keeps the hangar-bridge mod out of this pane's Claude Code. */
  bridge?: boolean;
}) => invoke<void>("pty_spawn", args);

export const ptyWrite = (id: string, data: string) =>
  invoke<void>("pty_write", { id, data });

export const ptyResize = (id: string, cols: number, rows: number) =>
  invoke<void>("pty_resize", { id, cols, rows });

export const ptyKill = (id: string) => invoke<void>("pty_kill", { id });

export const ptyAlive = (id: string) => invoke<boolean>("pty_alive", { id });

/** A prompt waiting in Hangar until the pane's Claude Code is free to take it. */
export interface QueuedPrompt {
  id: string;
  text: string;
}

/** Queued for the pane's hangar-bridge mod, which submits it once idle. */
export const bridgeEnqueue = (paneId: string, text: string) =>
  invoke<QueuedPrompt>("bridge_enqueue", { paneId, text });

/** One queued prompt by id, or the pane's whole queue without one. */
export const bridgeCancel = (paneId: string, id?: string) =>
  invoke<void>("bridge_cancel", { paneId, id: id ?? null });

export const bridgeQueue = (paneId: string) =>
  invoke<QueuedPrompt[]>("bridge_queue", { paneId });

export const listSessions = (agent: string, cwd: string) =>
  invoke<AgentSession[]>("list_sessions", { agent, cwd });

/** `false` only when the conversation has no transcript left anywhere;
    `null` when that cannot be told. */
export const sessionExists = (agent: string, id: string) =>
  invoke<boolean | null>("session_exists", { agent, id });

export const detectAgents = () => invoke<string[]>("detect_agents");

/**
 * What the agent's own transcript says its context holds. `null` is every
 * "show nothing" answer: no transcript for this agent, none written yet, no
 * usage line so far.
 */
export const contextUsage = (agent: string, sessionId: string, cwd: string) =>
  invoke<ContextUsage | null>("context_usage", { agent, sessionId, cwd });

export const detectShells = () => invoke<ShellInfo[]>("detect_shells");

export const loadState = () => invoke<AppState | null>("load_state");

export const saveState = (state: AppState) =>
  invoke<void>("save_state", { state });

export const dirExists = (path: string) => invoke<boolean>("dir_exists", { path });

/** The folders a VS Code workspace file lists. Rejects if it cannot be read. */
export const readCodeWorkspace = (path: string) =>
  invoke<WorkspaceRoot[]>("read_code_workspace", { path });

/** The `.code-workspace` sitting in `dir`, when there is one to offer. */
export const findCodeWorkspace = (dir: string) =>
  invoke<string | null>("find_code_workspace", { dir });

/** `null` clears the presence and closes the connection to Discord. */
export const setDiscordPresence = (wanted: DiscordPresence | null) =>
  invoke<void>("discord_presence_set", { wanted });

export const discordPresenceStatus = () =>
  invoke<DiscordStatus>("discord_presence_status");

export const homeDir = () => invoke<string>("home_dir");

/**
 * Arms the microphone. Rejects when there is none, when one is already open,
 * or when the chosen engine could not transcribe anyway — a model still to be
 * downloaded, a key still to be pasted.
 */
export const voiceStart = (config: VoiceConfig) => invoke<void>("voice_start", { config });

/** Ends the recording; `cancel` throws the audio away instead of sending it. */
export const voiceStop = (config: VoiceConfig, cancel: boolean) =>
  invoke<void>("voice_stop", { config, cancel });

/** Whether a recording is open — how the interface recovers after a reload. */
export const voiceRecording = () => invoke<boolean>("voice_recording");

/** Frees the resident model. */
export const voiceUnload = () => invoke<void>("voice_unload");

export const voiceModels = () => invoke<VoiceModel[]>("voice_models");

/** Resolves when the model is installed; progress arrives on `voice:download`. */
export const voiceModelDownload = (id: string) =>
  invoke<void>("voice_model_download", { id });

/**
 * The Groq key lives in the system keychain, and the window never reads it
 * back: only whether one is saved. Rejects when the keychain cannot be reached.
 */
export const voiceKeyStatus = () => invoke<boolean>("voice_key_status");

export const voiceKeySet = (key: string) => invoke<void>("voice_key_set", { key });

export const voiceKeyClear = () => invoke<void>("voice_key_clear");

/** The tail of a live pane's output, and the byte count it reaches. */
export const ptyHistory = (id: string) =>
  invoke<{ data: string; end: number }>("pty_history", { id });
