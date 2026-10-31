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
 * A window.hangar that answers like Hangar: `commands` maps a declared command's
 * name to a function (args) → { exitCode, stdout, stderr } or a thrown { code }.
 */
function fakeHangar({ params = {}, commands = {}, denyNotify = false } = {}) {
  const calls = [];
  const listeners = new Map();
  return {
    calls,
    callsTo: (method) => calls.filter((call) => call.method === method),
    emit: (event, data) => (listeners.get(event) || []).forEach((fn) => fn(data)),
    on(event, fn) {
      listeners.set(event, [...(listeners.get(event) || []), fn]);
      return () => listeners.set(event, (listeners.get(event) || []).filter((other) => other !== fn));
    },
    async call(method, args) {
      calls.push({ method, args });
      if (method === "tab.params") return { params };
      if (method === "tab.setTitle" || method === "tab.setBadge") return {};
      if (method === "notify") {
        if (denyNotify) throw Object.assign(new Error("not granted"), { code: "denied" });
        return null;
      }
      if (method === "command.run") {
        const handler = commands[args.name];
        if (!handler) throw Object.assign(new Error(`no such command ${args.name}`), { code: "denied" });
        const answer = await handler(args.args || {});
        return { exitCode: 0, stdout: "", stderr: "", timedOut: false, truncated: false, ...answer };
      }
      throw Object.assign(new Error(`unknown ${method}`), { code: "unknown" });
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

module.exports = { KIT, fixture, fakeClock, fakeHangar, fakeDocument, FakeNode, settle };
