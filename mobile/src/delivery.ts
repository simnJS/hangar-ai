import { parseUnifiedDiff, splitPath, type DiffRow } from "../lib/diff";
import {
  ApiError,
  type Client,
  type Delivery,
  type DeliveryCheck,
  type DeliveryFile,
  type DeliveryReview,
  type DeliveryStage,
} from "./api";
import type { Key, Translator } from "./i18n";

/**
 * What the Delivery screen shows of a delivery that waits to land, read from
 * what the PC says of it: the check and the review in a line each, the files
 * with their counts, what stands in the way of « Fusionner », and the landing
 * from the tap to its outcome.
 */

const NBSP = " ";

/** A length of time from the check's seconds: 48 s, 1 min 36 s, 1 h 05, a number never apart from its unit. */
export function seconds(total: number): string {
  const whole = Math.max(0, Math.round(total));
  if (whole < 60) return `${whole}${NBSP}s`;
  const minutes = Math.floor(whole / 60);
  if (minutes < 60) return whole % 60 ? `${minutes}${NBSP}min ${whole % 60}${NBSP}s` : `${minutes}${NBSP}min`;
  return `${Math.floor(minutes / 60)}${NBSP}h ${String(minutes % 60).padStart(2, "0")}`;
}

export type Tone = "ok" | "failed" | "none";

export interface Verdict {
  tone: Tone;
  title: string;
  /** The command, for the check. */
  detail: string | null;
}

export function checkVerdict(check: DeliveryCheck | null, { t }: Translator): Verdict {
  if (!check) return { tone: "none", title: t("delivery.check.none"), detail: null };
  if (check.passed) return { tone: "ok", title: t("delivery.check.passed", { duration: seconds(check.seconds) }), detail: check.command };
  return { tone: "failed", title: t("delivery.check.failed"), detail: check.command };
}

export function reviewVerdict(review: DeliveryReview | null, { t }: Translator): Verdict & { notes: string | null } {
  if (!review) return { tone: "none", title: t("delivery.review.none"), detail: null, notes: null };
  const approved = review.approved && review.blocking.length === 0;
  return {
    tone: approved ? "ok" : "failed",
    title: t(approved ? "delivery.review.approved" : "delivery.review.changes"),
    detail: null,
    notes: review.notes.trim() || null,
  };
}

const OUTCOMES: Record<string, Key> = {
  checkFailed: "delivery.outcome.checkFailed",
  changesRequested: "delivery.outcome.changesRequested",
  conflict: "delivery.outcome.conflict",
  dirty: "delivery.outcome.dirty",
  error: "delivery.outcome.error",
};

/** What a delivery that did not pass ran into, in a few words. */
export const outcomeLabel = (outcome: string, { t }: Translator) => t(OUTCOMES[outcome] ?? "delivery.outcome.other");

export const stageText = (stage: DeliveryStage | null, { t }: Translator) => t(`delivery.stage.${stage ?? "queued"}`);

export interface FileRow {
  file: DeliveryFile;
  dir: string;
  name: string;
  /** "A", "M", "D", "R": the letter beside the name. */
  mark: string;
  label: string;
  /** "+24 −3"; null for a binary file. */
  counts: string | null;
}

const MARKS: Record<DeliveryFile["status"], string> = { added: "A", modified: "M", deleted: "D", renamed: "R" };

export function fileRows(files: DeliveryFile[], { t }: Translator): FileRow[] {
  return files.map((file) => ({
    file,
    ...splitPath(file.path),
    mark: MARKS[file.status],
    label: t(`delivery.status.${file.status}`),
    counts: file.binary ? null : `+${file.additions} −${file.deletions}`,
  }));
}

/** The lines added and removed, all files together. */
export function totals(files: DeliveryFile[]): { additions: number; deletions: number } {
  return files.reduce(
    (sum, file) => ({ additions: sum.additions + file.additions, deletions: sum.deletions + file.deletions }),
    { additions: 0, deletions: 0 },
  );
}

/**
 * A patch as the diff screen draws it: its hunks and their lines. git's
 * header (diff --git, index, ---, +++) says again what the screen's title does.
 */
export function patchLines(patch: string): DiffRow[] {
  return parseUnifiedDiff(patch).filter((row) => row.kind !== "meta");
}

/** Why « Fusionner » cannot be pressed now, if it cannot. */
export function landBlocked(delivery: Delivery): "moved" | "running" | null {
  if (!delivery.waiting) return "running";
  if (delivery.moved) return "moved";
  return null;
}

/* ── The landing ───────────────────────────────────────────────── */

export type LandStatus =
  | { kind: "idle" }
  | { kind: "landing"; stage: DeliveryStage | null }
  | { kind: "landed"; target: string }
  /** The PC said why it did not land, in its words. */
  | { kind: "refused"; reason: string }
  /** The PC stopped answering: the landing may go on there. */
  | { kind: "lost"; error: unknown };

/** Between two asks while a landing tested again runs. */
export const LAND_POLL_MS = 1_500;

/**
 * Lands the delivery shown, asking again while a landing tested again runs
 * on the PC — the same call waits for that landing there, never starts a
 * second — until it lands or is refused. Stops asking once `stopped`.
 */
export async function landUntilDone(
  client: Pick<Client, "land">,
  task: string,
  merge: string | null,
  {
    onStage,
    stopped = () => false,
    sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms)),
  }: { onStage?: (stage: DeliveryStage | null) => void; stopped?: () => boolean; sleep?: (ms: number) => Promise<void> } = {},
): Promise<LandStatus> {
  for (;;) {
    let answer;
    try {
      answer = await client.land(task, merge);
    } catch (error) {
      if (error instanceof ApiError && error.kind === "refused") return { kind: "refused", reason: error.detail ?? error.code };
      return { kind: "lost", error };
    }
    if (answer.landed) return { kind: "landed", target: answer.target };
    onStage?.(answer.stage);
    if (stopped()) return { kind: "landing", stage: answer.stage };
    await sleep(LAND_POLL_MS);
    if (stopped()) return { kind: "landing", stage: answer.stage };
  }
}

/** What the person writes to send it back: something to fix, not blanks. */
export const reworkText = (text: string) => text.trim();

/* ── What the phone last read of each delivery ──────────────────── */

/** For the diff screen's title: the file's status and counts, without asking the PC again. */
const known = new Map<string, Delivery>();

export function remember(delivery: Delivery) {
  known.set(delivery.taskId, delivery);
}

export function rememberedFile(task: string, path: string): DeliveryFile | null {
  return known.get(task)?.files.find((file) => file.path === path) ?? null;
}
