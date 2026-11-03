/**
 * A unified diff, as `git diff` prints it, cut into rows the diff view can
 * colour and number. Read-only: nothing here writes a patch back.
 */

export type DiffRowKind = "meta" | "hunk" | "add" | "del" | "ctx" | "note";

export interface DiffRow {
  kind: DiffRowKind;
  text: string;
  /** Line number on the old side; absent on additions and outside hunks. */
  oldLine?: number;
  /** Line number on the new side; absent on deletions and outside hunks. */
  newLine?: number;
}

const HUNK = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/;

export function parseUnifiedDiff(text: string): DiffRow[] {
  const rows: DiffRow[] = [];
  let oldLine = 0;
  let newLine = 0;
  // Header lines ("--- a/x", "+++ b/x") look like a deletion and an addition;
  // only a hunk header says the body has started.
  let inHunk = false;

  const lines = text.split("\n");
  // The newline that ends the last line is not a line of its own.
  if (lines.length > 0 && lines[lines.length - 1] === "") lines.pop();

  for (const raw of lines) {
    const line = raw.endsWith("\r") ? raw.slice(0, -1) : raw;
    const hunk = HUNK.exec(line);
    if (hunk) {
      oldLine = Number(hunk[1]);
      newLine = Number(hunk[2]);
      inHunk = true;
      rows.push({ kind: "hunk", text: line });
      continue;
    }
    if (line.startsWith("diff --git ")) inHunk = false;
    if (!inHunk) {
      rows.push({ kind: "meta", text: line });
      continue;
    }
    switch (line[0]) {
      case "+":
        rows.push({ kind: "add", text: line.slice(1), newLine: newLine++ });
        break;
      case "-":
        rows.push({ kind: "del", text: line.slice(1), oldLine: oldLine++ });
        break;
      case "\\":
        rows.push({ kind: "note", text: line });
        break;
      default:
        rows.push({
          kind: "ctx",
          text: line.slice(1),
          oldLine: oldLine++,
          newLine: newLine++,
        });
    }
  }
  return rows;
}

/** "src/lib/" and "diff.ts": the folder reads dimmer than the file name. */
export function splitPath(path: string): { dir: string; name: string } {
  const cut = path.lastIndexOf("/");
  return cut === -1
    ? { dir: "", name: path }
    : { dir: path.slice(0, cut + 1), name: path.slice(cut + 1) };
}
