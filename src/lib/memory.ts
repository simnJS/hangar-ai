import { invoke } from "@tauri-apps/api/core";

/**
 * The global memory: one store for the whole app, shared by every workspace.
 *
 * Agents write to it over MCP (`memory_*` tools) and the window reads the very
 * same file through these commands, so both sides always see one truth.
 */

/**
 * Emitted by the backend after every mutation, wherever it came from — an
 * agent over HTTP or the user in this window. The payload is empty: memory is
 * global, so there is no workspace to filter on.
 */
export const MEMORY_CHANGED_EVENT = "memory:changed";

/** Field names are snake_case to match the Rust serialization. */
export interface MemoryEntry {
  id: string;
  title: string;
  content: string;
  tags: string[];
  /** Workspace the entry was written from — kept to tell facts apart, not to
      partition the store. */
  workspace: string;
  /** Agent that wrote it, `user` for what is typed in the app. */
  agent: string;
  created_at: number;
  updated_at: number;
}

export interface Memory {
  entries: MemoryEntry[];
}

/** Creation is an upsert on the title, so re-learning a fact updates it. */
export interface NewMemory {
  title: string;
  content: string;
  tags?: string[];
  workspace?: string;
  agent?: string;
}

export interface MemoryPatch {
  title?: string;
  content?: string;
  tags?: string[];
}

export const memoryLoad = () => invoke<Memory>("memory_load");

export const memoryCreate = (input: NewMemory) => invoke<MemoryEntry>("memory_create", { input });

export const memoryUpdate = (id: string, patch: MemoryPatch) =>
  invoke<MemoryEntry>("memory_update", { id, patch });

export const memoryDelete = (id: string) => invoke<void>("memory_delete", { id });
