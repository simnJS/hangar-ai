"use strict";
const test = require("node:test");
const assert = require("node:assert/strict");
const crypto = require("node:crypto");
const fs = require("node:fs");
const path = require("node:path");
const { spawnSync } = require("node:child_process");
const { KIT, SECRETS, tempWorkspace, workspaceWithDatabase, canBuildFixture, hasSqliteProgram, runDeclared, db } = require("./helpers");
const sqlite = require("../tools/sqlite");

const skip = canBuildFixture ? false : "neither node:sqlite nor the sqlite3 program is here to build the fixture";
const hash = (file) => crypto.createHash("sha256").update(fs.readFileSync(file)).digest("hex");
const rowsArgs = (extra = {}) => ({ path: "app.db", table: "users", sort: "0", direction: "asc", offset: "0", limit: "50", chars: "200", ...extra });

test("list: finds SQLite files by extension, checks their header, skips what is not one", { skip }, (t) => {
  const dir = workspaceWithDatabase(t);
  fs.mkdirSync(path.join(dir, "data", "deep"), { recursive: true });
  fs.copyFileSync(path.join(dir, "app.db"), path.join(dir, "data", "deep", "copy.sqlite3"));
  fs.writeFileSync(path.join(dir, "fake.db"), "this is not a database");
  fs.writeFileSync(path.join(dir, "notes.txt"), "SQLite format 3\0");
  fs.mkdirSync(path.join(dir, "node_modules", "x"), { recursive: true });
  fs.copyFileSync(path.join(dir, "app.db"), path.join(dir, "node_modules", "x", "skipped.db"));
  fs.copyFileSync(path.join(dir, "app.db"), path.join(dir, "bad ;name.db"));
  const answer = db(dir, "list");
  assert.equal(answer.ok, true);
  assert.deepEqual(answer.files.map((file) => file.path).sort(), ["app.db", "bad ;name.db", "data/deep/copy.sqlite3"]);
  assert.equal(answer.files.find((file) => file.path === "bad ;name.db").usable, false, "a name the patterns refuse is listed, as not openable");
  assert.equal(answer.files.find((file) => file.path === "app.db").usable, true);
  assert.ok(answer.files.every((file) => file.size > 0 && file.modified > 0));
});

test("list: an empty folder has none", (t) => {
  const answer = db(tempWorkspace(t), "list");
  assert.deepEqual(answer, { ok: true, files: [], truncated: false });
});

test("tables: names, kinds, counts and column counts; sqlite's own tables are left out", { skip }, (t) => {
  const dir = workspaceWithDatabase(t);
  const answer = db(dir, "tables", { path: "app.db" });
  assert.equal(answer.ok, true);
  assert.ok(["node:sqlite", "sqlite3"].includes(answer.driver));
  const byName = Object.fromEntries(answer.tables.map((item) => [item.name, item]));
  assert.deepEqual(Object.keys(byName).sort(), ["active_users", "empty_one", "events", "odd name", "pairs", "users"]);
  assert.equal(byName.users.rows, 3);
  assert.equal(byName.users.columns, 9);
  assert.equal(byName.events.rows, 120);
  assert.equal(byName.empty_one.rows, 0);
  assert.equal(byName.active_users.type, "view");
  assert.equal(byName.active_users.rows, 2);
  assert.equal(byName["odd name"].viewable, false, "a name outside the pattern is listed but never opened");
  assert.equal(byName.users.viewable, true);
});

test("schema: columns, keys, defaults, indexes", { skip }, (t) => {
  const dir = workspaceWithDatabase(t);
  const answer = db(dir, "schema", { path: "app.db", table: "users" });
  assert.equal(answer.ok, true);
  const columns = Object.fromEntries(answer.columns.map((column) => [column.name, column]));
  assert.equal(columns.id.primaryKey, true);
  assert.equal(columns.name.notNull, true);
  assert.equal(columns.api_token.default, "'none'");
  assert.equal(columns.avatar.type, "BLOB");
  assert.deepEqual(answer.indexes.map((index) => [index.name, index.unique]).sort(), [["users_email", true], ["users_name", false]]);
  assert.match(answer.sql, /^CREATE TABLE users/);
  assert.equal(db(dir, "schema", { path: "app.db", table: "active_users" }).type, "view");
});

test("rows: a page, with every type told apart, and the text cut in the database", { skip }, (t) => {
  const dir = workspaceWithDatabase(t);
  const answer = db(dir, "rows", rowsArgs({ chars: "20" }));
  assert.equal(answer.ok, true);
  assert.equal(answer.total, 3);
  assert.deepEqual(answer.columns.map((column) => column.name), ["id", "name", "email", "api_token", "password_hash", "bio", "avatar", "score", "active"]);
  const [ada, bob, eloise] = answer.rows;
  assert.deepEqual(ada[0], { type: "integer", value: "1" });
  assert.deepEqual(ada[1], { type: "text", value: "Ada", length: 3, cut: false });
  assert.equal(ada[5].type, "text");
  assert.equal(ada[5].value.length, 20);
  assert.equal(ada[5].length, 6000);
  assert.equal(ada[5].cut, true);
  assert.deepEqual(ada[6], { type: "blob", length: 8 }, "a blob is never sent, only its size");
  assert.deepEqual(ada[7], { type: "real", value: "3.5" });
  assert.deepEqual(bob[5], { type: "null" });
  assert.deepEqual(bob[7], { type: "real", value: "-1.25" });
  assert.equal(eloise[1].value, "Éloïse ‘quoted’");
  assert.equal(eloise[5].value, 'it\'s a "test"');
  assert.ok(!JSON.stringify(answer).includes("lorem ipsum lorem ipsum lorem"), "the long value did not leave whole");
});

test("rows: the whole of one cell is a page of one row with a large cut", { skip }, (t) => {
  const dir = workspaceWithDatabase(t);
  const answer = db(dir, "rows", rowsArgs({ limit: "1", chars: "100000" }));
  assert.equal(answer.rows.length, 1);
  assert.equal(answer.rows[0][5].value.length, 6000);
  assert.equal(answer.rows[0][5].cut, false);
});

test("rows: paging, sorting both ways, and a stable order among equal values", { skip }, (t) => {
  const dir = workspaceWithDatabase(t);
  const ask = (extra) => db(dir, "rows", rowsArgs({ table: "events", ...extra }));
  const first = ask({ limit: "50", offset: "0" });
  const second = ask({ limit: "50", offset: "50" });
  const last = ask({ limit: "50", offset: "100" });
  assert.deepEqual([first.rows.length, second.rows.length, last.rows.length], [50, 50, 20]);
  assert.equal(first.total, 120);
  const ids = [...first.rows, ...second.rows, ...last.rows].map((row) => Number(row[0].value));
  assert.deepEqual(ids, Array.from({ length: 120 }, (_, i) => i + 1));
  assert.equal(ask({ offset: "500" }).rows.length, 0);
  const byKind = ask({ sort: "2", limit: "200" }).rows.map((row) => row[1].value);
  assert.deepEqual(byKind, [...byKind].sort());
  const down = ask({ sort: "2", direction: "desc", limit: "200" }).rows.map((row) => row[1].value);
  assert.deepEqual(down, [...down].sort().reverse());
  const pagesA = [0, 40, 80].flatMap((offset) => ask({ sort: "2", offset: String(offset), limit: "40" }).rows.map((row) => row[0].value));
  assert.equal(new Set(pagesA).size, 120, "pages of a sort on repeated values neither repeat nor skip a row");
  const pairs = db(dir, "rows", rowsArgs({ table: "pairs" }));
  assert.deepEqual(pairs.rows.map((row) => row[0].value + row[1].value), ["a9", "x1", "x2"], "a table without rowid pages in key order");
});

test("rows: capped, whatever is asked", { skip }, (t) => {
  const dir = workspaceWithDatabase(t);
  const big = db(dir, "rows", rowsArgs({ table: "events", limit: "999", chars: "200" }));
  assert.equal(big.ok, true);
  assert.equal(big.limit, 200);
  assert.equal(big.rows.length, 120);
  assert.equal(db(dir, "rows", rowsArgs({ limit: "1", chars: "999999" })).chars, 16666, "the cut shrinks with the number of columns");
  const wide = db(dir, "rows", rowsArgs({ limit: "200", chars: "100000" }));
  assert.ok(wide.chars * wide.columns.length <= 150000, "the cut keeps the answer under what Hangar keeps of a command's output");
  assert.equal(wide.capped, true);
  assert.ok(JSON.stringify(wide).length < 900000);
  assert.equal(db(dir, "rows", rowsArgs({ table: "empty_one" })).rows.length, 0);
});

test("strict patterns: a table or a path outside them never reaches the script", { skip }, (t) => {
  const dir = workspaceWithDatabase(t);
  for (const table of ["users; DROP TABLE users", 'users"', "users--", "us ers", "1users", "", "users)", "../users", "-users"]) {
    assert.throws(() => runDeclared("schema", { path: "app.db", table }, dir), /bad_args/, JSON.stringify(table));
  }
  for (const bad of ["/etc/passwd", "C:\\x.db", "-rf", "a\\b.db", "a;b.db", 'a"b.db', "a$(x).db", "x\0y", "~/x.db", ""]) {
    assert.throws(() => runDeclared("tables", { path: bad }, dir), /bad_args/, JSON.stringify(bad));
  }
  for (const [name, value] of [["sort", "-1"], ["sort", "1; "], ["direction", "sideways"], ["offset", "-5"], ["limit", "1000"], ["chars", "1e3"]]) {
    assert.throws(() => runDeclared("rows", rowsArgs({ [name]: value }), dir), /bad_args/, `${name}=${value}`);
  }
});

test("the script checks again what Hangar already did: it is run by hand too", { skip }, (t) => {
  const dir = workspaceWithDatabase(t);
  const run = (...argv) => JSON.parse(spawnSync(process.execPath, [path.join(KIT, "tools", "db.js"), ...argv], { cwd: dir, encoding: "utf8" }).stdout);
  assert.equal(run("schema", "app.db", "users; DROP TABLE users").error, "bad-args");
  assert.equal(run("schema", "app.db", "nope").error, "bad-table");
  assert.equal(run("schema", "app.db", "sqlite_sequence").error, "bad-table", "sqlite's own tables are not the kit's to open");
  assert.equal(run("tables", "../app.db").error, "bad-args");
  assert.equal(run("tables", "a/../app.db").error, "bad-args");
  assert.equal(run("tables", "/app.db").error, "bad-args");
  assert.equal(run("tables", "missing.db").error, "not-found");
  assert.equal(run("rows", "app.db", "users", "0", "asc", "0", "5").error, "bad-args", "the wrong number of arguments");
  assert.equal(run("rows", "app.db", "users", "99", "asc", "0", "5", "10").error, "bad-args", "a column that is not there");
  assert.equal(run("select", "app.db", "SELECT 1").error, "bad-args", "there is no verb that takes SQL");
  assert.equal(run().error, "bad-args");
});

test("a path outside the workspace is refused, links included", { skip }, (t) => {
  const dir = workspaceWithDatabase(t);
  const outside = tempWorkspace(t);
  fs.copyFileSync(path.join(dir, "app.db"), path.join(outside, "other.db"));
  const inner = path.join(dir, "inner");
  fs.mkdirSync(inner);
  fs.copyFileSync(path.join(dir, "app.db"), path.join(inner, "own.db"));
  assert.equal(db(inner, "tables", { path: "own.db" }).ok, true);
  assert.equal(db(inner, "tables", { path: "../app.db" }).ok, false);
  let linked = true;
  try {
    fs.symlinkSync(path.join(outside, "other.db"), path.join(inner, "link.db"), "file");
  } catch {
    linked = false; // creating a link needs a privilege on some systems
  }
  if (linked) assert.equal(db(inner, "tables", { path: "link.db" }).error, "outside");
});

test("a file that is not a SQLite database is told so, whatever its name", { skip }, (t) => {
  const dir = tempWorkspace(t);
  fs.writeFileSync(path.join(dir, "text.db"), "just words");
  fs.writeFileSync(path.join(dir, "empty.db"), "");
  fs.mkdirSync(path.join(dir, "folder.db"));
  for (const name of ["text.db", "empty.db", "folder.db"]) assert.equal(db(dir, "tables", { path: name }).error, "not-database", name);
});

test("read-only: no statement can write, and the file is byte for byte what it was", { skip: !canBuildFixture || !sqlite.nodeDriver() ? "node:sqlite is not here" : false }, (t) => {
  const dir = workspaceWithDatabase(t);
  const file = path.join(dir, "app.db");
  const before = hash(file);
  const connection = sqlite.nodeDriver().open(file);
  const writes = [
    "INSERT INTO users (name) VALUES ('mallory')",
    "UPDATE users SET name = 'x'",
    "DELETE FROM users",
    "DROP TABLE users",
    "CREATE TABLE intruder (a)",
    "ALTER TABLE users ADD COLUMN extra TEXT",
    "VACUUM",
    "CREATE INDEX i ON users(email)",
  ];
  for (const sql of writes) assert.throws(() => connection.all(sql), (error) => error instanceof sqlite.SqliteError, sql);
  // Even the pragma that lifts query_only does not make the connection writable: it was opened read-only.
  connection.all("PRAGMA query_only = OFF");
  assert.throws(() => connection.all("INSERT INTO users (name) VALUES ('mallory')"), sqlite.SqliteError);
  assert.throws(() => connection.all("DROP TABLE users"), sqlite.SqliteError);
  connection.close();
  assert.equal(hash(file), before);
});

test("read-only: every verb, run over the file, leaves it untouched", { skip }, (t) => {
  const dir = workspaceWithDatabase(t);
  const file = path.join(dir, "app.db");
  const before = hash(file);
  const modified = fs.statSync(file).mtimeMs;
  db(dir, "list");
  db(dir, "tables", { path: "app.db" });
  db(dir, "schema", { path: "app.db", table: "users" });
  db(dir, "rows", rowsArgs({ sort: "2", direction: "desc" }));
  assert.equal(hash(file), before);
  assert.equal(fs.statSync(file).mtimeMs, modified);
  assert.deepEqual(fs.readdirSync(dir).sort(), ["app.db"], "no journal, no side file");
});

test("read-only: a database file marked read-only is still read", { skip }, (t) => {
  const dir = workspaceWithDatabase(t);
  const file = path.join(dir, "app.db");
  fs.chmodSync(file, 0o444);
  try {
    assert.equal(db(dir, "tables", { path: "app.db" }).ok, true);
  } finally {
    fs.chmodSync(file, 0o666);
  }
});

test("locked: a writer holding the file answers locked, and nothing is changed", { skip: !canBuildFixture || !sqlite.nodeDriver() ? "node:sqlite is not here" : false }, (t) => {
  const dir = workspaceWithDatabase(t);
  const file = path.join(dir, "app.db");
  const before = hash(file);
  const { DatabaseSync } = require("node:sqlite");
  const writer = new DatabaseSync(file);
  writer.exec("BEGIN EXCLUSIVE");
  try {
    for (const [name, args] of [["tables", { path: "app.db" }], ["schema", { path: "app.db", table: "users" }], ["rows", rowsArgs()]]) {
      const answer = db(dir, name, args);
      assert.deepEqual([answer.ok, answer.error], [false, "locked"], name);
    }
    writer.exec("ROLLBACK");
    assert.equal(db(dir, "tables", { path: "app.db" }).ok, true, "readable again once the writer let go");
  } finally {
    writer.close(); // before the folder is removed: a file held open cannot be, on Windows
  }
  assert.equal(hash(file), before);
});

test("no sqlite: neither node:sqlite nor the program answers no-sqlite, never a guess", { skip }, (t) => {
  const dir = workspaceWithDatabase(t);
  const answer = db(dir, "tables", { path: "app.db" }, { HANGAR_DB_DRIVER: "none" });
  assert.equal(answer.ok, false);
  assert.equal(answer.error, "no-sqlite");
});

test("the sqlite3 program answers the same, read-only too", { skip: !canBuildFixture || !hasSqliteProgram ? "the sqlite3 program is not here" : false }, (t) => {
  const dir = workspaceWithDatabase(t);
  const env = { HANGAR_DB_DRIVER: "sqlite3" };
  const viaProgram = db(dir, "rows", rowsArgs({ sort: "2", direction: "desc", chars: "20" }), env);
  assert.equal(viaProgram.driver, "sqlite3");
  const viaNode = db(dir, "rows", rowsArgs({ sort: "2", direction: "desc", chars: "20" }), { HANGAR_DB_DRIVER: "node" });
  if (viaNode.ok) assert.deepEqual({ ...viaProgram, driver: null }, { ...viaNode, driver: null });
  assert.equal(db(dir, "tables", { path: "app.db" }, env).tables.length, 6);
  const connection = sqlite.cliDriver().open(path.join(dir, "app.db"));
  const before = hash(path.join(dir, "app.db"));
  for (const sql of ["INSERT INTO users (name) VALUES ('mallory')", "DROP TABLE users", "CREATE TABLE intruder (a)"]) assert.throws(() => connection.all(sql), sqlite.SqliteError, sql);
  assert.equal(hash(path.join(dir, "app.db")), before);
});

test("the planted secrets are only ever in `rows`, never in list, tables or schema", { skip }, (t) => {
  const dir = workspaceWithDatabase(t);
  const outputs = [db(dir, "list"), db(dir, "tables", { path: "app.db" }), db(dir, "schema", { path: "app.db", table: "users" })].map((answer) => JSON.stringify(answer));
  for (const secret of Object.values(SECRETS)) for (const output of outputs) assert.ok(!output.includes(secret), secret);
  assert.ok(JSON.stringify(db(dir, "rows", rowsArgs())).includes(SECRETS.token));
});
