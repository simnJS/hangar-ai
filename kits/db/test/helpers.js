"use strict";
// Shared by the tests: a fixture database built in a temp folder, a runner that answers
// `command.run` the way Hangar does (the manifest's argv, its patterns, no shell) over the real
// script, a fake window.hangar, and a fake DOM.
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { spawnSync } = require("node:child_process");

const KIT = path.join(__dirname, "..");
const manifest = JSON.parse(fs.readFileSync(path.join(KIT, "hangar-kit.json"), "utf8"));

// Strings planted in the fixture so a test can look for them where they must never be.
const SECRETS = { token: "tok_live_9f3a7c51d2e84b60", email: "ada.lovelace@example.invalid", hash: "pbkdf2$310000$b7e1a4c9" };

function tempWorkspace(t) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "db-kit-"));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 }));
  return fs.realpathSync(dir);
}

/** Builds the fixture database at `file` with node:sqlite, or the sqlite3 program when that is all there is. */
function buildFixture(file) {
  const longBio = "lorem ipsum ".repeat(500); // 6000 characters
  const statements = [
    "CREATE TABLE users (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, email TEXT, api_token TEXT DEFAULT 'none', password_hash TEXT, bio TEXT, avatar BLOB, score REAL, active INTEGER DEFAULT 1)",
    "CREATE INDEX users_name ON users(name)",
    "CREATE UNIQUE INDEX users_email ON users(email)",
    `INSERT INTO users (name, email, api_token, password_hash, bio, avatar, score, active) VALUES ('Ada', '${SECRETS.email}', '${SECRETS.token}', '${SECRETS.hash}', '${longBio}', x'89504e470d0a1a0a', 3.5, 1)`,
    "INSERT INTO users (name, email, bio, score, active) VALUES ('Bob', 'bob@example.invalid', NULL, -1.25, 0)",
    "INSERT INTO users (name, email, bio, score, active) VALUES ('Éloïse ‘quoted’', NULL, 'it''s a \"test\"', 9007199254740993, 1)",
    "CREATE TABLE events (id INTEGER PRIMARY KEY, kind TEXT, n INTEGER)",
    ...Array.from({ length: 120 }, (_, i) => `INSERT INTO events (kind, n) VALUES ('${["click", "view", "buy"][i % 3]}', ${(i * 37) % 101})`),
    "CREATE TABLE pairs (a TEXT, b TEXT, PRIMARY KEY (a, b)) WITHOUT ROWID",
    "INSERT INTO pairs VALUES ('x', '2'), ('x', '1'), ('a', '9')",
    "CREATE TABLE empty_one (id INTEGER)",
    'CREATE TABLE "odd name" (id INTEGER)',
    "CREATE VIEW active_users AS SELECT id, name FROM users WHERE active = 1",
  ];
  let sqlite = null;
  try {
    sqlite = require("node:sqlite");
  } catch {
    /* falls back below */
  }
  if (sqlite && sqlite.DatabaseSync) {
    const database = new sqlite.DatabaseSync(file);
    for (const sql of statements) database.exec(sql);
    database.close();
    return;
  }
  const answer = spawnSync("sqlite3", [file], { input: `${statements.join(";\n")};\n`, encoding: "utf8" });
  if (answer.error || answer.status !== 0) throw new Error("no way to build the fixture database: node:sqlite and sqlite3 are both missing");
}

/** A workspace folder holding the fixture at `app.db`. */
function workspaceWithDatabase(t) {
  const dir = tempWorkspace(t);
  buildFixture(path.join(dir, "app.db"));
  return dir;
}

const hasSqliteProgram = spawnSync("sqlite3", ["-version"]).status === 0;
/** Whether the fixture can be built here: node:sqlite or the sqlite3 program. */
const canBuildFixture = (() => {
  try {
    require("node:sqlite");
    return true;
  } catch {
    return hasSqliteProgram;
  }
})();

/**
 * What Hangar does for `command.run { name, args }`: the manifest's argv with {kitDir} and the
 * placeholders filled in, every value held to its pattern whole and refused when it starts with
 * "-", started without a shell in `cwd`. Answers `{ exitCode, stdout, stderr, timedOut }`.
 */
function runDeclared(name, args, cwd, env = {}) {
  const command = manifest.commands[name];
  if (!command) throw Object.assign(new Error("denied"), { code: "denied" });
  const argv = command.run.map((item) => {
    if (item === "{kitDir}/tools/db.js") return path.join(KIT, "tools", "db.js");
    const match = /^\{(\w+)\}$/.exec(item);
    if (!match) return item;
    const rule = command.args[match[1]];
    const value = args && args[match[1]];
    if (typeof value !== "string" || !new RegExp(`^(?:${rule.pattern})$`, "u").test(value) || value.startsWith("-")) {
      throw Object.assign(new Error(`bad_args: ${match[1]}`), { code: "bad_args" });
    }
    return value;
  });
  const [program, ...rest] = argv;
  const answer = spawnSync(program === "node" ? process.execPath : program, rest, { cwd, encoding: "utf8", env: { ...process.env, ...env }, windowsHide: true });
  return { exitCode: answer.status, stdout: answer.stdout, stderr: answer.stderr, timedOut: false, truncated: false };
}

/** The script, started like Hangar starts it, and its document. */
function db(cwd, name, args, env) {
  return JSON.parse(runDeclared(name, args, cwd, env).stdout);
}

const fail = (code, message = code) => Object.assign(new Error(message), { code });

const settle = async () => {
  for (let i = 0; i < 30; i++) await new Promise((resolve) => setImmediate(resolve));
};

/**
 * A window.hangar answering like Hangar for the permissions the tab holds. `files` is the kit's
 * data folder in memory; commands run the real script in `cwd` unless `commands` overrides one.
 */
function fakeHangar({ cwd, files = {}, panes = [], params = {}, deny = [], sendError = null, env = {}, commands = {} } = {}) {
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
      if (deny.includes(method) || (method === "command.run" && deny.includes(`command:${args.name}`))) throw fail("denied", `this tab was not granted what ${method} needs`);
      if (method === "tab.setTitle" || method === "tab.setBadge") return {};
      if (method === "tab.params") return { params, caller: null };
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
        if (commands[args.name]) return commands[args.name](args.args);
        return runDeclared(args.name, args.args, cwd, env);
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

module.exports = { KIT, manifest, SECRETS, tempWorkspace, buildFixture, workspaceWithDatabase, canBuildFixture, hasSqliteProgram, runDeclared, db, fakeHangar, fakeDocument, FakeNode, settle, fail };
