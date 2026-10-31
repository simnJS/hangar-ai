"use strict";
// Shared by the tests: the recorded outputs, a fake window.hangar over an in-memory data
// folder, a fake clock, and a fake DOM.
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const KIT = path.join(__dirname, "..");

/** A recorded output, by file name: see fixtures/. */
const fixture = (name) => fs.readFileSync(path.join(__dirname, "fixtures", name), "utf8");

function tempFolder(t) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "todo-kit-"));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  return dir;
}

const fail = (code, message = code) => Object.assign(new Error(message), { code });

const settle = async () => {
  for (let i = 0; i < 20; i++) await new Promise((resolve) => setImmediate(resolve));
};

/**
 * A window.hangar answering like Hangar for the permissions the tab holds. `files` is the kit's
 * data folder in memory; `scan` is what `command.run` answers for the "scan" command (a
 * function of the call number, or a value); `panes` what panes.list answers.
 */
function fakeHangar({ files = {}, panes = [], scan = null, deny = [], sendError = null, openError = null } = {}) {
  const calls = [];
  const listeners = {};
  const api = {
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
      if (deny.includes(method)) throw fail("denied", `this tab was not granted what ${method} needs`);
      if (method === "tab.setTitle" || method === "tab.setBadge") return {};
      if (method === "data.read") {
        if (files[args.path] === undefined) throw fail("not_found");
        return { text: files[args.path] };
      }
      if (method === "data.write") {
        files[args.path] = args.text;
        return {};
      }
      if (method === "panes.list") return api.panes;
      if (method === "panes.send") {
        if (sendError) throw fail(sendError, `could not: ${sendError}`);
        return { queued: true, pane: args.pane };
      }
      if (method === "command.run") {
        const answer = typeof scan === "function" ? await scan(args, api.callsTo("command.run").length) : scan;
        if (answer instanceof Error) throw answer;
        return answer;
      }
      if (method === "preview.show") {
        if (openError) throw fail(openError, `could not: ${openError}`);
        return { opened: true, path: args.path };
      }
      throw fail("denied", `this tab was not granted what ${method} needs`);
    },
  };
  return api;
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

/** What command.run answers when scan.js printed `document`. */
const printed = (document, extra = {}) => ({ exitCode: 0, stdout: `${JSON.stringify(document)}
`, stderr: "", timedOut: false, truncated: false, ...extra });

module.exports = { KIT, fixture, tempFolder, fakeHangar, fakeDocument, FakeNode, settle, fail, printed };
