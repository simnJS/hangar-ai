import { describe, expect, it } from "vitest";
import { askedQuestions, describeCall } from "./calls";

describe("a call, described before the transcript has it", () => {
  it("titles it as chat/tools.rs does, and as a permission asks it", () => {
    const cwd = "C:\\dev\\shop";
    const cases: [string, Record<string, unknown>, string, string, string | undefined][] = [
      ["Bash", { command: "pnpm test\npnpm build" }, "Ran pnpm test", "Run pnpm test", "pnpm test\npnpm build"],
      ["PowerShell", { command: "Get-ChildItem" }, "Ran Get-ChildItem", "Run Get-ChildItem", "Get-ChildItem"],
      ["Read", { file_path: "C:\\dev\\shop\\src\\a.ts" }, "Read src/a.ts", "Read src/a.ts", undefined],
      ["Write", { file_path: "D:\\other\\b.md", content: "# B" }, "Wrote D:/other/b.md", "Write D:/other/b.md", "# B"],
      [
        "MultiEdit",
        { file_path: "c:\\DEV\\shop\\a.ts", edits: [{ old_string: "x", new_string: "y" }, { old_string: "", new_string: "z\n" }] },
        "Edited a.ts",
        "Edit a.ts",
        "- x\n+ y\n\n+ z",
      ],
      ["NotebookEdit", { notebook_path: "C:\\dev\\shop\\n.ipynb", new_source: "print(1)" }, "Edited n.ipynb", "Edit n.ipynb", "print(1)"],
      [
        "Grep",
        { pattern: "TODO", path: "C:\\dev\\shop\\src", glob: "*.ts" },
        "Searched for TODO",
        "Search for TODO",
        "TODO\npath: src\nglob: *.ts",
      ],
      ["WebFetch", { url: "https://example.com", prompt: "Summary" }, "Fetched https://example.com", "Fetch https://example.com", "Summary"],
      ["WebSearch", { query: "tauri 2" }, "Searched the web for tauri 2", "Search the web for tauri 2", undefined],
      ["Agent", { description: "Find the bug", prompt: "Look" }, "Ran agent: Find the bug", "Run agent: Find the bug", "Look"],
      [
        "TodoWrite",
        { todos: [{ content: "A", status: "completed" }, { content: "B", status: "in_progress" }, { content: "C", status: "pending" }] },
        "Updated the to-do list",
        "Update the to-do list",
        "[x] A\n[~] B\n[ ] C",
      ],
      ["Skill", { skill: "simplify" }, "Used skill simplify", "Use skill simplify", undefined],
      ["mcp__hangar__pane_send", { pane: "Ava" }, "Used pane_send", "Use pane_send", '{"pane":"Ava"}'],
      ["Mystery", {}, "Used Mystery", "Use Mystery", undefined],
    ];
    for (const [tool, input, title, ask, detail] of cases) {
      expect(describeCall(tool, input, cwd), tool).toEqual({ title, ask, ...(detail === undefined ? {} : { detail }) });
    }
  });

  it("cuts a long title to a line, and a long detail to a few KB", () => {
    const command = `echo ${"x".repeat(100)}`;
    const described = describeCall("Bash", { command: `${command}\n${"y".repeat(5000)}` }, null);
    expect(described.title).toBe(`Ran ${command.slice(0, 80)}…`);
    expect(described.detail).toHaveLength(4096 + 1);
    expect(described.detail?.endsWith("…")).toBe(true);
  });

  it("reads AskUserQuestion's questions, whatever else they carry", () => {
    expect(
      askedQuestions({
        questions: [
          {
            question: "Which?",
            header: "Pick",
            multiSelect: true,
            options: [{ label: "A", description: "First", preview: "<p>" }, { label: "B" }],
          },
          null,
        ],
      }),
    ).toEqual([
      {
        question: "Which?",
        header: "Pick",
        multiSelect: true,
        options: [
          { label: "A", description: "First" },
          { label: "B", description: "" },
        ],
      },
      { question: "", header: "", multiSelect: false, options: [] },
    ]);
    expect(askedQuestions({})).toEqual([]);
    expect(askedQuestions(null)).toEqual([]);
  });
});
