import { describe, expect, it } from "vitest";
import { parseUnifiedDiff, splitPath } from "./diff";

const SAMPLE = [
  "diff --git a/src/a.ts b/src/a.ts",
  "index 1111111..2222222 100644",
  "--- a/src/a.ts",
  "+++ b/src/a.ts",
  "@@ -3,4 +3,5 @@ export function a() {",
  " keep",
  "-old",
  "+new",
  "+added",
  " tail",
  "\\ No newline at end of file",
  "",
].join("\n");

describe("parseUnifiedDiff", () => {
  it("keeps the file header as meta even though it starts with - and +", () => {
    const rows = parseUnifiedDiff(SAMPLE);
    expect(rows.slice(0, 4).map((row) => row.kind)).toEqual(["meta", "meta", "meta", "meta"]);
    expect(rows[2].text).toBe("--- a/src/a.ts");
  });

  it("numbers both sides from the hunk header", () => {
    const rows = parseUnifiedDiff(SAMPLE).filter((row) => row.kind !== "meta");
    expect(rows.map((row) => [row.kind, row.text, row.oldLine, row.newLine])).toEqual([
      ["hunk", "@@ -3,4 +3,5 @@ export function a() {", undefined, undefined],
      ["ctx", "keep", 3, 3],
      ["del", "old", 4, undefined],
      ["add", "new", undefined, 4],
      ["add", "added", undefined, 5],
      ["ctx", "tail", 5, 6],
      ["note", "\\ No newline at end of file", undefined, undefined],
    ]);
  });

  it("goes back to headers at the next file and drops carriage returns", () => {
    const two = `${SAMPLE}diff --git a/b b/b\r\n--- a/b\r\n+++ b/b\r\n@@ -1 +1 @@\r\n-x\r\n+y\r\n`;
    const rows = parseUnifiedDiff(two);
    const second = rows.slice(rows.findIndex((row) => row.text === "diff --git a/b b/b"));
    expect(second.map((row) => row.kind)).toEqual(["meta", "meta", "meta", "hunk", "del", "add"]);
    expect(second[5].text).toBe("y");
  });

  it("reads an empty diff as no rows", () => {
    expect(parseUnifiedDiff("")).toEqual([]);
  });
});

describe("splitPath", () => {
  it("separates the folder from the file name", () => {
    expect(splitPath("src/lib/diff.ts")).toEqual({ dir: "src/lib/", name: "diff.ts" });
    expect(splitPath("README.md")).toEqual({ dir: "", name: "README.md" });
  });
});
