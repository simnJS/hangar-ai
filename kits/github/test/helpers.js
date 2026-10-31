"use strict";
// Shared by the tests: recorded `gh` output, a fake window.hangar, a fake clock, a fake DOM.
const fs = require("node:fs");
const path = require("node:path");

const KIT = path.join(__dirname, "..");
const fixture = (name) => fs.readFileSync(path.join(__dirname, "fixtures", name), "utf8");

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
    /** Moves the clock, firing due timers in order, letting promises settle between them. */
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
 * A window.hangar that answers like Hangar. `commands` maps a declared command's name
 * to a function (args) -> { exitCode, stdout, stderr } or a thrown { code }; `panes`
 * is what panes.list answers; `data` is the kit's data folder, in memory.
 */
function fakeHangar({ commands = {}, panes = [], data = {}, denySend = false, denyPanes = false, denyData = false } = {}) {
  const calls = [];
  const files = { ...data };
  const refuse = (code, message = code) => Object.assign(new Error(message), { code });
  return {
    calls,
    files,
    callsTo: (method) => calls.filter((call) => call.method === method),
    runs: (name) => calls.filter((call) => call.method === "command.run" && call.args.name === name),
    async call(method, args) {
      calls.push({ method, args });
      if (method === "tab.setBadge" || method === "tab.setTitle") return {};
      if (method === "panes.list") {
        if (denyPanes) throw refuse("denied");
        return panes;
      }
      if (method === "panes.send") {
        if (denySend) throw refuse("denied");
        return { queued: true, pane: args.pane };
      }
      if (method === "data.read") {
        if (denyData) throw refuse("denied");
        if (!(args.path in files)) throw refuse("not_found");
        return { text: files[args.path] };
      }
      if (method === "data.write") {
        if (denyData) throw refuse("denied");
        files[args.path] = args.text;
        return {};
      }
      if (method === "command.run") {
        const handler = commands[args.name];
        if (!handler) throw refuse("denied", `no such command ${args.name}`);
        const answer = await handler(args.args || {});
        return { exitCode: 0, stdout: "", stderr: "", timedOut: false, truncated: false, ...answer };
      }
      throw refuse("unknown", `unknown ${method}`);
    },
  };
}

const ok = (name) => ({ exitCode: 0, stdout: fixture(name) });
const failed = (name, exitCode = 1) => ({ exitCode, stderr: fixture(name) });

/** The commands of a repository with the recorded answers. */
const recorded = (overrides = {}) => ({
  "repo-info": () => ok("repo-info.json"),
  "pr-list": () => ok("pr-list.json"),
  "pr-view": () => ok("pr-view-failing.json"),
  "pr-threads": () => ok("pr-threads.json"),
  "issue-list": () => ok("issue-list.json"),
  "issue-view": () => ok("issue-view.json"),
  ...overrides,
});

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
const fakeDocument = { createElement: (tag) => new FakeNode(tag), activeElement: null, getElementById: () => null };

module.exports = { KIT, fixture, fakeClock, fakeHangar, fakeDocument, FakeNode, settle, ok, failed, recorded };
