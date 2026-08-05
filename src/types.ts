export type AgentId = "shell" | "claude" | "codex" | "gemini" | "opencode";

export type LayoutSize = 1 | 2 | 4 | 8;

/** A shell discovered on this machine, as reported by the Rust side. */
export interface ShellInfo {
  id: string;
  label: string;
  program: string;
  args: string[];
}

export interface Pane {
  id: string;
  agent: AgentId;
  /** Agent session to resume on next launch. Captured automatically. */
  sessionId: string | null;
  /** Optional per-pane directory, overriding the workspace root. */
  cwd: string | null;
  /** Overrides the workspace shell. null follows the workspace. */
  shellId: string | null;
  title: string | null;
}

export interface Workspace {
  id: string;
  name: string;
  cwd: string;
  layout: LayoutSize;
  panes: Pane[];
  /** Overrides the global theme for this workspace only. */
  themeId: string | null;
  /** Overrides the global default shell. null follows the settings. */
  shellId: string | null;
  /**
   * Nested split tree describing the pane layout. Unlike a CSS grid, each
   * split owns its own ratio, so resizing one boundary leaves the rest alone.
   */
  tree: SplitNode | null;
}

export type SplitNode =
  | { type: "pane"; index: number }
  /** `row` places children side by side (vertical bar), `col` stacks them. */
  | { type: "split"; dir: "row" | "col"; ratio: number; a: SplitNode; b: SplitNode };

export interface Settings {
  themeId: string;
  /** null follows the OS language. */
  locale: "en" | "fr" | null;
  /** Default shell for every new pane. null means "first one detected". */
  shellId: string | null;
  fontFamily: string;
  fontSize: number;
  lineHeight: number;
  letterSpacing: number;
  cursorStyle: "block" | "bar" | "underline";
  cursorBlink: boolean;
  scrollback: number;
  padding: number;
  /** Relaunch agents with their previous session when a workspace opens. */
  autoResume: boolean;
  /** Wait before auto-launching agents, so the shell finishes its profile. */
  launchDelayMs: number;
}

export interface AppState {
  workspaces: Workspace[];
  activeWorkspaceId: string | null;
  settings: Settings;
}

export interface AgentSession {
  id: string;
  label: string;
  modified_ms: number;
}

export const AGENTS: { id: AgentId; label: string; resumable: boolean }[] = [
  { id: "shell", label: "Shell", resumable: false },
  { id: "claude", label: "Claude Code", resumable: true },
  { id: "codex", label: "Codex", resumable: true },
  { id: "gemini", label: "Gemini", resumable: false },
  { id: "opencode", label: "OpenCode", resumable: false },
];

export const DEFAULT_SETTINGS: Settings = {
  themeId: "tokyo-night",
  locale: null,
  shellId: null,
  fontFamily: '"Cascadia Code", "JetBrains Mono", Consolas, "Courier New", monospace',
  fontSize: 13,
  lineHeight: 1.3,
  letterSpacing: 0,
  cursorStyle: "bar",
  cursorBlink: true,
  scrollback: 10000,
  padding: 10,
  autoResume: true,
  launchDelayMs: 700,
};
