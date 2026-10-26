import type { AgentActivity } from "../lib/agentState";
import type { QueuedPrompt } from "../lib/ipc";
import type { ShellInfo } from "../types";
import type { GlyphState } from "./StateGlyph";

/**
 * The pure half of a pane header: what its glyph, its state word and its
 * gauge say, and how the pane words what it reports to lib/paneInsight. Kept
 * apart from TerminalPane so it can be tested without a terminal.
 */

export type PaneStatus = "starting" | "running" | "exited";

/**
 * The shell's own life until it runs; then, for a pane that reports one, the
 * agent's state. A plain shell has nothing to report and rests as idle.
 */
export function paneGlyphState(status: PaneStatus, activity: AgentActivity | null): GlyphState {
  if (status !== "running") return status;
  return activity ?? "idle";
}

/** Where a context window starts to deserve a look, then a decision. */
export const GAUGE_WARN = 70;
export const GAUGE_HOT = 90;

export function gaugeLevel(pct: number): "ok" | "warn" | "hot" {
  if (pct >= GAUGE_HOT) return "hot";
  if (pct >= GAUGE_WARN) return "warn";
  return "ok";
}

/**
 * A shell as the header names it: the program, without its folder or its
 * `.exe` — `pwsh`, `bash`, `wsl` — which is what a terminal user calls it and
 * fits the mono metadata of a narrow pane.
 */
export function shellShortName(shell: ShellInfo | null): string | null {
  if (!shell) return null;
  const file = shell.program.split(/[\\/]/).pop() ?? "";
  const name = file.replace(/\.(exe|cmd|bat|com)$/i, "").toLowerCase();
  return name || shell.label;
}

/** Session ids are long; six characters tell two apart on screen. */
export function shortSessionId(id: string): string {
  return id.slice(0, 6);
}

/** The first line with something on it, trimmed — an event is one line. */
export function firstLine(text: string, max = 120): string {
  const line = text.split(/\r?\n/).find((entry) => entry.trim())?.trim() ?? "";
  return line.length > max ? `${line.slice(0, max - 1)}…` : line;
}

/** `src/lib/layout.ts` → `layout.ts`, whichever separator the agent used. */
export function fileBaseName(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, "");
  return trimmed.split(/[\\/]/).pop() || trimmed;
}

/** The prompts that joined the queue since it last changed, oldest first. */
export function newlyQueued(before: QueuedPrompt[], after: QueuedPrompt[]): QueuedPrompt[] {
  const known = new Set(before.map((entry) => entry.id));
  return after.filter((entry) => !known.has(entry.id));
}
