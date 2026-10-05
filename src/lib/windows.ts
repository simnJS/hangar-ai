import { invoke } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { Terminal } from "@xterm/xterm";
import type { WindowBounds } from "../types";

/**
 * Hangar in several windows: the main one, and one per workspace sent to a
 * window of its own (see src-tauri/src/windows.rs).
 *
 * The main window owns the store — it loads and saves state.json and carries
 * out what agents ask for. A workspace window runs the same app, keeps a copy
 * of the state in sync with the main one (store.tsx), and draws one workspace.
 *
 * A pane does not belong to a window: when a workspace moves, the window it
 * leaves lets go of its terminals without killing them (`markHandover`), and
 * the window it lands in takes the running PTYs over (TerminalPane).
 */

const label = (() => {
  try {
    return getCurrentWindow().label;
  } catch {
    return "main";
  }
})();

export const WINDOW_LABEL = label;

/** The window that owns the store. The browser demo has a single window, under
    its own label. */
export const isMainWindow = label === "main" || label === "demo";

/** The workspace this window shows, in a workspace window; null otherwise. */
export const windowWorkspaceId: string | null = isMainWindow
  ? null
  : new URLSearchParams(window.location.search).get("workspace");

/** Moving a workspace out needs a second window, which the demo cannot open. */
export const canDetach = label === "main";

export const openWorkspaceWindow = (
  workspaceId: string,
  title: string,
  bounds: WindowBounds | null | undefined,
) => invoke<void>("open_workspace_window", { workspaceId, title, bounds: bounds ?? null });

export const focusWorkspaceWindow = (workspaceId: string) =>
  invoke<boolean>("focus_workspace_window", { workspaceId });

export const closeWorkspaceWindow = (workspaceId: string) =>
  invoke<void>("close_workspace_window", { workspaceId });

/**
 * Panes this window is letting go of. Their terminals unmount here, but their
 * PTYs keep running for the window the workspace is moving to, so the unmount
 * must not kill them. The mark wears off on its own: a pane closed for real a
 * minute later has to die as usual.
 */
const handingOver = new Map<string, number>();
const HANDOVER_MS = 15_000;

export function markHandover(paneIds: string[]) {
  const until = Date.now() + HANDOVER_MS;
  for (const id of paneIds) handingOver.set(id, until);
}

export function isHandingOver(paneId: string): boolean {
  const until = handingOver.get(paneId);
  if (until === undefined) return false;
  if (until < Date.now()) {
    handingOver.delete(paneId);
    return false;
  }
  return true;
}

/**
 * A pane's terminal, wherever it is drawn. The main window carries out the
 * agents' requests, but a detached workspace's terminals live in another
 * window: these are the few things a request needs from one.
 */
export interface PaneTerminal {
  paste: (text: string) => void | Promise<void>;
  read: (lines: number) => string[] | Promise<string[]>;
  /** The application turned cursor-key mode on: arrows are sent as ESC O. */
  applicationCursor: boolean;
}

/** The last `count` lines a terminal shows, wrapped rows joined back into the
    line they came from, blank rows at the bottom left out. */
export function readLines(buffer: Terminal["buffer"]["active"], count: number): string[] {
  const lines: string[] = [];
  for (let row = 0; row < buffer.length; row++) {
    const line = buffer.getLine(row);
    if (!line) continue;
    const content = line.translateToString(true);
    if (line.isWrapped && lines.length > 0) lines[lines.length - 1] += content;
    else lines.push(content);
  }
  while (lines.length > 0 && lines[lines.length - 1].trim() === "") lines.pop();
  return lines.slice(-count);
}

export function localTerminal(term: Pick<Terminal, "paste" | "buffer" | "modes">): PaneTerminal {
  return {
    paste: (text) => term.paste(text),
    read: (lines) => readLines(term.buffer.active, lines),
    applicationCursor: Boolean(term.modes?.applicationCursorKeysMode),
  };
}

type TerminalAsk =
  | { kind: "probe" }
  | { kind: "paste"; text: string }
  | { kind: "read"; lines: number };

interface Ask {
  reqId: string;
  paneId: string;
  op: TerminalAsk;
}

interface Answer {
  reqId: string;
  value: unknown;
}

const ASK_TIMEOUT_MS = 1500;
let asked = 0;

function ask(paneId: string, op: TerminalAsk): Promise<unknown> {
  const reqId = `${label}:${++asked}`;
  return new Promise((resolve) => {
    let off: (() => void) | null = null;
    const timer = window.setTimeout(() => {
      off?.();
      resolve(undefined);
    }, ASK_TIMEOUT_MS);
    listen<Answer>("terminal:answer", (event) => {
      if (event.payload.reqId !== reqId) return;
      window.clearTimeout(timer);
      off?.();
      resolve(event.payload.value);
    })
      .then((stop) => {
        off = stop;
        return emit("terminal:ask", { reqId, paneId, op } satisfies Ask);
      })
      .catch(() => resolve(undefined));
  });
}

/** A terminal another window draws, or null when no window answers for it. */
export async function remoteTerminal(paneId: string): Promise<PaneTerminal | null> {
  const probe = (await ask(paneId, { kind: "probe" })) as { applicationCursor: boolean } | undefined;
  if (!probe) return null;
  return {
    paste: async (text) => {
      await ask(paneId, { kind: "paste", text });
    },
    read: async (lines) => ((await ask(paneId, { kind: "read", lines })) as string[] | undefined) ?? [],
    applicationCursor: probe.applicationCursor,
  };
}

/** Answers the main window's questions about the terminals this window draws.
    Returns the unsubscribe. */
export function serveTerminals(
  find: (paneId: string) => Pick<Terminal, "paste" | "buffer" | "modes"> | null,
): () => void {
  let off: (() => void) | null = null;
  let stopped = false;
  listen<Ask>("terminal:ask", (event) => {
    const { reqId, paneId, op } = event.payload;
    const term = find(paneId);
    // Not ours: some other window answers, or nobody, and the asker times out.
    if (!term) return;
    const access = localTerminal(term);
    let value: unknown;
    if (op.kind === "probe") value = { applicationCursor: access.applicationCursor };
    else if (op.kind === "paste") {
      access.paste(op.text);
      value = true;
    } else value = access.read(op.lines);
    emit("terminal:answer", { reqId, value } satisfies Answer).catch(() => undefined);
  })
    .then((stop) => {
      if (stopped) stop();
      else off = stop;
    })
    .catch(() => undefined);
  return () => {
    stopped = true;
    off?.();
  };
}
