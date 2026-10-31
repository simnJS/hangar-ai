"use strict";
// The four verbs of the kit, over a SQLite file inside the folder the command was started in
// (the workspace). Nothing here takes SQL from outside: a verb's arguments are a path, a table
// name, a column number, a direction and numbers, each checked against a strict pattern, and the
// statements are fixed text with those checked values in them. The connection is read-only.
const fs = require("node:fs");
const path = require("node:path");
const { pickDriver, SqliteError } = require("./sqlite");

const PATH_PATTERN = /^[A-Za-z0-9_.][A-Za-z0-9_./ -]{0,254}$/;
const TABLE_PATTERN = /^[A-Za-z_][A-Za-z0-9_]{0,63}$/;
const EXTENSIONS = [".db", ".sqlite", ".sqlite3", ".db3"];
const SKIPPED_FOLDERS = new Set([".git", "node_modules", "target", "dist", "__pycache__", ".venv", "venv"]);
const HEADER = Buffer.from("SQLite format 3\0", "latin1");

const MAX_FILES = 100;
const MAX_VISITED = 5000;
const MAX_DEPTH = 6;
const MAX_TABLES = 500;
const MAX_COLUMNS = 200;
const MAX_LIMIT = 200;
const MAX_CHARS = 100000;
// The most text one answer may carry in its cells: Hangar keeps 1 MB of a command's output.
const CELL_BUDGET = 150000;

const quote = (identifier) => `"${String(identifier).replace(/"/g, '""')}"`;
const literal = (value) => `'${String(value).replace(/'/g, "''")}'`;
const fail = (error, detail) => ({ ok: false, error, ...(detail ? { detail: String(detail).slice(0, 300) } : {}) });
const whole = (value, max) => (/^[0-9]{1,9}$/.test(String(value)) && Number(value) <= max ? Number(value) : null);
const insidePath = (relative) => PATH_PATTERN.test(relative) && !relative.split("/").includes("..");

function hasHeader(file) {
  let fd;
  try {
    fd = fs.openSync(file, "r");
    const head = Buffer.alloc(HEADER.length);
    return fs.readSync(fd, head, 0, head.length, 0) === HEADER.length && head.equals(HEADER);
  } catch {
    return false;
  } finally {
    if (fd !== undefined) fs.closeSync(fd);
  }
}

/** The file a relative path leads to, inside the working folder and a SQLite database; else the failure. */
function resolveDatabase(relative, cwd) {
  if (typeof relative !== "string" || !insidePath(relative)) return { failure: fail("bad-args", "path") };
  let root;
  let full;
  try {
    root = fs.realpathSync(cwd);
    full = fs.realpathSync(path.resolve(root, relative));
  } catch {
    return { failure: fail("not-found") };
  }
  if (!full.startsWith(root.endsWith(path.sep) ? root : root + path.sep)) return { failure: fail("outside") };
  let stat;
  try {
    stat = fs.statSync(full);
  } catch {
    return { failure: fail("not-found") };
  }
  if (!stat.isFile() || !hasHeader(full)) return { failure: fail("not-database") };
  return { full, size: stat.size };
}

/** `list`: the SQLite files of the working folder, found by extension and checked by their header. */
function list(cwd) {
  const found = [];
  let visited = 0;
  let truncated = false;
  const walk = (dir, depth) => {
    let entries;
    try {
      entries = fs.readdirSync(dir, { withFileTypes: true });
    } catch {
      return;
    }
    for (const entry of entries) {
      if (truncated) return;
      if (++visited > MAX_VISITED) {
        truncated = true;
        return;
      }
      const full = path.join(dir, entry.name);
      if (entry.isDirectory()) {
        if (depth < MAX_DEPTH && !SKIPPED_FOLDERS.has(entry.name)) walk(full, depth + 1);
      } else if (entry.isFile() && EXTENSIONS.includes(path.extname(entry.name).toLowerCase()) && hasHeader(full)) {
        const relative = path.relative(cwd, full).split(path.sep).join("/");
        try {
          const stat = fs.statSync(full);
          found.push({ path: relative, size: stat.size, modified: Math.round(stat.mtimeMs), usable: insidePath(relative) });
        } catch {
          /* gone meanwhile */
        }
      }
    }
  };
  walk(cwd, 0);
  found.sort((a, b) => b.modified - a.modified || a.path.localeCompare(b.path));
  if (found.length > MAX_FILES) truncated = true;
  return { ok: true, files: found.slice(0, MAX_FILES), truncated };
}

/** Opens `relative` read-only and hands the connection to `body`, closing it after. */
function withDatabase(relative, cwd, env, body) {
  const resolved = resolveDatabase(relative, cwd);
  if (resolved.failure) return resolved.failure;
  const driver = pickDriver(env);
  if (!driver) return fail("no-sqlite");
  let connection;
  try {
    connection = driver.open(resolved.full);
    return body(connection, driver.name, resolved);
  } catch (error) {
    if (error instanceof SqliteError) return fail(error.code, error.message);
    return fail("failed", error && error.message);
  } finally {
    if (connection) connection.close();
  }
}

const objects = (db) => db.all(`SELECT name, type, sql FROM sqlite_master WHERE type IN ('table','view') AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\' ORDER BY name LIMIT ${MAX_TABLES}`);

/** The table named `name`, if the database has it and the kit may open it. */
function findTable(db, name) {
  if (typeof name !== "string" || !TABLE_PATTERN.test(name)) return { failure: fail("bad-args", "table") };
  const table = objects(db).find((item) => item.name === name);
  return table ? { table } : { failure: fail("bad-table") };
}

const columnsOf = (db, name) =>
  db
    .all(`SELECT name, type, "notnull" AS "notnull", pk, dflt_value AS dflt FROM pragma_table_info(${literal(name)})`)
    .map((column) => ({
      name: String(column.name),
      type: String(column.type || ""),
      notNull: Number(column.notnull) === 1,
      primaryKey: Number(column.pk) > 0,
      default: column.dflt === null || column.dflt === undefined ? null : String(column.dflt).slice(0, 200),
    }));

function tables(relative, cwd, env) {
  return withDatabase(relative, cwd, env, (db, driver, file) => {
    const items = objects(db).map((item) => ({
      name: item.name,
      type: item.type,
      rows: Number(db.all(`SELECT count(*) AS n FROM ${quote(item.name)}`)[0].n),
      columns: Number(db.all(`SELECT count(*) AS n FROM pragma_table_info(${literal(item.name)})`)[0].n),
      viewable: TABLE_PATTERN.test(item.name),
    }));
    return { ok: true, driver, size: file.size, tables: items };
  });
}

function schema(relative, table, cwd, env) {
  return withDatabase(relative, cwd, env, (db, driver) => {
    const { table: found, failure } = findTable(db, table);
    if (failure) return failure;
    const columns = columnsOf(db, found.name);
    const indexes =
      found.type === "table"
        ? db.all(`SELECT name, "unique" AS "unique" FROM pragma_index_list(${literal(found.name)})`).map((index) => ({ name: String(index.name), unique: Number(index.unique) === 1 }))
        : [];
    return {
      ok: true,
      driver,
      table: found.name,
      type: found.type,
      columns: columns.slice(0, MAX_COLUMNS),
      columnsTruncated: columns.length > MAX_COLUMNS,
      indexes: indexes.slice(0, 50),
      sql: String(found.sql || "").slice(0, 4000),
    };
  });
}

/**
 * `rows`: one page of a table. `sort` is a column's number from 1, 0 for none; `direction` asc or
 * desc; `offset` the first row; `limit` the page size (at most 200); `chars` how much of a text
 * value comes back (at most 100 000: the full value of one cell is a page of one row with a large
 * `chars`). Text is cut in SQL, so a huge value never leaves the database whole; a blob never does.
 */
function rows(relative, table, sort, direction, offset, limit, chars, cwd, env) {
  const sortNumber = whole(sort, 9999);
  const offsetNumber = whole(offset, 999999999);
  const limitAsked = whole(limit, 999999999);
  const charsAsked = whole(chars, 999999999);
  if (sortNumber === null || offsetNumber === null || !limitAsked || !charsAsked || (direction !== "asc" && direction !== "desc")) return fail("bad-args", "numbers");
  return withDatabase(relative, cwd, env, (db, driver) => {
    const { table: found, failure } = findTable(db, table);
    if (failure) return failure;
    const columns = columnsOf(db, found.name).slice(0, MAX_COLUMNS);
    if (sortNumber > columns.length) return fail("bad-args", "sort");
    const total = Number(db.all(`SELECT count(*) AS n FROM ${quote(found.name)}`)[0].n);
    const base = { ok: true, driver, table: found.name, columns: columns.map((c) => ({ name: c.name, type: c.type, primaryKey: c.primaryKey })), offset: offsetNumber, total, sort: sortNumber, direction };
    if (!columns.length) return { ...base, rows: [], limit: 0, chars: 0, capped: false };
    const chars = Math.min(charsAsked, MAX_CHARS, Math.max(1, Math.floor(CELL_BUDGET / columns.length)));
    const pageSize = Math.max(1, Math.min(limitAsked, MAX_LIMIT, Math.floor(CELL_BUDGET / (columns.length * chars))));
    const select = columns
      .map((column, index) => {
        const c = quote(column.name);
        return [
          `typeof(${c}) AS "t${index}"`,
          `length(${c}) AS "l${index}"`,
          `CASE typeof(${c}) WHEN 'blob' THEN NULL WHEN 'text' THEN substr(${c}, 1, ${chars}) WHEN 'null' THEN NULL ELSE CAST(${c} AS TEXT) END AS "v${index}"`,
        ].join(", ");
      })
      .join(", ");
    const hasRowid = found.type === "table" && !/WITHOUT\s+ROWID/i.test(found.sql || "");
    const order = [];
    if (sortNumber > 0) order.push(`${quote(columns[sortNumber - 1].name)} ${direction === "desc" ? "DESC" : "ASC"}`);
    if (hasRowid) order.push("rowid");
    const sql = `SELECT ${select} FROM ${quote(found.name)}${order.length ? ` ORDER BY ${order.join(", ")}` : ""} LIMIT ${pageSize} OFFSET ${offsetNumber}`;
    const page = db.all(sql).map((row) =>
      columns.map((_, index) => {
        const type = String(row[`t${index}`]);
        const length = row[`l${index}`] === null || row[`l${index}`] === undefined ? 0 : Number(row[`l${index}`]);
        if (type === "null") return { type };
        if (type === "blob") return { type, length };
        const value = row[`v${index}`] === null || row[`v${index}`] === undefined ? "" : String(row[`v${index}`]);
        return type === "text" ? { type, value, length, cut: length > value.length } : { type, value };
      }),
    );
    return { ...base, rows: page, limit: pageSize, chars, capped: chars < charsAsked || pageSize < limitAsked };
  });
}

/** Runs a verb from its argv. Anything else, or the wrong number of arguments, is `bad-args`. */
function run(argv, { cwd = process.cwd(), env = process.env } = {}) {
  const [verb, ...args] = argv;
  if (verb === "list" && args.length === 0) return list(cwd);
  if (verb === "tables" && args.length === 1) return tables(args[0], cwd, env);
  if (verb === "schema" && args.length === 2) return schema(args[0], args[1], cwd, env);
  if (verb === "rows" && args.length === 7) return rows(...args, cwd, env);
  return fail("bad-args", "usage: list | tables PATH | schema PATH TABLE | rows PATH TABLE SORT DIRECTION OFFSET LIMIT CHARS");
}

module.exports = { run, PATH_PATTERN, TABLE_PATTERN, EXTENSIONS, MAX_LIMIT, MAX_CHARS };
