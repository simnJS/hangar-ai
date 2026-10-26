import { describe, expect, it } from "vitest";
import {
  diffSegments,
  diffTotals,
  editedRows,
  folderRows,
  normalizePath,
  relativeTo,
  splitPath,
  statusLetter,
} from "./inspectorFiles";
import type { ChangedFile, Changes } from "./git";

const file = (path: string, patch: Partial<ChangedFile> = {}): ChangedFile => ({
  path,
  oldPath: null,
  status: "M",
  additions: 1,
  deletions: 0,
  binary: false,
  ...patch,
});

const changes = (files: ChangedFile[], root = "C:/dev/hangar-ai"): Changes => ({
  root,
  base: "abc1234",
  files,
  truncated: false,
});

describe("paths", () => {
  it("normalises separators and trailing slashes", () => {
    expect(normalizePath("C:\\dev\\hangar-ai\\")).toBe("C:/dev/hangar-ai");
    expect(normalizePath("/home//you/x/")).toBe("/home/you/x");
    expect(normalizePath("/")).toBe("/");
  });

  it("finds a path inside a root, ignoring case only on Windows", () => {
    expect(relativeTo("c:\\Dev\\hangar-ai\\src\\a.ts", "C:/dev/hangar-ai")).toBe("src/a.ts");
    expect(relativeTo("C:/dev/hangar-ai", "C:\\dev\\hangar-ai\\")).toBe("");
    expect(relativeTo("C:/dev/hangar-ai-2/a.ts", "C:/dev/hangar-ai")).toBeNull();
    expect(relativeTo("/home/You/a.ts", "/home/you")).toBeNull();
    expect(relativeTo("/home/you/a.ts", "/home/you")).toBe("a.ts");
  });

  it("splits the directory from the name", () => {
    expect(splitPath("src/lib/a.ts")).toEqual({ dir: "src/lib/", name: "a.ts" });
    expect(splitPath("README.md")).toEqual({ dir: "", name: "README.md" });
  });

  it("reads git's letters the way a person would", () => {
    expect(statusLetter("?")).toBe("A");
    expect(statusLetter("T")).toBe("M");
    expect(statusLetter("R")).toBe("R");
    expect(statusLetter("D")).toBe("D");
  });
});

describe("editedRows", () => {
  it("enriches what the agent edited with git's counts", () => {
    const rows = editedRows(
      ["C:\\dev\\hangar-ai\\src\\store.tsx", "C:\\dev\\hangar-ai\\src\\new.ts"],
      changes([
        file("src/store.tsx", { additions: 3, deletions: 1 }),
        file("src/new.ts", { status: "?", additions: 64, deletions: 0 }),
        file("src/untouched.ts"),
      ]),
    );
    expect(rows.map((r) => [r.path, r.status, r.additions, r.deletions])).toEqual([
      ["src/store.tsx", "M", 3, 1],
      ["src/new.ts", "A", 64, 0],
    ]);
    expect(rows[0].dir).toBe("src/");
    expect(rows[0].name).toBe("store.tsx");
  });

  it("keeps a file git no longer lists, without counts", () => {
    const [row] = editedRows(["C:/dev/hangar-ai/done.ts"], changes([]));
    expect(row).toMatchObject({ path: "done.ts", status: null, additions: null, settled: true });
  });

  it("works without git, and outside the repository, calling nothing settled", () => {
    const [bare] = editedRows(["C:\\elsewhere\\a.ts"], null);
    expect(bare).toMatchObject({ path: "C:/elsewhere/a.ts", settled: false });
    const [outside] = editedRows(["C:/elsewhere/a.ts"], changes([]));
    expect(outside).toMatchObject({ path: "C:/elsewhere/a.ts", settled: false });
  });

  it("falls back to the pane's folder when git cannot place the file", () => {
    const [row] = editedRows(["C:\\dev\\app\\src\\a.ts"], null, "C:\\dev\\app");
    expect(row).toMatchObject({ path: "src/a.ts", dir: "src/", name: "a.ts", settled: false });
  });

  it("lists a file once, however it was spelled", () => {
    const rows = editedRows(["C:/dev/hangar-ai/a.ts", "c:\\DEV\\hangar-ai\\a.ts"], changes([]));
    expect(rows).toHaveLength(1);
  });
});

describe("folderRows", () => {
  const all = changes([
    file("README.md"),
    ...Array.from({ length: 10 }, (_, i) => file(`app/f${i}.ts`)),
  ]);

  it("keeps the changes under the pane's folder, capped", () => {
    const { rows, more } = folderRows(all, "C:\\dev\\hangar-ai\\app", 8);
    expect(rows).toHaveLength(8);
    expect(rows[0].path).toBe("app/f0.ts");
    expect(more).toBe(2);
  });

  it("takes everything at the root, and nothing outside it", () => {
    expect(folderRows(all, "C:/dev/hangar-ai", 20).rows).toHaveLength(11);
    expect(folderRows(all, "D:/other").rows).toHaveLength(0);
  });
});

describe("diff summary", () => {
  it("adds up the known counts", () => {
    const rows = editedRows(
      ["C:/dev/hangar-ai/a.ts", "C:/dev/hangar-ai/b.ts", "C:/dev/hangar-ai/c.png"],
      changes([
        file("a.ts", { additions: 3, deletions: 1 }),
        file("b.ts", { additions: 64, deletions: 0 }),
        file("c.png", { additions: null, deletions: null, binary: true }),
      ]),
    );
    expect(diffTotals(rows)).toEqual({ additions: 67, deletions: 1 });
  });

  it("lights one square per line, up to five, shared by their weight", () => {
    expect(diffSegments(0, 0)).toEqual(["none", "none", "none", "none", "none"]);
    expect(diffSegments(1, 0)).toEqual(["add", "none", "none", "none", "none"]);
    expect(diffSegments(3, 1)).toEqual(["add", "add", "add", "del", "none"]);
    expect(diffSegments(64, 0)).toEqual(["add", "add", "add", "add", "add"]);
    expect(diffSegments(0, 9)).toEqual(["del", "del", "del", "del", "del"]);
  });

  it("never lets a small side vanish", () => {
    expect(diffSegments(100, 1)).toEqual(["add", "add", "add", "add", "del"]);
    expect(diffSegments(1, 100)).toEqual(["add", "del", "del", "del", "del"]);
  });
});
