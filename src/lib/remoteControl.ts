import { useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getPaneActivity } from "./agentState";
import { runControl, type ControlDeps } from "./control";
import { activityHistory, startActivityLog, textOf, type ActivityHistory } from "./activityLog";
import { managerJournal, type ManagerEvent } from "./managerEvents";
import { managerOf } from "./orchestration";
import { loadProjects, projectsDeps } from "./projects";
import { isMainWindow } from "./windows";
import { systemLocale, translate, type TranslateKey, type TranslateParams } from "../i18n";
import {
  REMOTE_KEYS,
  type RemoteActivity,
  type RemoteHistory,
  type RemoteKey,
  type RemoteScreen,
  type RemoteState,
  type RemoteWaiting,
} from "../remote/types";
import type { AppState, Pane, Workspace } from "../types";

/**
 * What the window answers to Hangar Mobile (the phone app): the remote_*
 * internal requests the Rust mobile server relays as `control:internal`. The
 * person is the one asking, from their phone, so what is sent is typed as
 * them — agents' pane control being turned off does not stop it.
 *
 * Claude Code's permission prompt takes one key: Enter accepts the highlighted
 * "1. Yes", Escape is "No, and tell Claude what to do differently". The
 * hangar-bridge mod only reports that a prompt waits (permission {waiting}),
 * never what it offers, so those two keys are all that is sent.
 */

const SCREEN_DEFAULT = 80;
const NO_MANAGER = "Open the project manager on the PC";
/** A closed or asleep workspace gets this long to start a terminal for a send. */
const OPEN_WAIT_MS = 20_000;
const OPEN_POLL_MS = 500;

const same = (a: string, b: string) => a.trim().toLowerCase() === b.trim().toLowerCase();

const activityOf = (paneId: string): RemoteActivity => getPaneActivity(paneId)?.activity ?? null;

/** A pane by id or by name, wherever it is. */
function findPane(state: AppState, ref: unknown): { ws: Workspace; pane: Pane } {
  const wanted = typeof ref === "string" ? ref.trim() : "";
  if (!wanted) throw new Error("Name the pane, by id or by name.");
  for (const ws of state.workspaces) {
    const pane = ws.panes.find((entry) => entry.id === wanted);
    if (pane) return { ws, pane };
  }
  for (const ws of state.workspaces) {
    const pane = ws.panes.find((entry) => same(entry.name, wanted));
    if (pane) return { ws, pane };
  }
  throw new Error(`No pane "${wanted}" in Hangar.`);
}

/** Since when something waits, when the journals cannot say: the first time it was seen. */
const firstSeen = new Map<string, number>();

function since(key: string, journaled: number | undefined, at: number): number {
  if (journaled !== undefined) {
    firstSeen.set(key, journaled);
    return journaled;
  }
  const known = firstSeen.get(key);
  if (known !== undefined) return known;
  firstSeen.set(key, at);
  return at;
}

/** The latest time the manager's journal recorded an event of this kind. */
function journaledAt(match: (event: ManagerEvent) => boolean): number | undefined {
  let latest: number | undefined;
  for (const entry of managerJournal()) {
    if (match(entry.event)) latest = Math.max(latest ?? 0, entry.at);
  }
  return latest;
}

async function remoteState(deps: ControlDeps): Promise<RemoteState> {
  const at = Date.now();
  const state = deps.snapshot();
  const summaries = await loadProjects(deps.projects ?? projectsDeps(deps.snapshot, deps.isOpen));

  const waiting: RemoteWaiting[] = [];
  const seen = new Set<string>();
  for (const ws of state.workspaces) {
    for (const pane of ws.panes) {
      if (pane.agent === "shell" || activityOf(pane.id) !== "waiting") continue;
      const key = `pane:${pane.id}`;
      seen.add(key);
      waiting.push({
        kind: "pane",
        workspaceId: ws.id,
        workspace: ws.name,
        paneId: pane.id,
        pane: pane.name,
        since: since(key, journaledAt((e) => e.kind === "paneWaiting" && e.paneId === pane.id), at),
      });
    }
  }
  for (const project of summaries) {
    for (const [kind, tasks] of [
      ["needsYou", project.needsYou],
      ["readyToLand", project.readyToLand],
    ] as const) {
      for (const task of tasks) {
        const key = `${kind}:${project.id}:${task.id}`;
        seen.add(key);
        waiting.push({
          kind,
          workspaceId: project.id,
          workspace: project.name,
          taskId: task.id,
          title: task.title,
          since: since(
            key,
            journaledAt((e) => e.kind === (kind === "needsYou" ? "taskNeedsYou" : "readyToLand") && e.taskId === task.id),
            at,
          ),
        });
      }
    }
  }
  for (const key of [...firstSeen.keys()]) if (!seen.has(key)) firstSeen.delete(key);
  waiting.sort((a, b) => a.since - b.since);

  const manager = managerOf(state);
  return {
    at,
    manager: manager
      ? { paneId: manager.pane.id, name: manager.pane.name, activity: activityOf(manager.pane.id) }
      : null,
    waiting,
    projects: summaries.map((project) => {
      const ws = state.workspaces.find((entry) => entry.id === project.id);
      return {
        id: project.id,
        name: project.name,
        open: project.open,
        asleep: project.asleep,
        git: project.git,
        orchestrator: project.orchestrator,
        panes: (ws?.panes ?? []).map((pane) => ({
          id: pane.id,
          name: pane.name,
          agent: pane.agent,
          role: pane.role ?? null,
          activity: activityOf(pane.id),
        })),
        board: project.board,
        planStatus: project.planStatus,
        needsYou: project.needsYou,
        readyToLand: project.readyToLand,
      };
    }),
  };
}

async function remoteScreen(deps: ControlDeps, args: Record<string, unknown>): Promise<RemoteScreen> {
  const { ws, pane } = findPane(deps.snapshot(), args.pane);
  const lines = typeof args.lines === "number" ? args.lines : SCREEN_DEFAULT;
  const read = (await runControl("pane_read", { workspace: ws.id, pane: pane.id, lines }, deps)) as {
    text: string;
  };
  return {
    paneId: pane.id,
    pane: pane.name,
    workspace: ws.name,
    agent: pane.agent,
    activity: activityOf(pane.id),
    text: read.text,
  };
}

/**
 * Typed as the person: pane_send's path, without when_idle. A workspace that
 * is asleep or closed is opened first, and waited for.
 */
async function type(
  deps: ControlDeps,
  ws: Workspace,
  pane: Pane,
  input: { text?: string; keys?: string[]; submit?: boolean },
) {
  if (!(await deps.terminal(pane.id))) {
    deps.openWorkspace(ws.id);
    for (let waited = 0; waited < OPEN_WAIT_MS; waited += OPEN_POLL_MS) {
      await deps.sleep(OPEN_POLL_MS);
      if (await deps.terminal(pane.id)) break;
    }
  }
  await runControl(
    "pane_send",
    { workspace: ws.id, pane: pane.id, text: input.text, keys: input.keys, submit: input.submit },
    deps,
  );
}

async function remoteSend(deps: ControlDeps, args: Record<string, unknown>): Promise<{ sent: true }> {
  const { ws, pane } = findPane(deps.snapshot(), args.pane);
  const keys = Array.isArray(args.keys) ? args.keys.map((key) => String(key).toLowerCase()) : [];
  const bad = keys.find((key) => !REMOTE_KEYS.includes(key as RemoteKey));
  if (bad) throw new Error(`Unknown key "${bad}". One of: ${REMOTE_KEYS.join(", ")}.`);
  await type(deps, ws, pane, {
    text: typeof args.text === "string" ? args.text : undefined,
    keys,
    submit: args.submit === false ? false : undefined,
  });
  return { sent: true };
}

async function remotePermission(deps: ControlDeps, args: Record<string, unknown>): Promise<{ answered: true }> {
  if (typeof args.allow !== "boolean") throw new Error('Say whether to allow: "allow" is true or false.');
  const { ws, pane } = findPane(deps.snapshot(), args.pane);
  if (activityOf(pane.id) !== "waiting") {
    throw new Error(`"${pane.name}" is not waiting for a permission: nothing was sent.`);
  }
  await type(deps, ws, pane, { keys: [args.allow ? "enter" : "escape"] });
  return { answered: true };
}

async function remoteManager(deps: ControlDeps, args: Record<string, unknown>): Promise<{ sent: true }> {
  const text = typeof args.text === "string" ? args.text : "";
  if (!text.trim()) throw new Error("Nothing to send: give text.");
  const manager = managerOf(deps.snapshot());
  if (!manager) throw new Error(NO_MANAGER);
  await type(deps, manager.ws, manager.pane, { text });
  return { sent: true };
}

/**
 * What happened, from the log kept on disk (lib/activityLog), newest first:
 * the journals in memory forget with every restart, the disk does not.
 */
async function remoteHistory(
  deps: ControlDeps,
  args: Record<string, unknown>,
  history: ActivityHistory,
): Promise<RemoteHistory> {
  const from = typeof args.since === "number" && args.since > 0 ? args.since : 0;
  const locale = deps.snapshot().settings.locale ?? systemLocale();
  const say = (key: TranslateKey, params?: TranslateParams) =>
    translate(locale, key, params);
  const entries = await history.read(from);
  return {
    entries: entries
      .filter((entry) => entry.at > from)
      .map((entry) => ({ ...entry, text: textOf(entry, say) }))
      .sort((a, b) => b.at - a.at),
  };
}

/** Every op this module answers; any other is not its to answer. */
export const isRemoteOp = (op: string) => op.startsWith("remote_");

export async function runRemote(
  op: string,
  args: Record<string, unknown>,
  deps: ControlDeps,
  history: ActivityHistory = activityHistory,
): Promise<unknown> {
  // The person asks: not the agents' switch (agentPaneControl) to refuse.
  const asPerson: ControlDeps = { ...deps, enabled: () => true };
  switch (op) {
    case "remote_state":
      return remoteState(asPerson);
    case "remote_screen":
      return remoteScreen(asPerson, args);
    case "remote_send":
      return remoteSend(asPerson, args);
    case "remote_permission":
      return remotePermission(asPerson, args);
    case "remote_manager":
      return remoteManager(asPerson, args);
    case "remote_history":
      return remoteHistory(asPerson, args, history);
    default:
      throw new Error(`Unknown remote request "${op}".`);
  }
}

interface InternalRequest {
  id: string;
  op: string;
  args: Record<string, unknown>;
}

type Send = (id: string, ok: boolean, value: unknown, error: string | null) => Promise<void>;

const reply: Send = (id, ok, value, error) =>
  invoke<void>("control_reply", { id, ok, value: value ?? null, error });

/** Answers one internal request if it is a remote_* one; false when it is not. */
export function answerRemote(request: InternalRequest, deps: ControlDeps | null, send: Send = reply): boolean {
  if (!isRemoteOp(request.op)) return false;
  const run = deps
    ? runRemote(request.op, request.args ?? {}, deps)
    : Promise.reject(new Error("Hangar.AI is still starting. Try again in a moment."));
  run
    .then((value) => send(request.id, true, value, null))
    .catch((err) => send(request.id, false, null, err instanceof Error ? err.message : String(err)))
    .catch(() => undefined);
  return true;
}

/**
 * Answers Hangar Mobile's requests for as long as the main window is up.
 * `deps` is read when a request lands; null means the store has not loaded.
 */
export function useRemoteControl(deps: ControlDeps | null) {
  const depsRef = useRef(deps);
  depsRef.current = deps;
  const loaded = deps !== null;

  // The history is written from the moment the store has loaded.
  useEffect(() => {
    if (!isMainWindow || !loaded) return;
    return startActivityLog(() => depsRef.current!.snapshot());
  }, [loaded]);

  useEffect(() => {
    if (!isMainWindow) return;
    let cancelled = false;
    let off: (() => void) | null = null;
    listen<InternalRequest>("control:internal", (event) => {
      answerRemote(event.payload, depsRef.current);
    })
      .then((stop) => {
        if (cancelled) stop();
        else off = stop;
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
      off?.();
    };
  }, []);
}
