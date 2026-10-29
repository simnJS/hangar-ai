import type { Activity, HistoryEntry, Pane, Project, RemoteState, Share, Waiting, WaitingKind } from "./api";
import { dayHeading } from "./format";
import type { Locale } from "./i18n";

/**
 * What the screens show, read from the PC's state: what waits for the
 * person and where answering it leads, every agent by state, a pane found by
 * id or name.
 */

/** The states an agent can be in, the ones that need the person first. */
export const ACTIVITIES: Activity[] = ["waiting", "yours", "working", "idle"];

export interface ForYouGroup {
  kind: WaitingKind;
  items: Waiting[];
}

const KIND_ORDER: WaitingKind[] = ["pane", "needsYou", "readyToLand"];

/** What waits for the person, by kind, the longest waiting first in each. */
export function forYouGroups(waiting: Waiting[]): ForYouGroup[] {
  return KIND_ORDER.map((kind) => ({
    kind,
    items: waiting.filter((item) => item.kind === kind).sort((a, b) => a.since - b.since),
  })).filter((group) => group.items.length > 0);
}

/** A stable key for a waiting item, across polls. */
export function waitingKey(item: Waiting): string {
  return [item.kind, item.workspaceId, item.paneId ?? item.pane ?? "", item.taskId ?? ""].join("/");
}

export type Destination =
  | { to: "pane"; pane: string }
  | { to: "delivery"; task: string }
  | { to: "project"; projectId: string };

/**
 * Where a card's one action leads. A waiting pane opens that pane; a question
 * opens the pane to answer it in, the orchestrator's unless the PC names
 * another. A delivery ready to merge opens what it brings, to merge it or
 * send it back.
 */
export function destinationOf(item: Waiting, state: RemoteState): Destination {
  if (item.kind === "readyToLand") {
    return item.taskId ? { to: "delivery", task: item.taskId } : { to: "project", projectId: item.workspaceId };
  }
  const pane = item.paneId ?? item.pane;
  if (pane) return { to: "pane", pane };
  const project = state.projects.find((candidate) => candidate.id === item.workspaceId);
  const orchestrator = project && orchestratorPane(project);
  if (orchestrator) return { to: "pane", pane: orchestrator.id };
  return { to: "project", projectId: item.workspaceId };
}

export function orchestratorPane(project: Project): Pane | undefined {
  return (
    project.panes.find((pane) => pane.role === "orchestrator") ??
    (project.orchestrator ? project.panes.find((pane) => pane.name === project.orchestrator?.name) : undefined)
  );
}

export function workersOf(project: Project): Pane[] {
  return project.panes.filter((pane) => pane.role === "worker");
}

/** A pane by id, or by name (names are unique across Hangar), with its project. */
export function locatePane(state: RemoteState, idOrName: string): { project: Project; pane: Pane } | null {
  for (const project of state.projects) {
    const pane = project.panes.find((candidate) => candidate.id === idOrName);
    if (pane) return { project, pane };
  }
  for (const project of state.projects) {
    const pane = project.panes.find((candidate) => candidate.name === idOrName);
    if (pane) return { project, pane };
  }
  return null;
}

export interface FleetTile {
  id: string;
  name: string;
  activity: Activity;
}

export interface Fleet {
  /** Each project's agents, in the projects' order. */
  projects: { id: string; name: string; tiles: FleetTile[] }[];
  counts: Record<Activity, number>;
  total: number;
}

/** Every agent pane, by project, and how many are in each state. */
export function fleetOf(projects: Project[]): Fleet {
  const counts: Record<Activity, number> = { waiting: 0, yours: 0, working: 0, idle: 0 };
  const groups = projects
    .map((project) => ({
      id: project.id,
      name: project.name,
      tiles: project.panes.flatMap((pane) => (pane.activity ? [{ id: pane.id, name: pane.name, activity: pane.activity }] : [])),
    }))
    .filter((group) => group.tiles.length > 0);
  for (const tile of groups.flatMap((group) => group.tiles)) counts[tile.activity]++;
  return { projects: groups, counts, total: groups.reduce((sum, group) => sum + group.tiles.length, 0) };
}

/** Shares that arrived after `seenAt`. */
export function unseenShares(shares: Share[], seenAt: number | null): number {
  return seenAt === null ? shares.length : shares.filter((share) => share.at > seenAt).length;
}

/** Newest first, whatever order they came in, without duplicates. */
export function mergeShares(known: Share[], incoming: Share[]): Share[] {
  const byId = new Map(known.map((share) => [share.id, share]));
  for (const share of incoming) byId.set(share.id, share);
  return [...byId.values()].sort((a, b) => b.at - a.at);
}

export function isImage(share: Share): boolean {
  return /^image\/(png|jpeg|gif|webp|avif)$/i.test(share.mime);
}

// ESC [ … final byte, ESC ] … BEL or ESC \, and the lone two-byte escapes.
// eslint-disable-next-line no-control-regex
const ANSI = /\u001b\[[0-?]*[ -/]*[@-~]|\u001b\][^\u0007\u001b]*(?:\u0007|\u001b\\)|\u001b[@-Z\\-_]/g;

/**
 * A pane's screen as the phone shows it: no terminal escapes, no trailing
 * spaces, no blank lines below the last one written.
 */
export function cleanScreen(text: string): string {
  return text
    .replace(ANSI, "")
    .replace(/\r/g, "")
    .split("\n")
    .map((line) => line.replace(/\s+$/, ""))
    .join("\n")
    .replace(/\n+$/, "");
}

export type ScreenLine = { kind: "text"; text: string } | { kind: "rule" };

// A line drawn of box edges only: a frame's top or bottom, a separator.
const RULE = /^\s*[╭┌╰└├]?[─━═┄┈╌]{3,}[╮┐╯┘┤]?\s*$/;
// A line inside a frame: its two sides.
const FRAMED = /^\s*[│┃](.*?)[│┃]\s*$/;

/**
 * A screen as lines a phone can show: a terminal is wider than a phone, so
 * the frames Claude Code draws around a prompt would wrap into noise. Their
 * edges become rules, their sides go, and what they hold stays as written,
 * one space of padding less. Runs of rules count as one.
 */
export function screenLines(text: string): ScreenLine[] {
  const lines: ScreenLine[] = [];
  for (const line of cleanScreen(text).split("\n")) {
    if (RULE.test(line)) {
      if (lines[lines.length - 1]?.kind !== "rule") lines.push({ kind: "rule" });
      continue;
    }
    const framed = FRAMED.exec(line);
    lines.push({ kind: "text", text: framed ? framed[1].replace(/^ /, "").replace(/\s+$/, "") : line });
  }
  return lines;
}

/** The history under the day each entry happened, newest first. */
export function byDay(entries: HistoryEntry[], now: number, locale: Locale): { day: string; entries: HistoryEntry[] }[] {
  const days: { day: string; entries: HistoryEntry[] }[] = [];
  for (const entry of [...entries].sort((a, b) => b.at - a.at)) {
    const day = dayHeading(entry.at, now, locale);
    if (days[days.length - 1]?.day !== day) days.push({ day, entries: [] });
    days[days.length - 1].entries.push(entry);
  }
  return days;
}

/** The six digits of a pairing code, whatever else was typed or pasted. */
export const digitsOf = (value: string) => value.replace(/\D/g, "").slice(0, 6);
