"use strict";
// Shared by the tests: a fake window.hangar over a real or an in-memory data folder,
// a fake clock, a fake DOM, and the hook run as the process Claude Code starts.
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { spawn } = require("node:child_process");

const KIT = path.join(__dirname, "..");
const HOOK = path.join(KIT, "hooks", "guard.js");

function tempFolder(t) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "guard-kit-"));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  return dir;
}

const fail = (code, message = code) => Object.assign(new Error(message), { code });

/** A clock whose timers only fire when the test says so. */
function fakeClock(start = Date.parse("2026-10-31T17:02:00Z")) {
  let now = start;
  let nextId = 1;
  const timers = new Map();
  return {
    now: () => now,
    setTimer(fn, ms) {
      const id = nextId++;
      timers.set(id, { fn, at: now + ms });
      return id;
    },
    clearTimer: (id) => timers.delete(id),
    pending: () => timers.size,
    async advance(ms) {
      const end = now + ms;
      for (;;) {
        await settle();
        const due = [...timers.entries()].filter(([, timer]) => timer.at <= end).sort((a, b) => a[1].at - b[1].at)[0];
        if (!due) break;
        timers.delete(due[0]);
        now = Math.max(now, due[1].at);
        due[1].fn();
      }
      now = end;
      await settle();
    },
  };
}

const settle = async () => {
  for (let i = 0; i < 20; i++) await new Promise((resolve) => setImmediate(resolve));
};

/**
 * A window.hangar answering like Hangar for the two permissions the tab holds.
 * `files` is an in-memory data folder, or `folder` a real one on disk.
 */
function fakeHangar({ files = {}, folder = null, denyRead = false, denyWrite = false, writeError = null, readError = null } = {}) {
  const calls = [];
  return {
    calls,
    files,
    callsTo: (method) => calls.filter((call) => call.method === method),
    on: () => () => undefined,
    async call(method, args) {
      calls.push({ method, args });
      if (method === "tab.setTitle" || method === "tab.setBadge") return {};
      if (method === "data.read") {
        if (denyRead) throw fail("denied", "this tab was not granted data.read");
        if (readError) throw fail(readError);
        if (folder) {
          try {
            return { text: fs.readFileSync(path.join(folder, args.path), "utf8") };
          } catch {
            throw fail("not_found");
          }
        }
        if (files[args.path] === undefined) throw fail("not_found");
        return { text: files[args.path] };
      }
      if (method === "data.write") {
        if (denyWrite) throw fail("denied", "this tab was not granted data.write");
        if (writeError) throw fail(writeError, `could not: ${writeError}`);
        if (folder) {
          const file = path.join(folder, args.path);
          fs.mkdirSync(path.dirname(file), { recursive: true });
          fs.writeFileSync(file, args.text);
        } else {
          files[args.path] = args.text;
        }
        return {};
      }
      throw fail("denied", `this tab was not granted what ${method} needs`);
    },
  };
}

/** Just enough DOM for view.js: elements, text, children, properties. */
class FakeNode {
  constructor(tag) {
    this.tag = tag;
    this.className = "";
    this.children = [];
    this.attributes = {};
  }
  set textContent(text) {
    this.children = [String(text)];
  }
  append(...nodes) {
    this.children.push(...nodes);
  }
  replaceChildren(...nodes) {
    this.children = nodes;
  }
  setAttribute(name, value) {
    this.attributes[name] = value;
  }
  get text() {
    return this.children.map((child) => (typeof child === "string" ? child : child.text)).join("");
  }
  find(predicate, found = []) {
    if (predicate(this)) found.push(this);
    for (const child of this.children) if (typeof child !== "string") child.find(predicate, found);
    return found;
  }
}
const fakeDocument = { createElement: (tag) => new FakeNode(tag) };

/** The hook, as Claude Code starts it. Resolves { code, stdout, stderr, ms }. */
function runHook(stdin, env = {}, script = HOOK) {
  return new Promise((resolve, reject) => {
    const started = Date.now();
    const base = { ...process.env };
    for (const name of ["HANGAR_API_URL", "HANGAR_API_TOKEN", "HANGAR_PANE_ID", "HANGAR_PANE_NAME", "HANGAR_KITS_DATA", "CLAUDE_PROJECT_DIR", "CLAUDE_PLUGIN_ROOT"]) delete base[name];
    const child = spawn(process.execPath, [script], { env: { ...base, ...env }, stdio: ["pipe", "pipe", "pipe"], windowsHide: true });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (chunk) => (stdout += chunk));
    child.stderr.on("data", (chunk) => (stderr += chunk));
    child.on("error", reject);
    child.on("close", (code) => resolve({ code, stdout, stderr, ms: Date.now() - started }));
    child.stdin.end(stdin);
  });
}

const bashEvent = (command, cwd, extra = {}) => JSON.stringify({
  session_id: "session-1",
  cwd,
  hook_event_name: "PreToolUse",
  tool_name: "Bash",
  tool_input: { command },
  ...extra,
});

const readLog = (dir, name = "refusals.jsonl") => {
  try {
    return fs.readFileSync(path.join(dir, name), "utf8").split("\n").filter(Boolean).map((line) => JSON.parse(line));
  } catch {
    return [];
  }
};

module.exports = { KIT, HOOK, tempFolder, fakeClock, fakeHangar, fakeDocument, FakeNode, settle, runHook, bashEvent, readLog, fail };
