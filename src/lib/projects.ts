import { getPaneActivity, type AgentActivity, type PaneActivity } from "./agentState";
import { boardLoad, type BoardColumn } from "./board";
import { gitRepoInfo } from "./git";
import { isAsleep } from "./hibernateStore";
import { memoryLoad, type MemoryEntry } from "./memory";
import { NEEDS_YOU_LABEL, READY_TO_LAND_LABEL, orchestratorOf, workersOf } from "./orchestration";
import { findPlan, planTitle } from "./orchestratorBrief";
import { describeEvent, orchestratorJournal, type BoardTask, type JournalEntry } from "./orchestratorEvents";
import type { AgentId, AppState, Workspace } from "../types";

/**
 * Where every project stands, for the manager: the one pane with role
 * "manager", in the HQ workspace, which follows every other workspace of
 * Hangar, tells the person what waits for them and passes their instructions
 * on to each project's orchestrator.
 *
 * One summary per workspace, the HQ left out — its orchestrator and workers,
 * the panes waiting for the person, its board, its plan's status, what
 * happened last — and the texts a model reads them in. Everything is read
 * through `ProjectsDeps`, so the tools, the briefing and the events all write
 * the same overview. A part that cannot be read (a board, the memory) is left
 * empty, and every other project is summarized all the same.
 */

export interface ProjectPane {
  name: string;
  /** null: no agent this window knows of runs in it. */
  activity: AgentActivity | null;
}

export interface ProjectWorker extends ProjectPane {
  taskId: string | null;
}

export interface ProjectTask {
  id: string;
  title: string;
}

export interface ProjectSummary {
  id: string;
  name: string;
  cwd: string;
  /** Its panes are up in a window. */
  open: boolean;
  asleep: boolean;
  /** Whether its folder is a git repository; null when that cannot be told. */
  git: boolean | null;
  orchestrator: ProjectPane | null;
  workers: ProjectWorker[];
  /** Agent panes held by a permission or a question, workers or not. */
  waiting: { name: string; agent: AgentId }[];
  board: Record<BoardColumn, number>;
  /** Why the board could not be read; its parts are all empty then. */
  boardError: string | null;
  /** Open tasks only: a done task waits for nobody, whatever its labels. */
  needsYou: ProjectTask[];
  readyToLand: ProjectTask[];
  doing: (ProjectTask & { assignee: string | null })[];
  /** The board has tasks, and every one of them is done. */
  allDone: boolean;
  /** The "Status:" part of the orchestrator's plan; null without one. */
  planStatus: string | null;
  /** The orchestrator's latest events, oldest first, as lines a model reads. */
  lastEvents: { at: number; text: string }[];
}

export interface ProjectsDeps {
  snapshot: () => AppState;
  loadBoard: (cwd: string) => Promise<BoardTask[]>;
  loadMemory: () => Promise<MemoryEntry[]>;
  /** Whether a folder is a git repository; null when that cannot be told. */
  isRepo: (cwd: string) => Promise<boolean | null>;
  activity: (paneId: string) => PaneActivity | undefined;
  asleep: (workspaceId: string) => boolean;
  /** Whether the workspace's panes are up in a window. */
  isOpen: (workspaceId: string) => boolean;
  journal: (workspaceId: string) => JournalEntry[];
}

/** The window's own sources; which workspaces are open is the app's to say. */
export const projectsDeps = (
  snapshot: () => AppState,
  isOpen: (workspaceId: string) => boolean,
): ProjectsDeps => ({
  snapshot,
  loadBoard: (cwd) => boardLoad(cwd).then((board) => (board.tasks ?? []) as BoardTask[]),
  loadMemory: () => memoryLoad().then((memory) => memory.entries ?? []),
  // Without git installed, nobody can tell.
  isRepo: (cwd) => gitRepoInfo(cwd).then((repo) => (repo.installed ? repo.isRepo : null)),
  activity: getPaneActivity,
  asleep: isAsleep,
  isOpen,
  journal: orchestratorJournal,
});

/* ── The plan's status ─────────────────────────────────────────── */

const MAX_STATUS = 300;

const STATUS_LINE = /^\s*(?:[-*]\s+)?(?:#{1,6}\s+)?\*{0,2}status\*{0,2}\s*:\s*\*{0,2}\s*(.*)$/i;
const STATUS_HEADING = /^\s*#{1,6}\s+status\s*$/i;
/** The other parts orchestratorRole has the plan written in. */
const PLAN_PART = /^\s*(?:objective|validation criteria|decisions|assumptions|open questions|tasks)\b[^:\n]{0,40}:/i;

/**
 * The status in a plan: what follows "Status:" on its line or, the line
 * holding nothing else, the lines under it up to a blank one or the next part.
 */
export function planStatusOf(content: string): string | null {
  const lines = content.split(/\r?\n/);
  for (let at = 0; at < lines.length; at += 1) {
    const inline = STATUS_LINE.exec(lines[at]);
    if (!inline && !STATUS_HEADING.test(lines[at])) continue;
    const first = inline?.[1].trim() ?? "";
    if (first) return flat(first, MAX_STATUS);
    const under: string[] = [];
    for (const line of lines.slice(at + 1)) {
      if (!line.trim() || /^\s*#/.test(line) || PLAN_PART.test(line)) break;
      under.push(line.trim());
    }
    return under.length ? flat(under.join("\n"), MAX_STATUS) : null;
  }
  return null;
}

/* ── Reading ───────────────────────────────────────────────────── */

const EVENTS_KEPT = 5;

/** Two workspaces on one folder share its board and its repository. */
const folderKey = (cwd: string) => cwd.replace(/[\\/]+$/, "").replace(/\\/g, "/").toLowerCase();

const message = (err: unknown) => (err instanceof Error ? err.message : String(err)).replace(/^Error: /, "");

const byRank = (a: BoardTask, b: BoardTask) => b.priority - a.priority || a.order - b.order;

interface BoardRead {
  tasks: BoardTask[];
  error: string | null;
}

/** Every workspace but the HQ, summarized, in the order the store keeps them. */
export async function loadProjects(deps: ProjectsDeps): Promise<ProjectSummary[]> {
  const state = deps.snapshot();
  const boards = new Map<string, Promise<BoardRead>>();
  const repos = new Map<string, Promise<boolean | null>>();
  const once = <T>(cache: Map<string, Promise<T>>, cwd: string, read: () => Promise<T>): Promise<T> => {
    const key = folderKey(cwd);
    const known = cache.get(key);
    if (known) return known;
    const reading = read();
    cache.set(key, reading);
    return reading;
  };
  // An unreadable memory is no plan anywhere, not a failed list.
  const memory = Promise.resolve()
    .then(() => deps.loadMemory())
    .catch((): MemoryEntry[] => []);
  return Promise.all(
    state.workspaces
      .filter((ws) => !ws.hq)
      .map(async (ws) => {
        const [board, git, entries] = await Promise.all([
          once(boards, ws.cwd, () =>
            Promise.resolve()
              .then(() => deps.loadBoard(ws.cwd))
              .then(
                (tasks): BoardRead => ({ tasks: Array.isArray(tasks) ? tasks : [], error: null }),
                (err): BoardRead => ({ tasks: [], error: message(err) || "unknown error" }),
              ),
          ),
          once(repos, ws.cwd, () =>
            Promise.resolve()
              .then(() => deps.isRepo(ws.cwd))
              .catch(() => null),
          ),
          memory,
        ]);
        return summarize(ws, state.workspaces, board, git, entries, deps);
      }),
  );
}

function summarize(
  ws: Workspace,
  workspaces: Workspace[],
  board: BoardRead,
  git: boolean | null,
  memory: MemoryEntry[],
  deps: ProjectsDeps,
): ProjectSummary {
  const { tasks } = board;
  const activityOf = (paneId: string) => deps.activity(paneId)?.activity ?? null;
  const orchestrator = orchestratorOf(ws);
  const count = (column: BoardColumn) => tasks.filter((task) => task.column === column).length;
  const labelled = (label: string) =>
    tasks
      .filter((task) => task.column !== "done" && task.labels.includes(label))
      .sort(byRank)
      .map((task) => ({ id: task.id, title: task.title }));
  const titles = (id: string) => tasks.find((task) => task.id === id)?.title ?? null;
  const plan = findPlan(memory, planTitle(ws, workspaces), ws);
  return {
    id: ws.id,
    name: ws.name,
    cwd: ws.cwd,
    open: deps.isOpen(ws.id),
    asleep: deps.asleep(ws.id),
    git,
    orchestrator: orchestrator ? { name: orchestrator.name, activity: activityOf(orchestrator.id) } : null,
    workers: workersOf(ws).map((pane) => ({
      name: pane.name,
      taskId: pane.taskId ?? null,
      activity: activityOf(pane.id),
    })),
    waiting: ws.panes
      .filter((pane) => pane.agent !== "shell" && activityOf(pane.id) === "waiting")
      .map((pane) => ({ name: pane.name, agent: pane.agent })),
    board: { todo: count("todo"), doing: count("doing"), review: count("review"), done: count("done") },
    boardError: board.error,
    needsYou: labelled(NEEDS_YOU_LABEL),
    readyToLand: labelled(READY_TO_LAND_LABEL),
    doing: tasks
      .filter((task) => task.column === "doing")
      .sort(byRank)
      .map((task) => ({ id: task.id, title: task.title, assignee: task.assignee })),
    allDone: tasks.length > 0 && tasks.every((task) => task.column === "done"),
    planStatus: plan ? planStatusOf(plan.content) : null,
    lastEvents: deps
      .journal(ws.id)
      .slice(-EVENTS_KEPT)
      .map((entry) => ({ at: entry.at, text: describeEvent(entry.event, titles) })),
  };
}

/* ── Texts ─────────────────────────────────────────────────────── */

/** About 300 characters a project, for twenty of them. */
const MAX_OVERVIEW = 6_000;
/** Kept free for the line that says which projects were left out. */
const FOOTER_ROOM = 400;
const MAX_DOING_IN_DETAIL = 25;

/**
 * How much of a project the overview says: the fullest of these forms that
 * every project fits in, or the last one, which counts tasks rather than
 * naming them.
 */
interface Form {
  /** Task titles listed in a line, the others counted; 0: counted only. */
  titles: number;
  title: number;
  status: number | null;
  /** Said under the summary, so that the reader knows what is left out. */
  note: string | null;
}
const FORMS: Form[] = [
  { titles: 3, title: 60, status: 160, note: null },
  { titles: 1, title: 40, status: null, note: "Short form, for room: one task title per list, no plan status." },
  {
    titles: 0,
    title: 0,
    status: null,
    note: "Shortest form, for room: tasks counted rather than named, no plan status.",
  },
];

const clip = (text: string, max: number) => {
  const trimmed = text.trim();
  return trimmed.length > max ? `${trimmed.slice(0, max - 1)}…` : trimmed;
};
const flat = (text: string, max: number) => clip(text.replace(/\s*\n\s*/g, " / "), max);
const sentence = (text: string) => `${text.charAt(0).toUpperCase()}${text.slice(1)}.`;

const pad = (n: number) => String(n).padStart(2, "0");
/** Local wall time, as the briefings write it. */
const clock = (at: number) => {
  const date = new Date(at);
  return `${pad(date.getHours())}:${pad(date.getMinutes())}`;
};

const ACTIVITY: Record<AgentActivity, string> = {
  working: "working",
  waiting: "waiting for the person",
  yours: "ended its turn",
  idle: "idle",
};
const doing = (activity: AgentActivity | null) => (activity ? ACTIVITY[activity] : "not running");

const ref = (task: ProjectTask, max: number) => `"${flat(task.title, max)}" [${task.id}]`;
const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

/** Something only the person can move: an answer, a landing, a pane held. */
const forThePerson = (project: ProjectSummary) =>
  project.needsYou.length > 0 || project.readyToLand.length > 0 || project.waiting.length > 0;

const atWork = (project: ProjectSummary) =>
  project.orchestrator?.activity === "working" ||
  project.workers.length > 0 ||
  project.board.doing > 0 ||
  project.board.review > 0;

const rank = (project: ProjectSummary) => (forThePerson(project) ? 0 : atWork(project) ? 1 : 2);

function headLine(project: ProjectSummary): string {
  const parts = [project.asleep ? "asleep" : project.open ? "open" : "not open"];
  if (project.git === true) parts.push("git repository");
  if (project.git === false) parts.push("no git repository");
  return `${project.name} — ${project.cwd} · ${parts.join(" · ")}`;
}

function boardState(project: ProjectSummary): string {
  if (project.boardError) return `could not be read (${flat(project.boardError, 120)})`;
  const { todo, doing: inDoing, review, done } = project.board;
  const total = todo + inDoing + review + done;
  if (!total) return "empty";
  if (project.allDone) return `all ${plural(total, "task")} done`;
  return `${todo} todo, ${inDoing} doing, ${review} review, ${done} done`;
}

const orchestratorState = (project: ProjectSummary) =>
  project.orchestrator
    ? `orchestrator "${project.orchestrator.name}" ${doing(project.orchestrator.activity)}`
    : "no orchestrator";

/** Who works on it and how far its board is, in one line. */
function teamState(project: ProjectSummary): string {
  const parts = [orchestratorState(project)];
  if (project.workers.length) {
    const tally = new Map<string, number>();
    for (const worker of project.workers) {
      const what = doing(worker.activity);
      tally.set(what, (tally.get(what) ?? 0) + 1);
    }
    const counts = [...tally].map(([what, n]) => `${n} ${what}`).join(", ");
    parts.push(`${plural(project.workers.length, "worker")}: ${counts}`);
  }
  parts.push(`board: ${boardState(project)}`);
  return parts.join(" · ");
}

function refs(tasks: ProjectTask[], form: Form): string {
  const listed = tasks.slice(0, form.titles).map((task) => ref(task, form.title));
  const more = tasks.length > form.titles ? `, and ${tasks.length - form.titles} more` : "";
  return `${listed.join(", ")}${more}`;
}

function waitingList(project: ProjectSummary, shown = project.waiting.length): string {
  const names = project.waiting.slice(0, shown).map((pane) => `"${pane.name}" (${pane.agent})`);
  const more = project.waiting.length > shown ? `, and ${project.waiting.length - shown} more` : "";
  return `${names.join(", ")}${more}`;
}

/** What waits for the person in a project, counted. */
function tally(project: ProjectSummary): string | null {
  const counts = [
    project.needsYou.length ? `${plural(project.needsYou.length, "task")} needing you` : null,
    project.readyToLand.length ? `${plural(project.readyToLand.length, "task")} ready to land` : null,
    project.waiting.length ? `${plural(project.waiting.length, "pane")} waiting` : null,
  ].filter(Boolean);
  return counts.length ? `For the person: ${counts.join(", ")}.` : null;
}

/** One project in a few lines; one with nothing to add, in one. */
function overviewBlock(project: ProjectSummary, form: Form): string {
  const more: string[] = [];
  if (!form.titles) {
    const counted = tally(project);
    if (counted) more.push(counted);
  } else {
    if (project.needsYou.length) more.push(`Needs you: ${refs(project.needsYou, form)}.`);
    if (project.readyToLand.length) more.push(`Ready to land: ${refs(project.readyToLand, form)}.`);
    if (project.waiting.length) more.push(`Waiting for the person: ${waitingList(project, form.titles + 2)}.`);
  }
  if (form.status && project.planStatus) more.push(`Plan status: ${flat(project.planStatus, form.status)}`);
  if (!more.length) return `${headLine(project)} · ${teamState(project)}.`;
  return [headLine(project), sentence(teamState(project)), ...more].join("\n  ");
}

/**
 * Every project in a few lines, those with something for the person first,
 * then those at work, then the quiet ones — the order in which they are left
 * out when even their shortest form does not fit in about 6,000 characters.
 */
export function renderOverview(projects: ProjectSummary[], now: number): string {
  const title = `# Hangar projects at ${clock(now)}`;
  if (!projects.length) return `${title}\nNo project: Hangar has no workspace besides the HQ.`;
  const ordered = projects
    .map((project, index) => ({ project, index }))
    .sort((a, b) => rank(a.project) - rank(b.project) || a.index - b.index)
    .map((entry) => entry.project);
  const person = ordered.filter(forThePerson);
  const working = ordered.filter((project) => !forThePerson(project) && atWork(project)).length;
  const names = person.length ? ` (${clip(person.map((project) => project.name).join(", "), 300)})` : "";
  const summary =
    `${plural(projects.length, "project")}: ${person.length} with something for the person${names}, ` +
    `${working} at work, ${projects.length - person.length - working} quiet.`;
  let head = "";
  let blocks: string[] = [];
  for (const form of FORMS) {
    head = [title, summary, form.note].filter(Boolean).join("\n");
    blocks = ordered.map((project) => overviewBlock(project, form));
    if (blocks.reduce((length, block) => length + 2 + block.length, head.length) <= MAX_OVERVIEW) break;
  }
  let text = head;
  let shown = 0;
  for (const [index, block] of blocks.entries()) {
    const room = MAX_OVERVIEW - (index < blocks.length - 1 ? FOOTER_ROOM : 0);
    if (text.length + 2 + block.length > room) break;
    text += `\n\n${block}`;
    shown += 1;
  }
  const left = ordered.slice(shown);
  if (left.length) {
    const held = left.filter(forThePerson).length;
    const lead =
      `…and ${plural(left.length, "more project")} not shown` +
      `${held ? `, ${held} of them with something for the person` : ""}: `;
    const rest = left.map((project) => project.name).join(", ");
    text += `\n\n${lead}${clip(rest, FOOTER_ROOM - 3 - lead.length)}.`;
  }
  return text;
}

/** One project in detail: its team, its open tasks, its plan's status, its latest events. */
export function renderProject(project: ProjectSummary): string {
  const lines = [headLine(project), sentence(orchestratorState(project))];
  if (project.workers.length) {
    const known = [...project.doing, ...project.readyToLand, ...project.needsYou];
    lines.push(`Workers (${project.workers.length}):`);
    for (const worker of project.workers) {
      const task = worker.taskId ? known.find((entry) => entry.id === worker.taskId) : undefined;
      const on = worker.taskId ? ` on ${task ? ref(task, 120) : `[${worker.taskId}]`}` : "";
      lines.push(`- "${worker.name}"${on}: ${doing(worker.activity)}`);
    }
  } else {
    lines.push("Workers: none running.");
  }
  lines.push(`Board: ${boardState(project)}.`);
  if (project.doing.length) {
    lines.push("Doing:");
    for (const task of project.doing.slice(0, MAX_DOING_IN_DETAIL)) {
      lines.push(`- ${ref(task, 120)}${task.assignee ? ` · ${task.assignee}` : ""}`);
    }
    if (project.doing.length > MAX_DOING_IN_DETAIL) {
      lines.push(`…and ${project.doing.length - MAX_DOING_IN_DETAIL} more in doing.`);
    }
  }
  if (project.needsYou.length) {
    lines.push("Needs you:", ...project.needsYou.map((task) => `- ${ref(task, 120)}`));
  }
  if (project.readyToLand.length) {
    lines.push("Ready to land:", ...project.readyToLand.map((task) => `- ${ref(task, 120)}`));
  }
  lines.push(
    project.waiting.length ? `Waiting for the person: ${waitingList(project)}.` : "No pane waits for the person.",
  );
  lines.push(project.planStatus ? `Plan status: ${project.planStatus}` : "Plan status: none found.");
  if (project.lastEvents.length) {
    lines.push(
      "Latest events, oldest first:",
      ...project.lastEvents.map((entry) => `- ${clock(entry.at)} ${entry.text}`),
    );
  }
  return lines.join("\n");
}
