import type { Caller, ControlDeps } from "./control";
import { orchestratorOf } from "./orchestration";
import { loadProjects, projectsDeps, renderOverview, renderProject } from "./projects";
import type { Workspace } from "../types";

/**
 * The project_* control operations: the project manager (the pane with role
 * "manager", in the HQ workspace) seeing every project and passing the
 * person's instructions on to each project's orchestrator (lib/projects).
 *
 * Only the manager calls them, and it acts only when the person asks.
 *
 * Like the rest of lib/control, what is thrown is read by a model: English,
 * and saying what to do next.
 */

const text = (value: unknown) =>
  typeof value === "string" && value.trim() !== "" ? value.trim() : null;

/** Same folder, the way Windows compares them: either separator, any case. */
const normalize = (path: string) => path.replace(/[\\/]+$/, "").replace(/\\/g, "/").toLowerCase();

/** How the orchestrator reads what the manager passes on. */
export const RELAY_PREFIX = "Relayed by the project manager from the person:";

/** The manager pane's name, when it is the one calling; refused otherwise. */
function managerCalling(deps: ControlDeps, caller: Caller, op: string): string {
  const me = caller.paneId
    ? deps
        .snapshot()
        .workspaces.flatMap((ws) => ws.panes)
        .find((pane) => pane.id === caller.paneId)
    : undefined;
  if (me?.role === "manager") return me.name;
  throw new Error(
    `${op} is for the project manager, the pane of the HQ workspace, and you are not it. ` +
      "Work on your own workspace with your own tools; if the person wants something from another project, " +
      "tell them to ask the project manager.",
  );
}

/** The project asked for, by id, name or folder; the HQ is not one. */
function projectFor(deps: ControlDeps, ref: unknown): Workspace {
  const workspaces = deps.snapshot().workspaces;
  const names = workspaces.filter((ws) => !ws.hq).map((ws) => `"${ws.name}"`).join(", ") || "none";
  const wanted = text(ref);
  if (!wanted) throw new Error(`Name the project (its name or folder). Projects: ${names}.`);
  const lower = wanted.toLowerCase();
  const found =
    workspaces.find((ws) => ws.id === wanted) ??
    workspaces.find((ws) => ws.name.toLowerCase() === lower) ??
    workspaces.find((ws) => normalize(ws.cwd) === normalize(wanted));
  if (!found) throw new Error(`No project "${wanted}". Projects: ${names}.`);
  if (found.hq) {
    throw new Error(`"${found.name}" is the HQ, your own workspace, not a project. Projects: ${names}.`);
  }
  return found;
}

const summaries = (deps: ControlDeps) =>
  loadProjects(deps.projects ?? projectsDeps(deps.snapshot, deps.isOpen));

export async function managerControl(
  op: string,
  args: Record<string, unknown>,
  caller: Caller,
  deps: ControlDeps,
): Promise<unknown> {
  const me = managerCalling(deps, caller, op);
  switch (op) {
    case "project_list": {
      const projects = await summaries(deps);
      return { projects, text: renderOverview(projects, Date.now()) };
    }

    case "project_status": {
      const ws = projectFor(deps, args.workspace);
      const project = (await summaries(deps)).find((entry) => entry.id === ws.id);
      if (!project) throw new Error(`"${ws.name}" is gone. Call project_list again.`);
      return { project, text: renderProject(project) };
    }

    case "project_send": {
      const ws = projectFor(deps, args.workspace);
      const body = text(args.text);
      if (!body) throw new Error("Nothing to send: give the person's instruction in text.");
      const orchestrator = orchestratorOf(ws);
      if (!orchestrator) {
        throw new Error(
          `"${ws.name}" has no orchestrator, so there is nobody to relay to. Only the person can pin one, ` +
            "from the menu of a Claude Code pane; tell them, and ask whether they want one.",
        );
      }
      // An agent acting on a workspace is using it: its sleep waits.
      deps.touch(ws.id);
      await deps.enqueue(orchestrator.id, `${RELAY_PREFIX} ${body}`, me);
      // Held for a Claude Code that is not running, it would wait for good:
      // an asleep or unopened workspace is opened, as pane_send does.
      const woke = deps.asleep(ws.id);
      const starting = !deps.isOpen(ws.id);
      if (starting) deps.openWorkspace(ws.id);
      return {
        queued: true,
        workspace: ws.name,
        orchestrator: orchestrator.name,
        ...(woke ? { woke } : starting ? { starting } : {}),
        note:
          `Queued for "${orchestrator.name}", the orchestrator of "${ws.name}", until it is idle. ` +
          "Its progress comes back to you as events; do not poll.",
      };
    }

    default:
      throw new Error(`Unknown operation "${op}".`);
  }
}
