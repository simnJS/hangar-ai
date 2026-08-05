import { listSessions } from "./ipc";
import type { AgentId } from "../types";

/** What each agent is started with when nothing was customised. */
export const DEFAULT_AGENT_COMMANDS: Record<Exclude<AgentId, "shell">, string> = {
  claude: "claude",
  codex: "codex",
  gemini: "gemini",
  opencode: "opencode",
};

/**
 * The command a pane starts an agent with: whatever the settings hold for it,
 * or the plain binary name.
 *
 * Trimmed, and an empty override reads as "not customised" rather than as a
 * pane that types nothing — clearing the field in the settings is how you go
 * back to the default.
 */
export function agentCommand(agent: AgentId, commands?: Record<string, string>): string | null {
  if (agent === "shell") return null;
  return commands?.[agent]?.trim() || DEFAULT_AGENT_COMMANDS[agent];
}

/**
 * A path safe to put between double quotes in a command line.
 *
 * These roots are read out of a `.code-workspace` file, which can arrive with a
 * cloned repository — nobody types them. A newline would run whatever follows
 * it as its own command, and a quote, backtick or `$` would end the quoting and
 * let the rest of the path be read as shell syntax. None of them belong in a
 * project path, so a root carrying one is dropped rather than escaped.
 */
const isQuotable = (root: string) => !/["`$\r\n]/.test(root);

/**
 * Builds the command typed into a freshly opened shell. Returning null means
 * "leave the user at a plain prompt".
 *
 * The resume arguments are appended to the command rather than replacing it,
 * so a custom launcher — an alias carrying flags of its own, a wrapper script
 * — resumes a conversation as well as the bare binary does.
 *
 * `extraRoots` holds the folders a multi-root workspace has beyond the one the
 * terminal opened in. Only Claude Code takes them on the command line; the
 * others are left with their working directory rather than handed a flag they
 * would refuse to start with.
 *
 * Both arrive in one options object rather than as two more positional
 * parameters, so a caller that only cares about one of them does not have to
 * name the other to get past it.
 */
export function launchCommand(
  agent: AgentId,
  sessionId: string | null,
  options: { extraRoots?: string[]; commands?: Record<string, string> } = {},
): string | null {
  const command = agentCommand(agent, options.commands);
  if (!command) return null;

  switch (agent) {
    case "claude": {
      const resume = sessionId ? ` --resume ${sessionId}` : "";
      const roots = (options.extraRoots ?? []).filter(isQuotable);
      // `--add-dir` is variadic, so it comes last: a following flag — one the
      // custom command carries, or the resume above — would be swallowed as
      // one more directory.
      const added = roots.length
        ? ` --add-dir ${roots.map((root) => `"${root}"`).join(" ")}`
        : "";
      return `${command}${resume}${added}`;
    }
    case "codex":
      return sessionId ? `${command} resume ${sessionId}` : command;
    default:
      return command;
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

/** Brisk while hunting the first id, patient once the pane has one. */
const HUNT_MS = 1500;
const FOLLOW_MS = 5000;

/**
 * A transcript has to be warm to be ours. Without this a pane could adopt any
 * file it happens not to know about — one left by another window, say —
 * instead of the conversation that just started under it.
 */
const FRESH_MS = 15_000;

/**
 * Agents do not report their session id, so we diff the transcript directory:
 * whatever file appears that we did not already know about belongs to this
 * pane.
 *
 * This runs for the whole life of the pane, not just at launch. Typing /new
 * or /clear inside an agent abandons the current transcript for a fresh one,
 * and a pane that stopped watching would keep pointing at the old chat — and
 * reopen it on the next resume, which is not the conversation you left.
 */
export async function watchForSession(options: {
  agent: AgentId;
  cwd: string;
  known: Set<string>;
  onFound: (sessionId: string) => void;
  signal: AbortSignal;
  /**
   * Consumed once per round. A pane that printed nothing since the last look
   * cannot have opened a session, so it is skipped without touching the disk
   * — and a transcript another pane just created is far less likely to be
   * mistaken for this one's.
   */
  hadOutput?: () => boolean;
}): Promise<void> {
  const { agent, cwd, known, onFound, signal, hadOutput } = options;
  let intervalMs = HUNT_MS;

  while (!signal.aborted) {
    await sleep(intervalMs, signal);
    if (signal.aborted) return;
    if (hadOutput && !hadOutput()) continue;

    let sessions;
    try {
      sessions = await listSessions(agent, cwd);
    } catch {
      continue;
    }

    // Sorted most recently touched first, so the first match is the newest.
    const now = Date.now();
    const fresh = sessions.find(
      (s) => !known.has(s.id) && !claimed.has(s.id) && now - s.modified_ms < FRESH_MS,
    );
    if (!fresh) continue;

    claimed.add(fresh.id);
    // Remembered as ours, so the next round hunts the one after it.
    known.add(fresh.id);
    onFound(fresh.id);
    intervalMs = FOLLOW_MS;
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
    // Both paths unregister the other, so the polling loop below does not pile
    // up one abort listener per iteration on a signal that lives as long as
    // the pane does.
    const onAbort = () => {
      clearTimeout(timer);
      resolve();
    };
    const timer = setTimeout(() => {
      signal?.removeEventListener("abort", onAbort);
      resolve();
    }, ms);
    signal?.addEventListener("abort", onAbort, { once: true });
  });
}
