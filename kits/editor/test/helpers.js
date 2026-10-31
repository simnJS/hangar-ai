"use strict";
// Shared by the tests: the libraries as the page gets them, a fake window.hangar over
// files held in memory (no test touches a real file of anyone's), and a fake DOM.
const path = require("node:path");

const KIT = path.join(__dirname, "..");
const TAB = path.join(KIT, "tabs", "editor");

const logic = require(path.join(TAB, "logic.js"));
const dotenv = require(path.join(TAB, "dotenv.js"));
const validate = require(path.join(TAB, "validate.js"));
const diff = require(path.join(TAB, "diff.js"));
const gitignore = require(path.join(TAB, "gitignore.js"));
const answer = require(path.join(TAB, "answer.js"));
const { createController } = require(path.join(TAB, "controller.js"));
const lib = { logic, dotenv, validate, diff, gitignore, answer };

const settle = async () => {
  for (let i = 0; i < 20; i++) await new Promise((resolve) => setImmediate(resolve));
};

const refusal = (code, message) => Object.assign(new Error(message || code), { code });

/**
 * A window.hangar that answers as docs/kits.md says, over `files` (path → text):
 * `files.read` says not_found for what is absent and drops a BOM, `files.write` with
 * `expected` answers conflict when the file holds anything else or is gone, and
 * `deny` lists the methods answered `denied`. `sendError` makes panes.send refuse.
 */
function fakeHangar({ params = {}, caller = null, files = {}, deny = [], sendError = null, writeError = null, truncated = false } = {}) {
  const store = new Map(Object.entries(files));
  const calls = [];
  const listeners = new Map();
  const hangar = {
    calls,
    store,
    callsTo: (method) => calls.filter((call) => call.method === method),
    emit: (event, data) => (listeners.get(event) || []).forEach((fn) => fn(data)),
    on(event, fn) {
      listeners.set(event, [...(listeners.get(event) || []), fn]);
      return () => listeners.set(event, (listeners.get(event) || []).filter((other) => other !== fn));
    },
    /** What the next `files.write` finds on disk, as if a program had just written it. */
    sendError,
    writeError,
    async call(method, args) {
      calls.push({ method, args });
      if (deny.includes(method)) throw refusal("denied", `this tab was not granted ${method}`);
      switch (method) {
        case "tab.params":
          return { params, caller };
        case "tab.setTitle":
        case "tab.setBadge":
          return {};
        case "files.read":
          if (!store.has(args.path)) throw refusal("not_found", `${args.path} was not found in the workspace folder`);
          return store.get(args.path).replace(/^﻿/, "");
        case "files.list":
          return { files: [...store.keys()].map((file) => ({ path: file, size: store.get(file).length, modified: 0 })), truncated };
        case "files.write": {
          if (hangar.writeError) throw refusal(hangar.writeError);
          if (args.expected !== undefined) {
            const now = store.has(args.path) ? store.get(args.path).replace(/^﻿/, "") : null;
            if (now !== args.expected) throw refusal("conflict", "the file changed");
          }
          store.set(args.path, args.text);
          return {};
        }
        case "panes.send":
          if (hangar.sendError) throw refusal(hangar.sendError);
          return { queued: true, pane: args.pane };
        default:
          throw refusal("unknown", `unknown ${method}`);
      }
    },
  };
  return hangar;
}

/** A controller already started on these params, and its fake Hangar. */
async function opened(options = {}) {
  const hangar = options.hangar || fakeHangar(options);
  const changes = [];
  const controller = createController({ hangar, lib, lang: options.lang || "en", onChange: (state) => changes.push(state.view) });
  await controller.start();
  await settle();
  return { hangar, controller, state: controller.state, changes };
}

const row = (state, key) => state.form.rows.find((other) => other.key === key);

/** Just enough DOM for view.js: elements, text, children, properties. */
class FakeNode {
  constructor(tag) {
    this.tag = tag;
    this.className = "";
    this.children = [];
    this.attributes = {};
    this.style = {};
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
    return this.attributes[name] === undefined ? null : this.attributes[name];
  }
  focus() {
    fakeDocument.activeElement = this;
  }
  get text() {
    return this.children.map((child) => (typeof child === "string" ? child : child.text)).join("");
  }
  find(predicate, found = []) {
    if (predicate(this)) found.push(this);
    for (const child of this.children) if (typeof child !== "string") child.find(predicate, found);
    return found;
  }
  /** Every string this node and those below it hold or show, attributes and values included. */
  everything() {
    const own = [this.value, this.placeholder, this.title, ...Object.values(this.attributes)].filter((item) => typeof item === "string");
    return [...own, ...this.children.flatMap((child) => (typeof child === "string" ? [child] : child.everything()))];
  }
}
const fakeDocument = { createElement: (tag) => new FakeNode(tag), activeElement: null };

module.exports = { KIT, TAB, lib, logic, dotenv, validate, diff, gitignore, answer, createController, settle, refusal, fakeHangar, opened, row, FakeNode, fakeDocument };
