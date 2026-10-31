"use strict";
const test = require("node:test");
const assert = require("node:assert/strict");
const { SECRETS, workspaceWithDatabase, tempWorkspace, canBuildFixture, fakeHangar, fakeDocument, settle } = require("./helpers");
const logic = require("../tabs/db/logic");
const { createController } = require("../tabs/db/controller");
const { render } = require("../tabs/db/view");

const skip = canBuildFixture ? false : "neither node:sqlite nor the sqlite3 program is here to build the fixture";
const agents = [
  { name: "Jack", agent: "claude", role: "orchestrator", state: "idle", canSend: true },
  { name: "Jude", agent: "claude", role: "worker", state: "idle", canSend: true },
  { name: "Shell", agent: "shell", role: "worker", state: "idle", canSend: false },
];

function open(hangar, lang = "en") {
  const drawn = [];
  const controller = createController({ hangar, logic, lang, onChange: (state) => drawn.push(JSON.stringify(state)) });
  return { controller, drawn };
}

function draw(controller, lang = "en") {
  const calls = [];
  const proxy = new Proxy({}, { get: (_, name) => (...args) => calls.push([name, ...args]) });
  const container = fakeDocument.createElement("div");
  render(container, controller.state, { doc: fakeDocument, logic, lang, actions: proxy });
  return { container, calls, text: container.text };
}

const buttons = (view, label) => view.container.find((node) => node.tag === "button" && node.text === label);

test("opens the only database found, lists its tables, and remembers it", { skip }, async (t) => {
  const dir = workspaceWithDatabase(t);
  const hangar = fakeHangar({ cwd: dir, panes: agents });
  const { controller } = open(hangar);
  await controller.start();
  const { state } = controller;
  assert.equal(state.view, "ready");
  assert.equal(state.path, "app.db");
  assert.equal(state.db.phase, "ready");
  assert.equal(state.db.tables.find((item) => item.name === "events").rows, 120);
  assert.equal(hangar.files["state.json"], undefined, "opening what was found is not a choice to remember");
  const view = draw(controller);
  assert.match(view.text, /Tables/);
  assert.match(view.text, /events/);
  assert.match(view.text, /120 rows/);
  assert.match(view.text, /Read-only/);
  assert.match(view.text, /Values may be secrets/);
});

test("an agent's params { path } open that database, and a new ask replaces it", { skip }, async (t) => {
  const dir = workspaceWithDatabase(t);
  const { copyFileSync } = require("node:fs");
  copyFileSync(`${dir}/app.db`, `${dir}/other.sqlite`);
  const hangar = fakeHangar({ cwd: dir, params: { path: "other.sqlite" } });
  const { controller } = open(hangar);
  await controller.start();
  assert.equal(controller.state.path, "other.sqlite");
  assert.equal(controller.state.files.length, 2);
  hangar.emit("tab.params", { params: { path: "app.db" }, caller: { pane: "Jack" } });
  await settle();
  assert.equal(controller.state.path, "app.db");
  assert.deepEqual(JSON.parse(hangar.files["state.json"]), { path: "app.db" });
});

test("a remembered database that is gone falls back to the first found", { skip }, async (t) => {
  const dir = workspaceWithDatabase(t);
  const hangar = fakeHangar({ cwd: dir, files: { "state.json": JSON.stringify({ path: "gone.db" }) } });
  const { controller } = open(hangar);
  await controller.start();
  assert.equal(controller.state.path, "app.db");
});

test("a table: its structure, a page of rows, sorting that cycles, paging, a long value on demand", { skip }, async (t) => {
  const dir = workspaceWithDatabase(t);
  const hangar = fakeHangar({ cwd: dir });
  const { controller } = open(hangar);
  await controller.start();
  await controller.selectTable("events");
  let { detail } = controller.state;
  assert.equal(detail.phase, "ready");
  assert.deepEqual(detail.schema.columns.map((column) => column.name), ["id", "kind", "n"]);
  assert.equal(detail.page.rows.length, 50);
  assert.equal(detail.page.total, 120);
  assert.match(draw(controller).text, /1–50 of 120/);

  await controller.sortBy(2);
  assert.deepEqual([controller.state.sort, controller.state.direction], [2, "asc"]);
  await controller.sortBy(2);
  assert.deepEqual([controller.state.sort, controller.state.direction], [2, "desc"]);
  const rows = controller.state.detail.page.rows.map((row) => row[1].value);
  assert.deepEqual(rows, [...rows].sort().reverse());
  await controller.sortBy(2);
  assert.equal(controller.state.sort, 0);

  await controller.goTo(100);
  assert.equal(controller.state.detail.page.rows.length, 20);
  assert.match(draw(controller).text, /101–120 of 120/);
  assert.equal(buttons(draw(controller), "Next")[0].disabled, true);
  await controller.goTo(0);

  await controller.selectTable("users");
  const view = draw(controller);
  assert.match(view.text, /…/);
  assert.ok(view.container.find((node) => node.tag === "button" && /^Show all/.test(node.text)).length, "a cut value offers the whole one");
  await controller.toggleFull(0, 5);
  assert.equal(controller.state.full["0:5"].value.length, 6000);
  assert.ok(hangar.callsTo("command.run").some((entry) => entry.args.args && entry.args.args.limit === "1" && entry.args.args.chars === "100000"));
  assert.ok(draw(controller).container.find((node) => node.tag === "button" && node.text === "Show less").length);
  await controller.toggleFull(0, 5);
  assert.equal(controller.state.full["0:5"], undefined);
});

test("long values, blobs and nulls are told apart on screen", { skip }, async (t) => {
  const dir = workspaceWithDatabase(t);
  const { controller } = open(fakeHangar({ cwd: dir }));
  await controller.start();
  await controller.selectTable("users");
  const text = draw(controller).text;
  assert.match(text, /8 bytes \(binary\)/);
  assert.match(text, /NULL/);
  assert.match(text, /Éloïse ‘quoted’/);
  const french = draw(controller, "fr").text;
  assert.match(french, /8 octets \(binaire\)/);
  assert.match(french, /Lecture seule/);
  assert.match(french, /Confier à un agent/);
});

test("the markup of a value is text, never parsed", { skip }, async (t) => {
  const dir = workspaceWithDatabase(t);
  const hangar = fakeHangar({
    cwd: dir,
    commands: {
      rows: () => ({ exitCode: 0, stderr: "", stdout: JSON.stringify({ ok: true, table: "users", columns: [{ name: "<img src=x onerror=alert(1)>", type: "", primaryKey: false }], rows: [[{ type: "text", value: "<script>alert(1)</script>", length: 25, cut: false }]], offset: 0, total: 1, sort: 0, direction: "asc", capped: false }) }),
    },
  });
  const { controller } = open(hangar);
  await controller.start();
  await controller.selectTable("users");
  const view = draw(controller);
  assert.ok(view.text.includes("<script>alert(1)</script>"));
  assert.equal(view.container.find((node) => node.tag === "script" || node.tag === "img").length, 0);
});

test("« Confier à un agent » sends the table and the question, and no value of any row", { skip }, async (t) => {
  const dir = workspaceWithDatabase(t);
  const hangar = fakeHangar({ cwd: dir, panes: agents });
  const { controller } = open(hangar);
  await controller.start();
  await controller.selectTable("users");
  assert.equal(hangar.callsTo("panes.send").length, 0, "nothing is sent by opening, picking, sorting or reading");
  await controller.sortBy(2);
  await controller.toggleFull(0, 5);
  assert.equal(hangar.callsTo("panes.send").length, 0);
  controller.openHandOver();
  controller.setQuestion("Why is api_token filled for only one user?");
  await controller.sendHandOver();
  const sent = hangar.callsTo("panes.send");
  assert.equal(sent.length, 1);
  assert.equal(sent[0].args.pane, "Jack", "the orchestrator by default");
  const message = sent[0].args.text;
  assert.match(message, /Database file: app\.db/);
  assert.match(message, /Table: users \(3 rows\)/);
  assert.match(message, /Columns: id INTEGER \(primary key\), name TEXT, email TEXT, api_token TEXT, password_hash TEXT, bio TEXT, avatar BLOB, score REAL, active INTEGER/);
  assert.match(message, /Question: Why is api_token filled for only one user\?/);
  for (const secret of [...Object.values(SECRETS), "Bob", "Ada", "lorem ipsum", "bob@example.invalid", "Éloïse"]) assert.ok(!message.includes(secret), `${secret} did not go to the agent`);
  assert.deepEqual(controller.state.handOver.sent, { agent: "Jack" });
  assert.match(draw(controller).text, /Handed to Jack/);
});

test("the question is required, the agent can be picked, and a refused send says why", { skip }, async (t) => {
  const dir = workspaceWithDatabase(t);
  const hangar = fakeHangar({ cwd: dir, panes: agents, sendError: "unavailable" });
  const { controller } = open(hangar);
  await controller.start();
  await controller.selectTable("events");
  controller.openHandOver();
  await controller.sendHandOver();
  assert.equal(hangar.callsTo("panes.send").length, 0, "no question, no send");
  controller.choose("Jude");
  controller.setQuestion("Which kind is rarest?");
  await controller.sendHandOver();
  assert.equal(hangar.callsTo("panes.send")[0].args.pane, "Jude");
  assert.equal(controller.state.handOver.problem.code, "unavailable");
  assert.match(draw(controller).text, /Jude cannot receive text now/);
  assert.equal(controller.state.handOver.question, "Which kind is rarest?", "the question is kept for another try");
});

test("designed states: no database", async (t) => {
  const { controller } = open(fakeHangar({ cwd: tempWorkspace(t) }));
  await controller.start();
  assert.equal(controller.state.view, "ready");
  const text = draw(controller).text;
  assert.match(text, /No database found/);
  assert.match(draw(controller, "fr").text, /Aucune base de données trouvée/);
});

test("designed states: denied", { skip }, async (t) => {
  const dir = workspaceWithDatabase(t);
  const { controller } = open(fakeHangar({ cwd: dir, deny: ["command:list"] }));
  await controller.start();
  assert.equal(controller.state.view, "denied");
  assert.match(draw(controller).text, /may not read the database/);
  assert.match(draw(controller, "fr").text, /Réglages › Kits/);
  const reading = open(fakeHangar({ cwd: dir, deny: ["command:tables"] }));
  await reading.controller.start();
  assert.equal(reading.controller.state.view, "denied");
});

test("designed states: no sqlite", { skip }, async (t) => {
  const dir = workspaceWithDatabase(t);
  const { controller } = open(fakeHangar({ cwd: dir, env: { HANGAR_DB_DRIVER: "none" } }));
  await controller.start();
  assert.equal(controller.state.view, "nosqlite");
  assert.match(draw(controller).text, /No SQLite reader on this computer/);
  assert.match(draw(controller, "fr").text, /Aucun lecteur SQLite/);
});

test("designed states: locked, then readable once the writer lets go", { skip: !canBuildFixture || typeof require("node:sqlite") === "undefined" ? "node:sqlite is not here" : false }, async (t) => {
  const dir = workspaceWithDatabase(t);
  const { DatabaseSync } = require("node:sqlite");
  const writer = new DatabaseSync(`${dir}/app.db`);
  writer.exec("BEGIN EXCLUSIVE");
  const { controller } = open(fakeHangar({ cwd: dir }));
  try {
    await controller.start();
    assert.equal(controller.state.db.phase, "locked");
    assert.match(draw(controller).text, /The database is locked/);
    writer.exec("ROLLBACK");
    await controller.refresh();
    assert.equal(controller.state.db.phase, "ready");
  } finally {
    writer.close();
  }
});

test("designed states: not a database, gone, and a command that failed", { skip }, async (t) => {
  const dir = workspaceWithDatabase(t);
  const fs = require("node:fs");
  fs.writeFileSync(`${dir}/z.sqlite`, "x".repeat(4096));
  const { controller } = open(fakeHangar({ cwd: dir, params: { path: "z.sqlite" } }));
  await controller.start();
  assert.equal(controller.state.files.some((file) => file.path === "z.sqlite"), true, "an agent may ask for a file the listing left out");
  assert.equal(controller.state.db.phase, "not-database");
  assert.match(draw(controller).text, /not a SQLite database/);
  const gone = open(fakeHangar({ cwd: dir, params: { path: "missing.db" } }));
  await gone.controller.start();
  assert.equal(gone.controller.state.db.phase, "gone");
  const broken = open(fakeHangar({ cwd: dir, commands: { tables: () => ({ exitCode: 1, stdout: "", stderr: "boom: it broke\nmore", timedOut: false }) } }));
  await broken.controller.start();
  assert.equal(broken.controller.state.db.phase, "error");
  assert.match(draw(broken.controller).text, /boom: it broke/);
  const slow = open(fakeHangar({ cwd: dir, commands: { list: () => ({ exitCode: null, stdout: "", stderr: "", timedOut: true }) } }));
  await slow.controller.start();
  assert.equal(slow.controller.state.view, "error");
  assert.match(draw(slow.controller).text, /took longer than the time allowed/);
});

test("an answer that is not the script's is refused, not drawn", () => {
  assert.equal(logic.parseAnswer("not json"), null);
  assert.equal(logic.parseAnswer("[1,2]"), null);
  assert.equal(logic.parseAnswer('{"files":[]}'), null);
  assert.deepEqual(logic.parseAnswer('{"ok":false,"error":"made-up"}'), { ok: false, error: "failed", detail: "" });
  assert.equal(logic.cleanTables({ tables: [{ name: "a b", rows: -3, viewable: true }, { rows: 1 }, null] }).tables[0].viewable, false);
  assert.equal(logic.cleanRows({ rows: [[{ type: "evil", value: "x" }]] }).rows[0][0].type, "null");
});

test("a stale answer never replaces a newer one", { skip }, async (t) => {
  const dir = workspaceWithDatabase(t);
  const fs = require("node:fs");
  fs.copyFileSync(`${dir}/app.db`, `${dir}/b.db`);
  const hangar = fakeHangar({ cwd: dir });
  const { controller } = open(hangar);
  await controller.start();
  const first = controller.pick("b.db");
  const second = controller.pick("app.db");
  await Promise.all([first, second]);
  assert.equal(controller.state.path, "app.db");
  assert.equal(controller.state.db.phase, "ready");
});
