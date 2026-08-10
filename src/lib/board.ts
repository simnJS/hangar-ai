import { invoke } from "@tauri-apps/api/core";

export const BOARD_COLUMNS = ["todo", "doing", "review", "done"] as const;
export type BoardColumn = (typeof BOARD_COLUMNS)[number];

/** Maps a column to its translation key, so labels stay in one place. */
export const columnKey = (column: BoardColumn) => `board.${column}` as const;

export interface TaskComment {
  id: string;
  author: string;
  text: string;
  created_at: number;
}

/** Field names are snake_case to match the Rust serialization. */
export interface Task {
  id: string;
  title: string;
  description: string;
  column: BoardColumn;
  priority: number;
  assignee: string | null;
  labels: string[];
  comments: TaskComment[];
  depends_on: string[];
  created_at: number;
  updated_at: number;
  order: number;
}

export interface TaskPatch {
  title?: string;
  description?: string;
  column?: BoardColumn;
  priority?: number;
  assignee?: string;
  release?: boolean;
  labels?: string[];
  depends_on?: string[];
  order?: number;
}

export interface NewTask {
  title: string;
  description?: string;
  column?: BoardColumn;
  priority?: number;
  labels?: string[];
  depends_on?: string[];
}

export const boardLoad = (cwd: string) => invoke<{ tasks: Task[] }>("board_load", { cwd });

export const boardCreate = (cwd: string, task: NewTask) =>
  invoke<Task>("board_create", { cwd, task });

export const boardUpdate = (cwd: string, id: string, patch: TaskPatch) =>
  invoke<Task>("board_update", { cwd, id, patch });

export const boardDelete = (cwd: string, id: string) =>
  invoke<void>("board_delete", { cwd, id });

export const boardClaim = (cwd: string, id: string, agent: string) =>
  invoke<Task>("board_claim", { cwd, id, agent });

export const boardComment = (cwd: string, id: string, author: string, text: string) =>
  invoke<Task>("board_comment", { cwd, id, author, text });

export interface McpTarget {
  id: string;
  label: string;
  path: string;
  detected: boolean;
  configured: boolean;
  /** Registered, but under an old key or pointing at a missing executable. */
  stale: boolean;
  /** The executable the config names today, shown when it is the broken part. */
  command: string | null;
  project_scoped: boolean;
}

export interface McpReport {
  id: string;
  ok: boolean;
  message: string;
  path: string;
}

export const mcpTargets = (workspaceCwd: string) =>
  invoke<McpTarget[]>("mcp_targets", { workspaceCwd });

export const mcpInstall = (ids: string[], workspaceCwd: string) =>
  invoke<McpReport[]>("mcp_install", { ids, workspaceCwd });

export interface InstructionReport {
  path: string;
  ok: boolean;
  message: string;
}

export const writeAgentInstructions = (workspaceCwd: string) =>
  invoke<InstructionReport[]>("write_agent_instructions", { workspaceCwd });

export const agentInstructionsStatus = (workspaceCwd: string) =>
  invoke<boolean>("agent_instructions_status", { workspaceCwd });

export const mcpManualCommands = (workspaceCwd: string) =>
  invoke<{ executable: string; claude: string; codex: string; workspace: string }>(
    "mcp_manual_commands",
    { workspaceCwd },
  );
