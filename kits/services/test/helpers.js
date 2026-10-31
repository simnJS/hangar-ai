"use strict";
// Shared by the tests: recorded output, a fake window.hangar, a fake clock, a fake DOM.
const fs = require("node:fs");
const path = require("node:path");

const KIT = path.join(__dirname, "..");
const TAB = path.join(KIT, "tabs", "services");
const fixture = (name) => fs.readFileSync(path.join(__dirname, "fixtures", name), "utf8");

const settle = async () => {
  for (let i = 0; i < 20; i++) await new Promise((resolve) => setImmediate(resolve));
};

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

const refusal = (code, message) => Object.assign(new Error(message || code), { code });

/**
 * A window.hangar answering as docs/kits.md says. `commands` maps a declared command's
 * name to a function (args) → { exitCode, stdout, stderr } or a thrown refusal; `panes`
 * is what panes.list gives, `deny` the methods answered `denied`.
 */
function fakeHangar({ commands = {}, panes = [], deny = [], sendError = null } = {}) {
  const calls = [];
  const listeners = new Map();
  const hangar = {
    calls,
    callsTo: (method) => calls.filter((call) => call.method === method),
    emit: (event, data) => (listeners.get(event) || []).forEach((fn) => fn(data)),
    on(event, fn) {
      listeners.set(event, [...(listeners.get(event) || []), fn]);
      return () => listeners.set(event, (listeners.get(event) || []).filter((other) => other !== fn));
    },
    panes,
    async call(method, args) {
      calls.push({ method, args });
      if (deny.includes(method)) throw refusal("denied", `this tab was not granted ${method}`);
      if (method === "panes.list") return hangar.panes;
      if (method === "panes.send") {
        if (sendError) throw refusal(sendError, "x");
        if (!hangar.panes.some((pane) => pane.name.toLowerCase() === String(args.pane).toLowerCase())) throw refusal("not_found", "no such pane");
        return { queued: true, pane: args.pane };
      }
      if (method === "command.run") {
        const handler = commands[args.name];
        if (!handler) throw refusal("denied", `no such command ${args.name}`);
        const answer = await handler(args.args || {});
        return { exitCode: 0, stdout: "", stderr: "", timedOut: false, truncated: false, ...answer };
      }
      throw refusal("unknown", `unknown ${method}`);
    },
  };
  return hangar;
}

/** The words of the real hangar.ui, a trace of the rest. The real library has a browser to run in. */
function fakeUi(lang = "en") {
  const ui = {
    lang,
    locale: () => ui.lang,
    i18n: (dictionaries) => (key, vars) => {
      const table = dictionaries[ui.lang] || dictionaries.en;
      const template = table[key] !== undefined ? table[key] : dictionaries.en[key] !== undefined ? dictionaries.en[key] : key;
      return template.replace(/\{(\w+)\}/g, (whole, name) => (vars && vars[name] !== undefined ? String(vars[name]) : whole));
    },
    format: {
      since: (at, now) => `<${Math.round((now - at) / 60000)} min ago>`,
      clock: () => "<clock>",
    },
    log(root, options) {
      root.logged = options;
    },
  };
  return ui;
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
    this.attributes[name] = String(value);
  }
  get text() {
    return this.children.map((child) => (typeof child === "string" ? child : child.text)).join("");
  }
  all(name) {
    return this.find((node) => node.className.split(/\s+/).includes(name));
  }
  find(predicate, found = []) {
    if (predicate(this)) found.push(this);
    for (const child of this.children) if (typeof child !== "string") child.find(predicate, found);
    return found;
  }
  /** Every string this node and those below it hold or show, attributes and titles included. */
  everything() {
    const own = [this.title, this.value, ...Object.values(this.attributes)].filter((item) => typeof item === "string");
    return [...own, ...this.children.flatMap((child) => (typeof child === "string" ? [child] : child.everything()))];
  }
}
const fakeDocument = { createElement: (tag) => new FakeNode(tag) };

module.exports = { KIT, TAB, fixture, settle, fakeClock, fakeHangar, fakeUi, fakeDocument, FakeNode, refusal };
