import { invoke } from "@tauri-apps/api/core";
import { boardLoad } from "./board";
import type { ControlDeps } from "./control";
import { workersOf } from "./orchestration";
import { appPortsOf } from "./remote";
import { PORT_COUNT } from "./workerControl";
import type { RemoteApp, RemoteApps } from "../remote/types";
import type { Workspace } from "../types";

/**
 * What the window answers Hangar Mobile about the apps the agents build: the
 * dev servers the phone may open, each through a slot of the mobile server's
 * own (src-tauri/src/remote/apps.rs).
 *
 * A worker's are found by themselves, in the port block its shell was given
 * (HANGAR_PORT_BASE and the 19 after it); the person adds their own to a
 * workspace in Settings › Mobile (`mobilePorts`). Only the ports a server
 * listens on right now are listed: the phone shows what it can open, and the
 * mobile server, which asks this before it opens anything, opens nothing else.
 */

/** remote_ports_listening asks about this many ports at most at once (apps.rs). */
const PROBE_BATCH = 256;

/** Where remote_apps reads from; the tests bring their own. */
export interface AppsSources {
  /** Which of these ports a server takes connections on, on this PC's loopback. */
  listening: (ports: number[]) => Promise<number[]>;
  /** A workspace's board, for its workers' task titles. */
  tasks: (cwd: string) => Promise<{ id: string; title: string }[]>;
}

export const appsSources: AppsSources = {
  listening: async (ports) => {
    const batches: number[][] = [];
    for (let at = 0; at < ports.length; at += PROBE_BATCH) batches.push(ports.slice(at, at + PROBE_BATCH));
    const found = await Promise.all(batches.map((batch) => invoke<number[]>("remote_ports_listening", { ports: batch })));
    return found.flat();
  },
  tasks: (cwd) => boardLoad(cwd).then((board) => board.tasks ?? []),
};

/** A port the phone could open, listening or not; a worker's names its task by id. */
interface Candidate {
  app: RemoteApp;
  taskId: string | null;
}

/**
 * Every port of these workspaces the phone could open: each worker's block,
 * in the order of the panes, then the ports the person added, but one already
 * in a worker's block. The HQ is not a project: it has none.
 */
export function candidatesOf(workspaces: Workspace[]): Candidate[] {
  const candidates: Candidate[] = [];
  for (const ws of workspaces) {
    if (ws.hq) continue;
    const taken = new Set<number>();
    for (const pane of workersOf(ws)) {
      if (typeof pane.portBase !== "number") continue;
      for (let port = pane.portBase; port < pane.portBase + PORT_COUNT; port++) {
        taken.add(port);
        candidates.push({
          app: { workspaceId: ws.id, workspace: ws.name, port, source: "worker", pane: pane.name },
          taskId: pane.taskId ?? null,
        });
      }
    }
    for (const port of appPortsOf(ws)) {
      if (taken.has(port)) continue;
      candidates.push({ app: { workspaceId: ws.id, workspace: ws.name, port, source: "custom" }, taskId: null });
    }
  }
  return candidates;
}

/** A project by id or by name; the HQ is not one. */
function findProject(workspaces: Workspace[], ref: unknown): Workspace {
  const wanted = typeof ref === "string" ? ref.trim() : "";
  if (!wanted) throw new Error("Name the project, by id or by name.");
  const projects = workspaces.filter((ws) => !ws.hq);
  const ws =
    projects.find((entry) => entry.id === wanted) ??
    projects.find((entry) => entry.name.trim().toLowerCase() === wanted.toLowerCase());
  if (!ws) throw new Error(`No project "${wanted}" in Hangar.`);
  return ws;
}

/** The titles of these tasks, from the boards of the workspaces that hold them; a board that cannot be read names none. */
async function titlesOf(
  workspaces: Workspace[],
  listed: Candidate[],
  sources: AppsSources,
): Promise<Map<string, string>> {
  const wanted = new Set(listed.map((candidate) => candidate.taskId).filter((id): id is string => id !== null));
  const holders = workspaces.filter((ws) => listed.some((candidate) => candidate.taskId && candidate.app.workspaceId === ws.id));
  const titles = new Map<string, string>();
  const boards = await Promise.all(holders.map((ws) => sources.tasks(ws.cwd).catch(() => [])));
  for (const tasks of boards) {
    for (const task of tasks) if (wanted.has(task.id)) titles.set(task.id, task.title);
  }
  return titles;
}

/**
 * remote_apps: every project's apps a server listens on, or one project's
 * (`workspace`, by id or name). A worker's carries its pane, and its task's
 * title when its board has it.
 */
export async function remoteApps(
  deps: ControlDeps,
  args: Record<string, unknown>,
  sources: AppsSources = appsSources,
): Promise<RemoteApps> {
  const { workspaces } = deps.snapshot();
  const asked = args.workspace === undefined ? workspaces : [findProject(workspaces, args.workspace)];
  const candidates = candidatesOf(asked);
  if (candidates.length === 0) return { apps: [] };
  const ports = [...new Set(candidates.map((candidate) => candidate.app.port))];
  const listening = new Set(await sources.listening(ports));
  const listed = candidates.filter((candidate) => listening.has(candidate.app.port));
  const titles = await titlesOf(asked, listed, sources);
  return {
    apps: listed.map(({ app, taskId }) => {
      const task = taskId === null ? undefined : titles.get(taskId);
      return task === undefined ? app : { ...app, task };
    }),
  };
}

/** The apps ops of remote_*; any other is not this module's. */
export const isAppsOp = (op: string) => op === "remote_apps";

/** `deps` is the person's: they ask from their phone (lib/remoteControl). */
export async function runApps(
  op: string,
  args: Record<string, unknown>,
  deps: ControlDeps,
  sources: AppsSources = appsSources,
): Promise<unknown> {
  if (op === "remote_apps") return remoteApps(deps, args, sources);
  throw new Error(`Unknown remote request "${op}".`);
}
