import {
  boardClaimWorker,
  boardLoad,
  boardReleaseWorker,
  type Task,
} from "./board";
import type { Caller, ControlDeps } from "./control";
import {
  gitBranchDelete,
  gitChanges,
  gitCommitsAhead,
  gitCopyUntracked,
  gitRepoInfo,
  gitWorktreeAdd,
  gitWorktreeRemove,
  type Changes,
  type CopyResult,
  type DeleteBranchResult,
  type RemoveResult,
  type RepoInfo,
  type Worktree,
} from "./git";
import { ptyKill } from "./ipc";
import { MAX_PANES } from "./layout";
import { orchestrationOf, orchestratorOf, workersOf } from "./orchestration";
import { postOrchestratorEvent } from "./orchestratorEvents";
import { allPaneNames, pickPaneName } from "./paneNames";
import { COPY_PATTERNS, sanitizeBranchName, worktreeFolder } from "./worktrees";
import type { Pane, Workspace } from "../types";

/**
 * The worker_* control operations: an orchestrator starting, listing and
 * stopping the workers of its workspace (lib/orchestration).
 *
 * A worker is a Claude Code pane on one board task, in a git worktree of its
 * own, with ports of its own and a brief written from the task. Starting one
 * is all or nothing: the claim, the worktree, the pane and the brief either
 * all happen, or whatever was done is undone and the task is back in todo.
 * The limits — workers at once, attempts per task, files two tasks in doing
 * may not share — are refused here and in board.rs, never left to a prompt.
 *
 * Like the rest of lib/control, what is thrown is read by a model: English,
 * and saying what to do next.
 */

/** What the worker operations reach beyond the panes (ControlDeps.workers). */
export interface WorkerDeps {
  /** Patches a pane in place: done before its terminal mounts, it starts with the patch. */
  updatePane: (workspaceId: string, paneId: string, patch: Partial<Pane>) => void;
  /** Ends a pane's shell and everything it started. */
  killPane: (paneId: string) => Promise<void>;
  boardLoad: (cwd: string) => Promise<{ tasks: Task[] }>;
  /** board.rs `claim_for_worker`: refused, with the reason, for every limit on the task. */
  claim: (cwd: string, id: string, worker: string, maxAttempts: number) => Promise<Task>;
  /** board.rs `release_worker`. */
  release: (cwd: string, id: string, worker: string, undo: boolean) => Promise<Task>;
  repoInfo: (path: string) => Promise<RepoInfo>;
  worktreeAdd: (args: {
    path: string;
    worktreePath: string;
    branch: string;
    createBranch: boolean;
    baseRef: string | null;
  }) => Promise<void>;
  worktreeRemove: (path: string, worktreePath: string, force: boolean) => Promise<RemoveResult>;
  branchDelete: (path: string, branch: string, force: boolean) => Promise<DeleteBranchResult>;
  /** The files git ignores that the patterns name: a worker's `.env`. */
  copyIgnored: (from: string, to: string, patterns: string[]) => Promise<CopyResult>;
  /** What is not committed in the working tree holding `path`. */
  changes: (path: string) => Promise<Changes>;
  commitsAhead: (path: string, base: string, head: string) => Promise<number>;
}

/** The app's own: Rust behind each of them. */
export const liveWorkerDeps = (updatePane: WorkerDeps["updatePane"]): WorkerDeps => ({
  updatePane,
  killPane: (paneId) => ptyKill(paneId, true),
  boardLoad,
  claim: boardClaimWorker,
  release: boardReleaseWorker,
  repoInfo: gitRepoInfo,
  worktreeAdd: gitWorktreeAdd,
  worktreeRemove: gitWorktreeRemove,
  branchDelete: gitBranchDelete,
  copyIgnored: (from, to, patterns) => gitCopyUntracked(from, to, patterns, true),
  changes: (path) => gitChanges(path, null),
  commitsAhead: gitCommitsAhead,
});

/** How many ports a worker gets: HANGAR_PORT_COUNT (pty.rs). */
export const PORT_COUNT = 20;
/**
 * The pool every workspace's workers share: below the range each system hands
 * out for outgoing connections (Linux from 32768, Windows and macOS from
 * 49152), so a worker's server never meets a port the system lent elsewhere,
 * and short of 27017, which MongoDB listens on by default.
 */
export const PORT_POOL_START = 21000;
const PORT_POOL_END = 26999;

/** Branch names tried for one task before giving up: `x`, `x-2`, … */
const BRANCH_TRIES = 9;
/** Folders tried for one branch: `x`, `x-1`, … — VS Code's 20 as well. */
const FOLDER_TRIES = 20;
/** Longest piece of the task's title a branch name carries. */
const SLUG_MAX = 32;
/** How long a killed worker's processes get to let go of its worktree. */
const SETTLE_MS = 400;
/** Windows refuses to delete a folder a dying process still stands in. */
const REMOVE_TRIES = 6;
const REMOVE_RETRY_MS = 500;

const text = (value: unknown) =>
  typeof value === "string" && value.trim() !== "" ? value.trim() : null;

const messageOf = (err: unknown) => (err instanceof Error ? err.message : String(err));

/** Same folder, the way Windows compares them: either separator, any case. */
const comparable = (path: string) =>
  path
    .replace(/^\\\\\?\\/, "")
    .replace(/[\\/]+$/, "")
    .replace(/\\/g, "/")
    .toLowerCase();

/**
 * Where `folder` sits inside `root`, in forward slashes: "" for the root
 * itself, null outside it. A workspace on a subfolder of its repository has
 * its workers start in the same subfolder of their worktrees.
 */
function inside(root: string, folder: string): string | null {
  const base = comparable(root);
  const target = comparable(folder);
  if (target === base) return "";
  if (!target.startsWith(`${base}/`)) return null;
  // Cut from the folder as given, so the subfolder keeps its own case.
  const given = folder.replace(/^\\\\\?\\/, "").replace(/[\\/]+$/, "").replace(/\\/g, "/");
  return given.slice(given.length - (target.length - base.length - 1));
}

const joinPath = (root: string, relative: string) => {
  if (!relative) return root;
  const separator = root.includes("\\") ? "\\" : "/";
  return `${root.replace(/[\\/]+$/, "")}${separator}${relative.split("/").join(separator)}`;
};

/** The worktree a pane works in: the deepest one holding its folder. */
const worktreeOf = (trees: Worktree[], folder: string) =>
  trees
    .filter((tree) => inside(tree.path, folder) !== null)
    .sort((a, b) => b.path.length - a.path.length)[0];

/** A worktree's folder, spelled the way the pane has it when the pane is at its root. */
const rootOf = (tree: Worktree, folder: string) => (inside(tree.path, folder) === "" ? folder : tree.path);

/**
 * The starts still under way. They hold their place, their name and their
 * ports from the first check to the end, so two starts at once can neither
 * both take the last free worker, nor the same name or the same ports.
 */
const starting = {
  names: new Set<string>(),
  ports: new Set<number>(),
  byWorkspace: new Map<string, number>(),
};

const startingIn = (workspaceId: string) => starting.byWorkspace.get(workspaceId) ?? 0;

/**
 * The first block of PORT_COUNT ports nobody has, from a pool shared by every
 * workspace: 21000, 21020, 21040… A worker keeps its block for as long as
 * its pane lives, restarts included. Null when every block is taken.
 */
export function allocatePortBase(
  workspaces: Workspace[],
  reserved: Iterable<number> = [],
): number | null {
  const taken = [...reserved];
  for (const ws of workspaces) {
    for (const pane of ws.panes) {
      if (typeof pane.portBase === "number") taken.push(pane.portBase);
    }
  }
  for (let base = PORT_POOL_START; base + PORT_COUNT - 1 <= PORT_POOL_END; base += PORT_COUNT) {
    if (taken.every((other) => other + PORT_COUNT <= base || base + PORT_COUNT <= other)) return base;
  }
  return null;
}

/**
 * A task's title as a branch can carry it: ASCII, lower case, dashes, cut at
 * a word. Accents go ("écran" is "ecran"); a title with no Latin letter at
 * all leaves "task".
 */
export function taskSlug(title: string): string {
  const words = title
    .normalize("NFD")
    .replace(/[̀-ͯ]/g, "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  if (words.length <= SLUG_MAX) return words || "task";
  const cut = words.slice(0, SLUG_MAX);
  const dash = cut.lastIndexOf("-");
  return (dash >= SLUG_MAX / 2 ? cut.slice(0, dash) : cut).replace(/-+$/, "");
}

/** `hangar/<title>-<start of the id>`: whose it is, what it is, and which task. */
export const workerBranch = (task: Pick<Task, "id" | "title">) =>
  sanitizeBranchName(`hangar/${taskSlug(task.title)}-${task.id.replace(/[^\w]/g, "").slice(0, 6)}`);

/** Everything the brief is written from. */
export interface BriefFacts {
  task: Pick<Task, "id" | "title" | "description" | "owns">;
  worker: string;
  /** Null when the person started it rather than an orchestrator. */
  orchestrator: string | null;
  workspace: string;
  branch: string;
  target: string;
  worktree: string;
  portBase: number;
  checkCommand: string | null;
  attempt: number;
  maxAttempts: number;
}

/**
 * The worker's first message, and all it knows: it starts with a fresh
 * context. The task's description carries the goal and its acceptance; this
 * says where to work, what not to touch, how to report, when to stop and how
 * to hand the work in.
 */
export function workerBrief(facts: BriefFacts): string {
  const { task, worker, orchestrator, branch, target, worktree, portBase } = facts;
  const owns = (task.owns ?? []).map((pattern) => pattern.trim()).filter(Boolean);
  const lines: (string | null)[] = [
    `You are ${worker}, a worker in the Hangar workspace "${facts.workspace}". ` +
      (orchestrator ? `Its orchestrator, ${orchestrator}, started you` : "The person started you") +
      " on one board task, and that task is your whole job.",
    "",
    `## Task ${task.id}: ${task.title}`,
    task.description.trim() || "(No description beyond the title.)",
    facts.attempt > 1
      ? `\nThis is attempt ${facts.attempt} of ${facts.maxAttempts}: an earlier worker did not finish it. ` +
        "Read the task's comments (board_list_tasks) before you start."
      : null,
    "",
    "## Where you work",
    `- Your folder is a git worktree of its own: ${worktree}, on branch ${branch}, made from ${target}. ` +
      `Work and commit there only. Never push, and never merge into ${target} yourself.`,
    owns.length
      ? `- Files you own: ${owns.map((pattern) => `\`${pattern}\``).join(", ")}. Change nothing outside them. ` +
        "If the task cannot be done without another file, name it in a comment on the task and stop."
      : "- No files were assigned to this task. Change only what it needs, and name every file you changed in your report.",
    `- Your ports: HANGAR_PORT_BASE to HANGAR_PORT_BASE+19 (${portBase} to ${portBase + PORT_COUNT - 1}; ` +
      "HANGAR_PORT_BASE and HANGAR_PORT_COUNT are set in your shell). Bind every server, test server and " +
      "debugger you start to these: other agents run beside you.",
    "- The task is claimed for you and in doing. Do not claim, move or reassign it: Hangar does.",
    "",
    "## Done means",
    "- What the task above asks for, its acceptance criteria included.",
    facts.checkCommand
      ? `- \`${facts.checkCommand}\` passes in your worktree. It runs again, with a review of your diff, when you deliver.`
      : "- The project's own checks pass for what you changed: its tests, its build. A reviewer reads your diff when you deliver.",
    "",
    "## Report",
    `Before you stop, whatever the reason, leave one short comment on task ${task.id} (board_comment_task): ` +
      "what you did, and each command you ran to check it, with its result.",
    "",
    "## When blocked",
    "If something is missing — a decision, an access, a file outside yours, a dependency that does not work — " +
      "say what it is in a comment on the task and stop. Never mock, stub or guess your way past it.",
    "",
    "## When done",
    `Commit everything on ${branch}, leave the comment above, then call delivery_submit with a one-line summary. ` +
      "Hangar checks your branch, has it reviewed and lands it; if anything fails, a message tells you what to fix.",
  ];
  return lines.filter((line): line is string => line !== null).join("\n");
}

function workerDeps(deps: ControlDeps): WorkerDeps {
  if (!deps.workers) throw new Error("Workers cannot be started from this Hangar window.");
  return deps.workers;
}

/** The workspace's orchestrator, when it is the one calling; refused otherwise. */
function orchestratorCalling(ws: Workspace, caller: Caller, op: string): Pane {
  const orchestrator = orchestratorOf(ws);
  const me = caller.paneId ? ws.panes.find((pane) => pane.id === caller.paneId) : undefined;
  if (orchestrator && me?.id === orchestrator.id) return orchestrator;
  if (me?.role === "worker") {
    throw new Error(
      `${op} is for the orchestrator of "${ws.name}", and you are one of its workers. Do the task in your ` +
        "brief; if it needs more hands or other files, say so in a comment on your task (board_comment_task) and stop.",
    );
  }
  if (!orchestrator) {
    throw new Error(
      `${op} is for a workspace's orchestrator, and "${ws.name}" has none. Only the person can pin one, ` +
        "from the menu of a Claude Code pane; ask them if this workspace should have one.",
    );
  }
  throw new Error(
    `${op} is for the orchestrator of "${ws.name}", which is "${orchestrator.name}", not you. ` +
      "Leave the work on the board for it, or ask it with pane_send (when_idle=true).",
  );
}

function modelArg(value: unknown): string | null {
  const wanted = text(value);
  if (!wanted) return null;
  if (!/^[\w.:[\]-]{1,80}$/.test(wanted)) {
    throw new Error(
      `"${wanted}" is not a model name. Use an alias — sonnet, opus, haiku — or a full model id such as claude-sonnet-4-5.`,
    );
  }
  return wanted;
}

function nameArg(value: unknown, state: { workspaces: Workspace[] }): string {
  const taken = [...allPaneNames(state.workspaces), ...starting.names];
  const wanted = text(value);
  if (!wanted) return pickPaneName(taken);
  if (/["`$\r\n]/.test(wanted) || wanted.length > 40) {
    throw new Error(
      `"${wanted}" cannot name a pane: keep it under 40 characters, without quotes, backticks or $. ` +
        "Leave name out to get the next free first name.",
    );
  }
  const holder = state.workspaces.find((ws) =>
    ws.panes.some((pane) => pane.name.trim().toLowerCase() === wanted.toLowerCase()),
  );
  if (holder || [...starting.names].some((name) => name.toLowerCase() === wanted.toLowerCase())) {
    throw new Error(
      `A pane is already named "${wanted}"${holder ? ` (in "${holder.name}")` : ""}; pane names are unique ` +
        "across Hangar. Pick another name, or leave name out to get the next free one.",
    );
  }
  return wanted;
}

/** The first of `<path>`, `<path>-1`, `<path>-2`… that does not exist. */
async function freeFolder(deps: ControlDeps, base: string): Promise<string | null> {
  if (!(await deps.dirExists(base))) return base;
  for (let index = 1; index <= FOLDER_TRIES; index++) {
    const candidate = `${base}-${index}`;
    if (!(await deps.dirExists(candidate))) return candidate;
  }
  return null;
}

/** A new branch from `target` and its worktree, beside the main checkout. */
async function addWorktree(
  ws: Workspace,
  mainRoot: string,
  task: Task,
  target: string,
  deps: ControlDeps,
): Promise<{ branch: string; path: string }> {
  const workers = workerDeps(deps);
  const stem = workerBranch(task);
  let last = "";
  for (let round = 1; round <= BRANCH_TRIES; round++) {
    const branch = round === 1 ? stem : `${stem}-${round}`;
    const path = await freeFolder(deps, worktreeFolder(mainRoot, branch));
    if (!path) continue;
    try {
      await workers.worktreeAdd({
        path: ws.cwd,
        worktreePath: path,
        branch,
        createBranch: true,
        baseRef: target,
      });
      return { branch, path };
    } catch (err) {
      last = messageOf(err);
      // A branch left from an earlier attempt, kept for its commits: the next name.
      if (/already exists/i.test(last)) continue;
      throw new Error(
        `Could not create the worker's worktree at ${path} (branch ${branch} from ${target}): ${last}` +
          (/invalid reference|not a valid/i.test(last)
            ? ` Check that ${target} exists; the person sets the branch workers start from in the workspace's orchestration settings.`
            : ""),
      );
    }
  }
  throw new Error(
    `Could not find a free branch name for the worker: ${stem} and its numbered variants are all taken${last ? ` (${last})` : ""}. ` +
      "Delete the old ones from the worktree panel, or ask the person to.",
  );
}

/** What worker_spawn answers. */
export interface SpawnedWorker {
  worker: string;
  task: { id: string; title: string; attempt: number; maxAttempts: number };
  model: string;
  branch: string;
  base: string;
  worktree: string;
  ports: { base: number; count: number };
  brief: string;
  note: string;
}

/**
 * Starts a worker on a task: claim, worktree, pane, brief — all of them, or
 * none. `by` is the orchestrator asking; null when the person does it from
 * the window, and the brief then comes from Hangar.
 */
export async function spawnWorker(
  ws: Workspace,
  request: { task: unknown; name?: unknown; model?: unknown },
  deps: ControlDeps,
  by: Pane | null,
): Promise<SpawnedWorker> {
  const workers = workerDeps(deps);
  const settings = orchestrationOf(ws);
  const state = deps.snapshot();

  const taskId = text(request.task);
  if (!taskId) throw new Error("Give the id of the task to hand over (task), as board_list_tasks shows it.");
  if (state.settings.claudeBridge === false) {
    throw new Error(
      "Hangar's Claude Code bridge is turned off (Settings › Sessions), so a worker would never receive its " +
        "brief. Ask the person to turn it on.",
    );
  }
  const running = workersOf(ws);
  const pending = startingIn(ws.id);
  if (running.length + pending >= settings.maxWorkers) {
    const names = running.map((pane) => `"${pane.name}"`).join(", ");
    throw new Error(
      `"${ws.name}" already runs ${running.length + pending} worker(s)${names ? ` (${names})` : ""}, its limit ` +
        `(maxWorkers ${settings.maxWorkers}). Wait for one to deliver, or stop one with worker_stop, then start ` +
        "the next. The limit is the person's setting.",
    );
  }
  if (ws.panes.length + pending >= MAX_PANES) {
    throw new Error(
      `"${ws.name}" is full: a workspace holds at most ${MAX_PANES} panes. Stop a worker or close a pane first.`,
    );
  }
  const model = modelArg(request.model) ?? settings.workerModel;
  const name = nameArg(request.name, state);
  const portBase = allocatePortBase(state.workspaces, starting.ports);
  if (portBase === null) {
    throw new Error("Every block of worker ports is taken. Stop a worker you no longer need first.");
  }

  // Held from here to the end, before anything is awaited.
  starting.names.add(name);
  starting.ports.add(portBase);
  starting.byWorkspace.set(ws.id, pending + 1);
  /** What was done so far, undone last first when a later step fails. */
  const undo: { what: string; run: () => Promise<unknown> }[] = [];
  try {
    const repo = await workers.repoInfo(ws.cwd);
    if (!repo.installed) {
      throw new Error("git is not installed, or not on the PATH Hangar sees: workers need it for their worktrees.");
    }
    if (!repo.isRepo || !repo.root || !repo.mainRoot) {
      throw new Error(
        `"${ws.name}" (${ws.cwd}) is not a git repository, and each worker works in a git worktree of its own. ` +
          "Ask the person whether to make it one (git init and a first commit).",
      );
    }
    const target = settings.branch ?? repo.branch;
    if (!target) {
      throw new Error(
        `${ws.cwd} is on a detached HEAD, so there is no branch to start workers from. Ask the person to check ` +
          "one out, or to set the branch in the workspace's orchestration settings.",
      );
    }

    const task = await workers.claim(ws.cwd, taskId, name, settings.maxAttempts).catch((err) => {
      throw new Error(messageOf(err));
    });
    undo.push({ what: "give the task back", run: () => workers.release(ws.cwd, taskId, name, true) });

    const { branch, path } = await addWorktree(ws, repo.mainRoot, task, target, deps);
    // A workspace on a subfolder of its repository: the same subfolder there.
    const folder = joinPath(path, inside(repo.root, ws.cwd) ?? "");
    undo.push({
      what: `remove the worktree ${path}`,
      run: async () => {
        await removeWorktree(ws, path, deps);
        await workers.branchDelete(ws.cwd, branch, true);
      },
    });
    // A convenience, as in the worktree panel: a failed copy never undoes the start.
    await workers.copyIgnored(repo.root, path, COPY_PATTERNS).catch(() => null);

    // Read again: the workspace may have changed during the awaits above.
    const current = deps.snapshot().workspaces.find((entry) => entry.id === ws.id);
    if (!current) throw new Error(`"${ws.name}" was closed in the meantime.`);
    const others = workersOf(current);
    const near = others[others.length - 1] ?? orchestratorOf(current);
    const id = deps.addPane(ws.id, { agent: "claude", cwd: folder, name, near: near?.id ?? null });
    if (!id) {
      throw new Error(
        `"${ws.name}" is full: a workspace holds at most ${MAX_PANES} panes. Stop a worker or close a pane first.`,
      );
    }
    // In the same tick as the pane itself: its terminal mounts with all of it.
    workers.updatePane(ws.id, id, { role: "worker", taskId: task.id, model, portBase });
    undo.push({
      what: "close its pane",
      run: async () => {
        await workers.killPane(id).catch(() => undefined);
        deps.closePane(ws.id, id);
        await deps.sleep(SETTLE_MS);
      },
    });
    const created = deps
      .snapshot()
      .workspaces.find((entry) => entry.id === ws.id)
      ?.panes.find((pane) => pane.id === id);
    if (created && created.name !== name) {
      throw new Error(`A pane named "${name}" appeared in the meantime. Try again.`);
    }

    deps.touch(ws.id);
    deps.openWorkspace(ws.id);
    const brief = workerBrief({
      task,
      worker: name,
      orchestrator: by?.name ?? null,
      workspace: ws.name,
      branch,
      target,
      worktree: path,
      portBase,
      checkCommand: settings.checkCommand,
      attempt: task.attempts ?? 1,
      maxAttempts: settings.maxAttempts,
    });
    await deps.enqueue(id, brief, by?.name ?? "hangar");

    deps.announce({ kind: "created", by: by?.name ?? null, pane: name, workspace: ws.name });
    postOrchestratorEvent(ws.id, { kind: "workerStarted", worker: name, taskId: task.id });
    return {
      worker: name,
      task: {
        id: task.id,
        title: task.title,
        attempt: task.attempts ?? 1,
        maxAttempts: settings.maxAttempts,
      },
      model,
      branch,
      base: target,
      worktree: path,
      ports: { base: portBase, count: PORT_COUNT },
      brief: "queued: submitted as soon as its Claude Code session is ready",
      note:
        "Its comments, its delivery, a wait for the person or an exit reach you as events: do not poll it. " +
        "worker_list shows where every worker stands.",
    };
  } catch (err) {
    const failed: string[] = [];
    for (const step of undo.reverse()) {
      await step.run().catch((reason) => failed.push(`could not ${step.what}: ${messageOf(reason)}`));
    }
    const message = messageOf(err);
    throw new Error(
      failed.length
        ? `${message} The start was undone, except that Hangar ${failed.join("; ")}.`
        : undo.length
          ? `${message} Nothing was left behind: the task is back in todo.`
          : message,
    );
  } finally {
    starting.names.delete(name);
    starting.ports.delete(portBase);
    const left = startingIn(ws.id) - 1;
    if (left > 0) starting.byWorkspace.set(ws.id, left);
    else starting.byWorkspace.delete(ws.id);
  }
}

/** Forced: only called once nothing in it is worth keeping. */
async function removeWorktree(ws: Workspace, path: string, deps: ControlDeps): Promise<void> {
  const workers = workerDeps(deps);
  let last: unknown = null;
  for (let attempt = 1; attempt <= REMOVE_TRIES; attempt++) {
    try {
      const result = await workers.worktreeRemove(ws.cwd, path, true);
      if (result.removed) return;
      last = result.message;
    } catch (err) {
      last = err;
    }
    if (attempt < REMOVE_TRIES) await deps.sleep(REMOVE_RETRY_MS);
  }
  throw new Error(messageOf(last ?? "git did not remove it"));
}

/** Where a delivery stands, as far as the board says. */
function deliveryOf(task: Task | undefined): string | null {
  if (!task) return null;
  if (task.column === "done") return "landed";
  if (task.labels.includes("ready-to-land")) return "ready";
  if (task.labels.includes("needs-you")) return "needs-you";
  if (task.column === "review") return "review";
  return null;
}

export async function listWorkers(ws: Workspace, deps: ControlDeps): Promise<unknown> {
  const workers = workerDeps(deps);
  const panes = workersOf(ws);
  const settings = orchestrationOf(ws);
  const [board, trees] = panes.length
    ? await Promise.all([
        workers.boardLoad(ws.cwd).catch(() => ({ tasks: [] as Task[] })),
        deps.worktrees(ws.cwd).catch(() => [] as Worktree[]),
      ])
    : [{ tasks: [] as Task[] }, [] as Worktree[]];
  const open = deps.isOpen(ws.id);
  return {
    workspace: ws.name,
    maxWorkers: settings.maxWorkers,
    maxAttempts: settings.maxAttempts,
    workers: await Promise.all(
      panes.map(async (pane) => {
        const task = board.tasks.find((entry) => entry.id === pane.taskId);
        const tree = pane.cwd ? worktreeOf(trees, pane.cwd) : undefined;
        return {
          name: pane.name,
          task: task
            ? { id: task.id, title: task.title, column: task.column, attempts: task.attempts ?? 0 }
            : pane.taskId
              ? { id: pane.taskId, missing: true }
              : null,
          activity: deps.activity(pane.id)?.activity ?? null,
          running: open ? await deps.alive(pane.id).catch(() => false) : false,
          branch: tree?.branch ?? null,
          worktree: tree && pane.cwd ? rootOf(tree, pane.cwd) : (pane.cwd ?? null),
          model: pane.model ?? null,
          ports: typeof pane.portBase === "number" ? { base: pane.portBase, count: PORT_COUNT } : null,
          delivery: deliveryOf(task),
        };
      }),
    ),
  };
}

/** What worker_stop answers. */
export interface StoppedWorker {
  stopped: string;
  task: { id: string; title: string; column: string } | null;
  worktree: {
    path: string | null;
    branch: string | null;
    removed: boolean;
    branchDeleted: boolean;
    /** Why it was kept. */
    kept?: string;
  };
}

/** Why a worker's worktree has to stay, or null when nothing in it is worth keeping. */
async function workKept(
  ws: Workspace,
  path: string,
  tree: Worktree,
  deps: ControlDeps,
): Promise<string | null> {
  const workers = workerDeps(deps);
  const changes = await workers.changes(path).catch(() => null);
  if (!changes) return "its changes could not be read, so nothing was deleted";
  if (changes.files.length) {
    const named = changes.files.slice(0, 5).map((file) => file.path);
    const more = changes.files.length - named.length;
    return (
      `it has ${changes.files.length} uncommitted change(s): ${named.join(", ")}` +
      (more > 0 ? ` and ${more} more` : "")
    );
  }
  const head = tree.branch ?? tree.head;
  const target =
    orchestrationOf(ws).branch ?? (await workers.repoInfo(ws.cwd).catch(() => null))?.branch ?? null;
  if (!head || !target) return "there is no branch to compare it with, so nothing was deleted";
  const ahead = await workers.commitsAhead(ws.cwd, target, head).catch(() => null);
  if (ahead === null) return `it could not be compared with ${target}, so nothing was deleted`;
  if (ahead > 0) return `${tree.branch ?? head} has ${ahead} commit(s) that ${target} does not`;
  return null;
}

/**
 * Stops a worker: its processes, its pane, its claim on the task, and its
 * worktree and branch when they hold nothing worth keeping. `by` is who
 * asked, for the line on screen: the orchestrator, or null for the person.
 */
export async function stopWorker(
  ws: Workspace,
  pane: Pane,
  options: { keepWorktree?: boolean },
  deps: ControlDeps,
  by: string | null,
): Promise<StoppedWorker> {
  const workers = workerDeps(deps);
  // Read while the worktree still exists: it names the branch.
  const trees = pane.cwd ? await deps.worktrees(ws.cwd).catch(() => [] as Worktree[]) : [];
  const tree = pane.cwd ? worktreeOf(trees, pane.cwd) : undefined;
  const path = tree && pane.cwd ? rootOf(tree, pane.cwd) : pane.cwd;

  // Its processes first: nothing writes in the worktree once it is looked at,
  // and a process still standing in it would keep it from being removed.
  await workers.killPane(pane.id).catch(() => undefined);
  deps.closePane(ws.id, pane.id);
  deps.touch(ws.id);

  const task = pane.taskId
    ? await workers.release(ws.cwd, pane.taskId, pane.name, false).catch(() => null)
    : null;

  const worktree: StoppedWorker["worktree"] = {
    path,
    branch: tree?.branch ?? null,
    removed: false,
    branchDeleted: false,
  };
  if (!path || !tree) {
    if (path) worktree.kept = "git does not list it as a worktree any more";
  } else if (tree.isMain || inside(tree.path, ws.cwd) !== null) {
    worktree.kept = "it is the workspace's own checkout, not one Hangar made for the worker";
  } else if (options.keepWorktree) {
    worktree.kept = "keepWorktree was asked for";
  } else {
    await deps.sleep(SETTLE_MS);
    const reason = await workKept(ws, path, tree, deps);
    if (reason) {
      worktree.kept = `${reason}. Merge or salvage it from there; the person can remove it from the worktree panel.`;
    } else {
      try {
        await removeWorktree(ws, path, deps);
        worktree.removed = true;
      } catch (err) {
        worktree.kept = `git could not remove it (${messageOf(err)}); the person can from the worktree panel`;
      }
      if (worktree.removed && tree.branch) {
        worktree.branchDeleted = await workers
          .branchDelete(ws.cwd, tree.branch, true)
          .then((result) => result.deleted)
          .catch(() => false);
      }
    }
  }

  deps.announce({ kind: "closed", by, pane: pane.name, workspace: ws.name });
  postOrchestratorEvent(ws.id, { kind: "workerStopped", worker: pane.name, taskId: pane.taskId ?? null });
  return {
    stopped: pane.name,
    task: task ? { id: task.id, title: task.title, column: task.column } : null,
    worktree,
  };
}

/** A worker of `ws` by pane name or id. */
function workerIn(ws: Workspace, ref: unknown): Pane {
  const wanted = text(ref);
  const workers = workersOf(ws);
  const names = workers.map((pane) => `"${pane.name}"`).join(", ") || "none";
  if (!wanted) throw new Error(`Name the worker to stop. Workers of "${ws.name}": ${names}.`);
  const matches = (pane: Pane) =>
    pane.id === wanted || pane.name.trim().toLowerCase() === wanted.toLowerCase();
  const found = workers.find(matches);
  if (found) return found;
  if (ws.panes.some(matches)) {
    throw new Error(`"${wanted}" is not a worker: worker_stop only stops workers. Workers of "${ws.name}": ${names}.`);
  }
  throw new Error(`No worker "${wanted}" in "${ws.name}". Workers: ${names}.`);
}

export async function workerControl(
  op: string,
  ws: Workspace,
  args: Record<string, unknown>,
  caller: Caller,
  deps: ControlDeps,
): Promise<unknown> {
  switch (op) {
    case "worker_spawn": {
      const me = orchestratorCalling(ws, caller, op);
      return spawnWorker(ws, { task: args.task, name: args.name, model: args.model }, deps, me);
    }
    case "worker_list":
      orchestratorCalling(ws, caller, op);
      return listWorkers(ws, deps);
    case "worker_stop": {
      const me = orchestratorCalling(ws, caller, op);
      const pane = workerIn(ws, args.worker);
      return stopWorker(ws, pane, { keepWorktree: args.keepWorktree === true }, deps, me.name);
    }
    default:
      throw new Error(`Unknown operation "${op}".`);
  }
}

/**
 * Stops every worker of a workspace: the person's « Arrêter les workers ».
 * Resolves the number stopped; one that fails does not keep the others going.
 */
export async function stopAllWorkers(ws: Workspace, deps: ControlDeps): Promise<number> {
  const current = deps.snapshot().workspaces.find((entry) => entry.id === ws.id) ?? ws;
  let stopped = 0;
  for (const pane of workersOf(current)) {
    const fresh = deps.snapshot().workspaces.find((entry) => entry.id === ws.id) ?? current;
    const ok = await stopWorker(fresh, pane, {}, deps, null)
      .then(() => true)
      .catch(() => false);
    if (ok) stopped++;
  }
  return stopped;
}
