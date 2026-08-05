import { invoke } from "@tauri-apps/api/core";
import type { DiscordPresence, DiscordStatus } from "./discord";
import type { AgentSession, AppState, ShellInfo } from "../types";

export const ptySpawn = (args: {
  id: string;
  cwd: string;
  shell?: ShellInfo | null;
  cols: number;
  rows: number;
}) => invoke<void>("pty_spawn", args);

export const ptyWrite = (id: string, data: string) =>
  invoke<void>("pty_write", { id, data });

export const ptyResize = (id: string, cols: number, rows: number) =>
  invoke<void>("pty_resize", { id, cols, rows });

export const ptyKill = (id: string) => invoke<void>("pty_kill", { id });

export const ptyAlive = (id: string) => invoke<boolean>("pty_alive", { id });

export const listSessions = (agent: string, cwd: string) =>
  invoke<AgentSession[]>("list_sessions", { agent, cwd });

export const detectAgents = () => invoke<string[]>("detect_agents");

export const detectShells = () => invoke<ShellInfo[]>("detect_shells");

export const loadState = () => invoke<AppState | null>("load_state");

export const saveState = (state: AppState) =>
  invoke<void>("save_state", { state });

export const dirExists = (path: string) => invoke<boolean>("dir_exists", { path });

/** `null` clears the presence and closes the connection to Discord. */
export const setDiscordPresence = (wanted: DiscordPresence | null) =>
  invoke<void>("discord_presence_set", { wanted });

export const discordPresenceStatus = () =>
  invoke<DiscordStatus>("discord_presence_status");

export const homeDir = () => invoke<string>("home_dir");
