"use strict";
// Shared by the tests: the libraries as the page gets them, a fake window.hangar over
// files held in memory (no test touches a real file of anyone's), the two browser-only
// dependencies faked, and a fake DOM with a canvas that records what is drawn on it.
const path = require("node:path");

const KIT = path.join(__dirname, "..");
const TAB = path.join(KIT, "tabs", "annotate");

const logic = require(path.join(TAB, "logic.js"));
const model = require(path.join(TAB, "model.js"));
const answer = require(path.join(TAB, "answer.js"));
const { createController } = require(path.join(TAB, "controller.js"));
const lib = { logic, model, answer };

const settle = async () => {
  for (let i = 0; i < 20; i++) await new Promise((resolve) => setImmediate(resolve));
};

const refusal = (code, message) => Object.assign(new Error(message || code), { code });

/** The first bytes of a PNG, and a few more: all `files.writeBinary` looks at. */
const PNG_BASE64 = Buffer.concat([Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), Buffer.from("body")]).toString("base64");

/**
 * A window.hangar that answers as docs/kits.md says: `files.readBinary` says not_found
 * for what is absent and unsupported for what is not an image, `files.writeBinary`
 * answers conflict where a file is, invalid for a name or content that is not a
 * raster image's, and `files.write` takes text. `deny` lists methods answered
 * `denied`; `fail` maps a method to the code it answers.
 */
function fakeHangar({ params = {}, caller = null, images = {}, texts = {}, deny = [], fail = {} } = {}) {
  const store = new Map(Object.entries(texts));
  const pictures = new Map(Object.entries(images));
  const written = new Map();
  const calls = [];
  const listeners = new Map();
  const hangar = {
    calls,
    store,
    written,
    callsTo: (method) => calls.filter((call) => call.method === method),
    emit: (event, data) => (listeners.get(event) || []).forEach((fn) => fn(data)),
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
        case "files.readBinary": {
          if (!pictures.has(args.path)) throw refusal("not_found", `${args.path} was not found`);
          return pictures.get(args.path);
        }
        case "files.writeBinary": {
          if (!/\.(png|jpe?g|webp)$/i.test(args.path)) throw refusal("invalid", "not a raster image's name");
          if (!Buffer.from(args.base64, "base64").subarray(0, 8).equals(Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]))) {
            throw refusal("invalid", "not a png");
          }
          if (written.has(args.path) || store.has(args.path)) throw refusal("conflict", "already exists");
          written.set(args.path, args.base64);
          return {};
        }
        case "files.write":
          store.set(args.path, args.text);
          return {};
        case "panes.send":
          return { queued: true, pane: args.pane };
        default:
          throw refusal("unknown", `unknown ${method}`);
      }
    },
  };
  return hangar;
}

/** What a browser would do, as a test can: sizes from a table, a PNG of the right shape. */
function fakeDeps({ sizes = {}, exportBase64 = PNG_BASE64, now = new Date(2026, 9, 31, 23, 15, 12) } = {}) {
  const exported = [];
  return {
    exported,
    decode: async (url) => {
      if (sizes[url]) return sizes[url];
      if (url.startsWith("data:image/")) return { width: 800, height: 600 };
      throw new Error("not an image");
    },
    readFile: async (file) => `data:${file.type};base64,${file.base64 || "AAAA"}`,
    exportPng: async (state, scale) => {
      exported.push({ scale, shapes: state.shapes.length });
      return typeof exportBase64 === "function" ? exportBase64(scale) : exportBase64;
    },
    now: () => now,
  };
}

/** A controller already started on these params, and its fake Hangar and browser. */
async function opened(options = {}) {
  const hangar = options.hangar || fakeHangar(options);
  const deps = options.deps || fakeDeps(options);
  const lang = options.lang || "en";
  const changes = [];
  const typing = [];
  const controller = createController({
    hangar,
    t: (key, vars) => logic.t(lang, key, vars),
    lib,
    deps,
    onChange: (state) => changes.push(state.phase),
    onTyping: (state) => typing.push(state.comment),
  });
  await controller.start();
  await settle();
  return { hangar, deps, controller, state: controller.state, changes, typing };
}

/** An image the workspace holds, as `files.readBinary` answers it. */
const PICTURE = { base64: PNG_BASE64, mime: "image/png" };

/** A pasted or dropped file, as the browser hands it over. */
const fileOf = (type = "image/png", name = "pasted.png", size = 1000) => ({ type, name, size, base64: PNG_BASE64 });

/** A recording 2D context: every call it gets, in order, and the text it was asked to measure. */
function recorder() {
  const log = [];
  const ctx = {
    log,
    measureText: (text) => ({ width: String(text).length * 6 }),
  };
  for (const name of ["setTransform", "clearRect", "drawImage", "beginPath", "moveTo", "lineTo", "arc", "stroke", "fill", "fillRect", "strokeRect", "fillText"]) {
    ctx[name] = (...args) => log.push([name, ...args]);
  }
  return ctx;
}

/** Just enough DOM for view.js: elements, text, children, properties, a canvas. */
class FakeNode {
  constructor(tag) {
    this.tag = tag;
    this.className = "";
    this.children = [];
    this.attributes = {};
    this.style = {};
    if (tag === "canvas") {
      this.context = recorder();
      this.getContext = () => this.context;
      this.getBoundingClientRect = () => ({ left: 0, top: 0, width: parseFloat(this.style.width) || 1, height: parseFloat(this.style.height) || 1 });
    }
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

module.exports = {
  KIT,
  TAB,
  lib,
  logic,
  model,
  answer,
  createController,
  settle,
  refusal,
  fakeHangar,
  fakeDeps,
  opened,
  recorder,
  PNG_BASE64,
  PICTURE,
  fileOf,
  FakeNode,
  fakeDocument,
};
