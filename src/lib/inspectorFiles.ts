import type { ChangedFile, Changes } from "./git";

/**
 * The inspector's list of changed files, worked out from two sources that
 * speak different dialects: the paths the agent reported editing (absolute,
 * in whatever separators its platform used) and git's view of the working
 * tree (relative to the repository root, forward slashes, with line counts).
 * Pure, so the matching rules can be tested without a repository.
 */

export interface FileRow {
  /** Stable across refreshes: the normalised path the row stands for. */
  key: string;
  /** What the row reads, relative to the repository when the file is in it. */
  path: string;
  /** The directory part, slash included; empty for a file at the top. */
  dir: string;
  name: string;
  /** M, A, D, R… as the inspector shows it; null when git has nothing on it. */
  status: string | null;
  additions: number | null;
  deletions: number | null;
  binary: boolean;
  /**
   * git read the repository and has nothing pending for this file: it was
   * committed since, or put back. Never set when git could not be asked.
   */
  settled: boolean;
}

export type DiffSegment = "add" | "del" | "none";

/** Forward slashes, no doubled or trailing separator. */
export function normalizePath(path: string): string {
  const slashed = path.replace(/\\/g, "/").replace(/\/{2,}/g, "/");
  return slashed.length > 1 ? slashed.replace(/\/$/, "") : slashed;
}

/**
 * Windows paths compare without regard to case — `C:/Dev` and `c:/dev` are
 * one folder — and git on Windows may report either spelling.
 */
const comparable = (path: string) => (/^[a-z]:\//i.test(path) ? path.toLowerCase() : path);

/** `path` relative to `root`, or null when it is not inside it. */
export function relativeTo(path: string, root: string): string | null {
  const p = normalizePath(path);
  const r = normalizePath(root);
  const cp = comparable(p);
  const cr = comparable(r);
  if (cp === cr) return "";
  const prefix = cr.endsWith("/") ? cr : `${cr}/`;
  return cp.startsWith(prefix) ? p.slice(prefix.length) : null;
}

export function splitPath(path: string): { dir: string; name: string } {
  const at = path.lastIndexOf("/");
  if (at < 0) return { dir: "", name: path };
  return { dir: path.slice(0, at + 1), name: path.slice(at + 1) };
}

/**
 * git's letter as a person reads it: an untracked file is a new one, a type
 * change is a modification. Renames, copies and conflicts keep their letter.
 */
export function statusLetter(status: string): string {
  if (status === "?") return "A";
  if (status === "T") return "M";
  return status.slice(0, 1).toUpperCase() || "M";
}

function rowFromChange(file: ChangedFile, path = file.path): FileRow {
  return {
    key: comparable(normalizePath(path)),
    path,
    ...splitPath(path),
    status: statusLetter(file.status),
    additions: file.additions,
    deletions: file.deletions,
    binary: file.binary,
    settled: false,
  };
}

/**
 * The files the agent edited, newest first, each with what git says about it
 * when it knows. A file git no longer lists — committed since, or put back as
 * it was — stays in the list without counts: the agent did touch it. Paths
 * read relative to the repository, or to `folder` (the pane's) when git
 * could not say where the repository is.
 */
export function editedRows(
  edited: string[],
  changes: Changes | null,
  folder: string | null = null,
): FileRow[] {
  const byPath = new Map<string, ChangedFile>();
  if (changes) {
    for (const file of changes.files) byPath.set(comparable(normalizePath(file.path)), file);
  }
  const seen = new Set<string>();
  const rows: FileRow[] = [];
  for (const raw of edited) {
    const absolute = normalizePath(raw);
    const inRepo = changes ? relativeTo(absolute, changes.root) : null;
    const inFolder = folder ? relativeTo(absolute, folder) : null;
    const path = inRepo || inFolder || absolute;
    const key = comparable(path);
    if (seen.has(key)) continue;
    seen.add(key);
    const known = byPath.get(key);
    rows.push(
      known
        ? rowFromChange(known, path)
        : {
            key,
            path,
            ...splitPath(path),
            status: null,
            additions: null,
            deletions: null,
            binary: false,
            settled: inRepo !== null,
          },
    );
  }
  return rows;
}

/**
 * What changed under the pane's own folder, for a pane whose agent reported
 * nothing: the first `limit`, and how many more there are.
 */
export function folderRows(
  changes: Changes,
  folder: string,
  limit = 8,
): { rows: FileRow[]; more: number } {
  const within = relativeTo(folder, changes.root);
  // A folder outside the repository it was asked about has nothing of it.
  if (within === null) return { rows: [], more: 0 };
  const prefix = within ? comparable(`${within}/`) : "";
  const mine = changes.files.filter((file) =>
    comparable(normalizePath(file.path)).startsWith(prefix),
  );
  return {
    rows: mine.slice(0, limit).map((file) => rowFromChange(file, normalizePath(file.path))),
    more: Math.max(0, mine.length - limit),
  };
}

export function diffTotals(rows: FileRow[]): { additions: number; deletions: number } {
  let additions = 0;
  let deletions = 0;
  for (const row of rows) {
    additions += row.additions ?? 0;
    deletions += row.deletions ?? 0;
  }
  return { additions, deletions };
}

/**
 * GitHub's five squares: as many lit as lines changed, up to five, split
 * between additions and deletions by their share. Either side that changed at
 * all keeps one square, so a one-line deletion in a big rewrite still shows.
 */
export function diffSegments(additions: number, deletions: number, segments = 5): DiffSegment[] {
  const a = Math.max(0, additions);
  const d = Math.max(0, deletions);
  const total = a + d;
  const lit = Math.min(segments, total);
  let adds = total ? Math.round((lit * a) / total) : 0;
  if (a > 0 && adds === 0) adds = 1;
  if (d > 0 && adds === lit && lit > 0) adds = lit - 1;
  const dels = lit - adds;
  return Array.from({ length: segments }, (_, i): DiffSegment =>
    i < adds ? "add" : i < adds + dels ? "del" : "none",
  );
}
