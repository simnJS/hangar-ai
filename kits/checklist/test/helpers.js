"use strict";
// Shared by the tests: the libraries as the page gets them, a fake window.hangar over
// files and a data folder held in memory (no test touches a real file), a fake clock
// and a fake DOM.
const path = require("node:path");

const KIT = path.join(__dirname, "..");
const TAB = path.join(KIT, "tabs", "checklist");

const logic = require(path.join(TAB, "logic.js"));
const { createController } = require(path.join(TAB, "controller.js"));
const view = require(path.join(TAB, "view.js"));

const settle = async () => {
  for (let i = 0; i < 20; i++) await new Promise((resolve) => setImmediate(resolve));
};

const refusal = (code, message) => Object.assign(new Error(message || code), { code });

/** A clock whose timers only fire when the test says so. */
function fakeClock(start = Date.parse("2026-10-31T18:00:00Z")) {
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

/**
 * A window.hangar that answers as docs/kits.md says: `files` is the workspace (path → text),
 * `data` the kit's data folder; `deny` lists the methods answered `denied`; `fail` maps a
 * method to the error code it answers; `writeLimit` makes data.write answer rate_limited
 * past that many writes in a minute of the fake clock.
 */
function fakeHangar({ params = {}, caller = null, files = {}, data = {}, deny = [], fail = {}, now = () => 0, writeLimit = 10 } = {}) {
  const calls = [];
  const listeners = new Map();
  const writes = [];
  const hangar = {
    calls,
    files: new Map(Object.entries(files)),
    data: new Map(Object.entries(data)),
    callsTo: (method) => calls.filter((call) => call.method === method),
    emit: (event, payload) => (listeners.get(event) || []).forEach((fn) => fn(payload)),
    on(event, fn) {
      listeners.set(event, [...(listeners.get(event) || []), fn]);
      return () => listeners.set(event, (listeners.get(event) || []).filter((other) => other !== fn));
    },
    async call(method, args) {
      calls.push({ method, args });
      if (deny.includes(method)) throw refusal("denied", `this tab was not granted ${method}`);
      if (fail[method]) throw refusal(fail[method]);
      switch (method) {
        case "tab.params":
          return { params, caller };
        case "tab.setTitle":
        case "tab.setBadge":
          return {};
        case "files.read":
          if (!hangar.files.has(args.path)) throw refusal("not_found", `${args.path} was not found in the workspace folder`);
          return hangar.files.get(args.path).replace(/^﻿/, "");
        case "data.read":
          if (!hangar.data.has(args.path)) throw refusal("not_found");
          return { text: hangar.data.get(args.path) };
        case "data.write": {
          const recent = writes.filter((at) => now() - at < 60000);
          if (recent.length >= writeLimit) throw refusal("rate_limited");
          writes.push(now());
          hangar.data.set(args.path, args.text);
          return {};
        }
        case "panes.send":
          return { queued: true, pane: args.pane };
        case "notify":
          return null;
        default:
          throw refusal("unknown", `unknown ${method}`);
      }
    },
  };
  return hangar;
}

const t = (key, vars) => logic.t("en", key, vars);

const LIST = {
  title: "Login and logout",
  steps: [
    { text: "Open /login and enter a wrong password.", expect: "An error shows under the field." },
    { text: "Sign in with the test account.", expect: "The dashboard shows." },
    { text: "Press Sign out, then Back.", expect: "The login page shows." },
  ],
};
const LIST_PATH = "recette/login.json";

/** A controller already started on the list, and its fake Hangar and clock. */
async function opened(options = {}) {
  const clock = options.clock || fakeClock();
  const hangar =
    options.hangar ||
    fakeHangar({
      params: { path: LIST_PATH },
      caller: { pane: "Ava" },
      files: { [LIST_PATH]: JSON.stringify(LIST) },
      now: clock.now,
      ...options,
    });
  const changes = [];
  const controller = createController({
    hangar, logic, t, now: clock.now, setTimer: clock.setTimer, clearTimer: clock.clearTimer,
    onChange: (state) => changes.push(state.view),
  });
  await controller.start();
  await settle();
  return { hangar, controller, state: controller.state, clock, changes };
}

/** Just enough DOM for view.js: elements, text, children, properties. */
class FakeNode {
  constructor(tag) {
    this.tag = tag;
    this.className = "";
    this.children = [];
    this.attributes = {};
    this.style = {};
    this.value = "";
  }
  set textContent(text) {
    this.children = [String(text)];
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
    this.attributes[name] = String(value);
  }
  getAttribute(name) {
    return this.attributes[name] ?? null;
  }
  get text() {
    return this.children.map((child) => (typeof child === "string" ? child : child.text)).join(" ");
  }
  has(cls) {
    return this.className.split(/\s+/).includes(cls);
  }
  find(predicate, found = []) {
    if (predicate(this)) found.push(this);
    for (const child of this.children) if (typeof child !== "string") child.find(predicate, found);
    return found;
  }
}
const fakeDocument = { activeElement: null, createElement: (tag) => new FakeNode(tag) };

/** The view on a fake DOM, redrawn at every change of a controller. */
async function drawn(options = {}) {
  const lang = options.lang || "en";
  const root = new FakeNode("div");
  const ctx = {
    doc: fakeDocument,
    logic,
    t: (key, vars) => logic.t(lang, key, vars),
    actions: null,
    summary: () => "",
  };
  const clock = options.clock || fakeClock();
  const hangar =
    options.hangar ||
    fakeHangar({ params: { path: LIST_PATH }, caller: { pane: "Ava" }, files: { [LIST_PATH]: JSON.stringify(LIST) }, now: clock.now, ...options });
  const copied = [];
  ctx.copyButton = (button, source) => copied.push({ button, source });
  const controller = createController({
    hangar, logic, t: ctx.t, now: clock.now, setTimer: clock.setTimer, clearTimer: clock.clearTimer,
    onChange: (state) => view.render(root, state, ctx),
  });
  ctx.actions = controller;
  ctx.summary = () => controller.summary();
  await controller.start();
  await settle();
  return { root, controller, hangar, clock, copied, ctx };
}

module.exports = { KIT, TAB, logic, view, LIST, LIST_PATH, fakeClock, fakeHangar, fakeDocument, FakeNode, opened, drawn, settle, refusal, t };
