"use strict";
// Shared by the tests: the recorded outputs, a fake window.hangar over a real data folder, a
// fake clock, a fake DOM, and the hook run as the process Claude Code starts.
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { spawn } = require("node:child_process");

const KIT = path.join(__dirname, "..");
const HOOK = path.join(KIT, "hooks", "record.js");

/** A recorded output, by file name: see fixtures/. */
const fixture = (name) => fs.readFileSync(path.join(__dirname, "fixtures", name), "utf8");

function tempFolder(t) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "tests-kit-"));
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
 * A window.hangar answering like Hangar for the permissions the tab holds. `files` is an
 * in-memory data folder; `panes` what panes.list answers. A listener registered with `on`
 * is called by `emit`.
 */
function fakeHangar({ files = {}, panes = [], denyRead = false, denyPanes = false, readError = null, sendError = null } = {}) {
  const calls = [];
  const listeners = {};
  return {
    calls,
    files,
    panes,
    callsTo: (method) => calls.filter((call) => call.method === method),
    on(event, handler) {
      (listeners[event] = listeners[event] || []).push(handler);
      return () => {
        listeners[event] = listeners[event].filter((item) => item !== handler);
      };
    },
    emit: (event, data) => (listeners[event] || []).forEach((handler) => handler({ data })),
    async call(method, args) {
      calls.push({ method, args });
      if (method === "tab.setTitle" || method === "tab.setBadge") return {};
      if (method === "data.read") {
        if (denyRead) throw fail("denied", "this tab was not granted data.read");
        if (readError) throw fail(readError);
        if (files[args.path] === undefined) throw fail("not_found");
        return { text: files[args.path] };
      }
      if (method === "panes.list") {
        if (denyPanes) throw fail("denied", "this tab was not granted panes.read");
        return this.panes;
      }
      if (method === "panes.send") {
        if (denyPanes === "send") throw fail("denied", "this tab was not granted panes.send");
        if (sendError) throw fail(sendError, `could not: ${sendError}`);
        return { queued: true, pane: args.pane };
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
  set textContent(value) {
    this.children = [String(value)];
  }
  get textContent() {
    return this.text;
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

/** What Claude Code hands a PostToolUse hook for a call that succeeded. */
const successEvent = (command, stdout, extra = {}) =>
  JSON.stringify({
    session_id: "session-1",
    cwd: extra.cwd || "/work/project",
    hook_event_name: "PostToolUse",
    tool_name: extra.tool || "Bash",
    tool_input: { command },
    tool_response: { stdout, stderr: "", interrupted: false, isImage: false },
  });

/** And for one that exited non-zero, which is how a failing test run arrives. */
const failureEvent = (command, output, extra = {}) =>
  JSON.stringify({
    session_id: "session-1",
    cwd: extra.cwd || "/work/project",
    hook_event_name: "PostToolUseFailure",
    tool_name: extra.tool || "Bash",
    tool_input: { command },
    error: `Exit code ${extra.exitCode || 1}\n${output}`,
    is_interrupt: false,
  });

module.exports = { KIT, HOOK, fixture, tempFolder, fakeClock, fakeHangar, fakeDocument, FakeNode, settle, runHook, successEvent, failureEvent, fail };
