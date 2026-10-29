import type { ChatQuestion } from "./types";

/**
 * How a tool call reads on the phone before the transcript has it — Claude
 * Code writes a call there only once it has its result — from what the
 * hangar-bridge mod reports: a call running, a permission dialog open, a
 * question waiting. Titled as src-tauri/src/chat/tools.rs titles the calls of
 * the transcript ("Ran pnpm test"); a permission asks in the imperative
 * ("Run pnpm test").
 */

/** As chat/tools.rs: a detail past a few KB shows the gist, the PC has the rest. */
const CLIP_CHARS = 4 * 1024;
const TITLE_CHARS = 80;

export interface DescribedCall {
  /** As a done or running action: "Ran pnpm test". */
  title: string;
  /** As a permission asks it: "Run pnpm test". */
  ask: string;
  /** The input that matters: the command, the diff, the pattern. */
  detail?: string;
}

const clip = (text: string) => (text.length <= CLIP_CHARS ? text : `${text.slice(0, CLIP_CHARS)}…`);

/** The first line, cut to a title's length. */
function headline(text: string): string {
  const chars = [...(text.trim().split("\n")[0] ?? "").trim()];
  return chars.length > TITLE_CHARS ? `${chars.slice(0, TITLE_CHARS).join("")}…` : chars.join("");
}

/** Relative to the session's folder when it lies inside, with forward slashes either way. */
function shorten(path: string, cwd: string | null): string {
  const slashed = path.replace(/\\/g, "/");
  const root = (cwd ?? "").replace(/\\/g, "/").replace(/\/+$/, "");
  if (root && slashed.length > root.length + 1 && slashed[root.length] === "/") {
    // Windows paths come back with the drive letter cased either way.
    if (slashed.slice(0, root.length).toLowerCase() === root.toLowerCase()) return slashed.slice(root.length + 1);
  }
  return slashed;
}

const linesOf = (text: string) => (text ? text.replace(/\r?\n$/, "").split(/\r?\n/) : []);

/** `-`/`+` lines of an edit's old and new text. */
const diff = (old: string, next: string) =>
  [...linesOf(old).map((line) => `- ${line}`), ...linesOf(next).map((line) => `+ ${line}`)].join("\n").trimEnd();

export function describeCall(tool: string, input: unknown, cwd: string | null): DescribedCall {
  const args = (input !== null && typeof input === "object" ? input : {}) as Record<string, unknown>;
  const text = (key: string) => (typeof args[key] === "string" ? (args[key] as string) : undefined);
  const path = (key: string) => shorten(text(key) ?? "", cwd);
  const said = (title: string, ask: string, detail?: string) => {
    const kept = detail?.trim() ? clip(detail) : undefined;
    return kept ? { title, ask, detail: kept } : { title, ask };
  };
  switch (tool) {
    case "Bash":
    case "PowerShell": {
      const command = text("command") ?? "";
      return said(`Ran ${headline(command)}`, `Run ${headline(command)}`, command);
    }
    case "Read":
      return said(`Read ${path("file_path")}`, `Read ${path("file_path")}`);
    case "Edit":
      return said(`Edited ${path("file_path")}`, `Edit ${path("file_path")}`, diff(text("old_string") ?? "", text("new_string") ?? ""));
    case "MultiEdit": {
      const edits = Array.isArray(args.edits) ? (args.edits as Record<string, unknown>[]) : [];
      const detail = edits
        .map((edit) => diff(String(edit?.old_string ?? ""), String(edit?.new_string ?? "")))
        .join("\n\n");
      return said(`Edited ${path("file_path")}`, `Edit ${path("file_path")}`, detail);
    }
    case "Write":
      return said(`Wrote ${path("file_path")}`, `Write ${path("file_path")}`, text("content"));
    case "NotebookEdit":
      return said(`Edited ${path("notebook_path")}`, `Edit ${path("notebook_path")}`, text("new_source"));
    case "Grep":
    case "Glob": {
      const pattern = text("pattern") ?? "";
      const detail = [
        pattern,
        ...["path", "glob", "type"].flatMap((key) => {
          const value = text(key);
          return value === undefined ? [] : [`${key}: ${key === "path" ? shorten(value, cwd) : value}`];
        }),
      ].join("\n");
      return said(`Searched for ${headline(pattern)}`, `Search for ${headline(pattern)}`, detail);
    }
    case "WebFetch":
      return said(`Fetched ${headline(text("url") ?? "")}`, `Fetch ${headline(text("url") ?? "")}`, text("prompt"));
    case "WebSearch":
      return said(`Searched the web for ${headline(text("query") ?? "")}`, `Search the web for ${headline(text("query") ?? "")}`);
    case "Agent":
    case "Task": {
      const what = headline(text("description") ?? "subagent");
      return said(`Ran agent: ${what}`, `Run agent: ${what}`, text("prompt"));
    }
    case "TodoWrite": {
      const todos = Array.isArray(args.todos) ? (args.todos as Record<string, unknown>[]) : [];
      const marks: Record<string, string> = { completed: "[x]", in_progress: "[~]" };
      const detail = todos.map((todo) => `${marks[String(todo?.status)] ?? "[ ]"} ${String(todo?.content ?? "")}`).join("\n");
      return said("Updated the to-do list", "Update the to-do list", detail);
    }
    case "Skill":
      return said(`Used skill ${text("skill") ?? ""}`, `Use skill ${text("skill") ?? ""}`, text("args"));
    case "ToolSearch":
      return said("Loaded tools", "Load tools", text("query"));
    case "ExitPlanMode":
      return said("Presented a plan", "Present a plan", text("plan"));
    default: {
      // mcp__<server>__<tool>: the tool's own name says what it does.
      const name = /^mcp__.+?__(.+)$/.exec(tool)?.[1] ?? tool;
      const raw = Object.keys(args).length > 0 ? JSON.stringify(args) : undefined;
      return said(`Used ${name}`, `Use ${name}`, raw);
    }
  }
}

/** AskUserQuestion's questions, as its input lists them. */
export function askedQuestions(input: unknown): ChatQuestion[] {
  const list = (input as { questions?: unknown } | null)?.questions;
  if (!Array.isArray(list)) return [];
  const textOf = (value: unknown) => (typeof value === "string" ? value : "");
  return list.map((entry) => {
    const q = (entry ?? {}) as Record<string, unknown>;
    const options = Array.isArray(q.options) ? (q.options as Record<string, unknown>[]) : [];
    return {
      question: textOf(q.question),
      header: textOf(q.header),
      multiSelect: q.multiSelect === true,
      options: options.map((option) => ({ label: textOf(option?.label), description: textOf(option?.description) })),
    };
  });
}
