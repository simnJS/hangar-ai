import { listSessions } from "./ipc";
import type { AgentId } from "../types";

/**
 * Builds the command typed into a freshly opened shell. Returning null means
 * "leave the user at a plain prompt".
 */
export function launchCommand(agent: AgentId, sessionId: string | null): string | null {
  switch (agent) {
    case "claude":
      return sessionId ? `claude --resume ${sessionId}` : "claude";
    case "codex":
      return sessionId ? `codex resume ${sessionId}` : "codex";
    case "gemini":
      return "gemini";
    case "opencode":
      return "opencode";
    case "shell":
    default:
      return null;
  }
}

export const isResumable = (agent: AgentId) => agent === "claude" || agent === "codex";

/**
 * Session ids already handed to a pane during this app run. Without this, two
 * panes started in the same directory would both latch onto the same new
 * transcript file.
 */
const claimed = new Set<string>();

export function releaseClaim(sessionId: string | null) {
  if (sessionId) claimed.delete(sessionId);
}

export function claim(sessionId: string) {
  claimed.add(sessionId);
}

/**
 * Agents do not report their session id, so we diff the transcript directory:
 * whatever file appears after launch that we did not already know about
 * belongs to this pane.
 */
export async function watchForSession(options: {
  agent: AgentId;
  cwd: string;
  known: Set<string>;
  onFound: (sessionId: string) => void;
  signal: AbortSignal;
  timeoutMs?: number;
}): Promise<void> {
  const { agent, cwd, known, onFound, signal } = options;
  const timeoutMs = options.timeoutMs ?? 120_000;
  const startedAt = Date.now();

  while (!signal.aborted && Date.now() - startedAt < timeoutMs) {
    await sleep(1500, signal);
    if (signal.aborted) return;

    let sessions;
    try {
      sessions = await listSessions(agent, cwd);
    } catch {
      continue;
    }

    const fresh = sessions.find((s) => !known.has(s.id) && !claimed.has(s.id));
    if (fresh) {
      claimed.add(fresh.id);
      onFound(fresh.id);
      return;
    }
  }
}

export async function knownSessionIds(agent: AgentId, cwd: string): Promise<Set<string>> {
  if (!isResumable(agent)) return new Set();
  try {
    return new Set((await listSessions(agent, cwd)).map((s) => s.id));
  } catch {
    return new Set();
  }
}

export function sleep(ms: number, signal?: AbortSignal): Promise<void> {
  return new Promise((resolve) => {
    const timer = setTimeout(resolve, ms);
    signal?.addEventListener("abort", () => {
      clearTimeout(timer);
      resolve();
    });
  });
}
