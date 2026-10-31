import type { KitSetupStatus } from "./kits";

/**
 * Setup commands of a kit (docs/kits.md): shown verbatim, run in a visible pane
 * only when the person clicks Run, once per machine.
 *
 * WHEN A STEP COUNTS AS DONE. Hangar types the command into an interactive
 * shell pane (the way a saved command is run), so it never sees the exit code:
 * the shell outlives the command. A step is therefore marked done
 * (`kit_setup_mark`) only after the person confirms it worked, in the pane they
 * are watching — never on its own, and never merely because Run was clicked.
 * The record is keyed by the command's hash, so an update that changes a
 * command makes `kit_setup_status` report the step as not done again.
 */

/** The steps still to run on this machine: never run, or run with another command. */
export const pendingSteps = (steps: readonly KitSetupStatus[]): KitSetupStatus[] =>
  steps.filter((step) => !step.done);

export interface SetupSummary {
  done: number;
  total: number;
  /** Every step is done (a kit with none counts as complete). */
  complete: boolean;
}

export function setupSummary(steps: readonly KitSetupStatus[]): SetupSummary {
  const done = steps.filter((step) => step.done).length;
  return { done, total: steps.length, complete: done === steps.length };
}

/**
 * The bytes typed into the pane: the command exactly as the manifest writes it
 * and the Enter that runs it. Nothing is added, quoted or rewritten, so what the
 * person read is what the shell gets.
 */
export const setupKeystrokes = (command: string): string => `${command}\r`;

/** The operating system the commands are chosen for. */
export type KitOs = "windows" | "macos" | "linux";

/** Every unix that is not macOS counts as linux, as the Rust side does. */
export function currentOs(
  platform: string = typeof navigator === "undefined" ? "" : navigator.platform,
): KitOs {
  const p = platform.toLowerCase();
  if (p.includes("win")) return "windows";
  if (p.includes("mac")) return "macos";
  return "linux";
}

export interface SetupPaneDeps {
  /** Opens a shell pane in the kit's folder; null when the workspace is full. */
  openPane: () => string | null;
  /** Whether the pane's terminal has started. */
  alive: (paneId: string) => Promise<boolean>;
  /** Types into the pane. */
  write: (paneId: string, data: string) => Promise<void>;
  wait: (ms: number) => Promise<void>;
  /** Closes the pane just opened, so a failed start leaves none behind. */
  closePane?: (paneId: string) => void;
  /** How many times to look for the pane's terminal, and the pause between looks. */
  tries?: number;
  delayMs?: number;
}

export type SetupRunResult =
  | { ok: true; paneId: string }
  | { ok: false; reason: "no-pane" | "not-started" | "write-failed"; detail?: string };

/**
 * Opens the pane, waits for its terminal, then types the command. Writes
 * nothing if the terminal never starts (a command typed into nowhere would
 * look as if it ran) and closes the pane it opened.
 */
export async function runSetupCommand(
  command: string,
  deps: SetupPaneDeps,
): Promise<SetupRunResult> {
  const paneId = deps.openPane();
  if (!paneId) return { ok: false, reason: "no-pane" };
  // A workspace never opened this session has to mount before its pane can start.
  const tries = deps.tries ?? 80;
  const delay = deps.delayMs ?? 250;
  for (let i = 0; i < tries; i++) {
    if (await deps.alive(paneId).catch(() => false)) {
      try {
        await deps.write(paneId, setupKeystrokes(command));
        return { ok: true, paneId };
      } catch (err) {
        deps.closePane?.(paneId);
        return {
          ok: false,
          reason: "write-failed",
          detail: err instanceof Error ? err.message : String(err),
        };
      }
    }
    await deps.wait(delay);
  }
  deps.closePane?.(paneId);
  return { ok: false, reason: "not-started" };
}

export interface ShowWorkspaceDeps {
  workspaceId: string;
  /** The workspace the app shows now. */
  activeId: string | null;
  /** It was put to sleep: its panes are unmounted, its agents stopped. */
  asleep: boolean;
  /** The store's action that selects a workspace; App mounts (wakes) what it selects. */
  setActive: (id: string | null) => void;
  wait: (ms: number) => Promise<void>;
}

/**
 * Makes the workspace the one on screen before a pane is added to it: App only
 * mounts the workspaces it has opened, and an unmounted pane never starts its
 * terminal. Selecting it opens it — the way the sidebar wakes a sleeping one.
 * A sleeping workspace that is already the selected one is selected again, from
 * nothing, because selecting the same id changes nothing.
 */
export async function showWorkspace(deps: ShowWorkspaceDeps): Promise<void> {
  if (deps.asleep && deps.activeId === deps.workspaceId) {
    deps.setActive(null);
    await deps.wait(50);
  }
  deps.setActive(deps.workspaceId);
}
