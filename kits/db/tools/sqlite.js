"use strict";
// The only door to a database: every connection is read-only, whichever driver answers.
//   1. node:sqlite, when this Node has it (22.12 or later, where `readOnly` exists);
//   2. else the `sqlite3` program, started with -readonly and no shell.
// A connection only ever runs the fixed SELECTs and PRAGMA reads of core.js, and `query_only`
// is switched on besides, so a write fails even if one slipped through.
const { spawnSync } = require("node:child_process");

const BUSY_MS = 300;
const MAX_OUTPUT = 32 * 1024 * 1024;

class SqliteError extends Error {
  constructor(code, message) {
    super(message);
    this.code = code;
  }
}

/** Maps what SQLite said to one of the kit's own codes. */
function classify(message, errcode) {
  const text = String(message || "");
  const primary = Number.isInteger(errcode) ? errcode & 0xff : null;
  // 5 busy, 6 locked, 8 read-only (a hot journal or a WAL this connection may not recover).
  if (primary === 5 || primary === 6 || primary === 8 || /database (table )?is locked|readonly database/i.test(text)) return "locked";
  if (primary === 26 || /not a database|malformed|encrypted/i.test(text)) return "not-database";
  return "failed";
}

function nodeAtLeast(major, minor) {
  const [a, b] = process.versions.node.split(".").map(Number);
  return a > major || (a === major && b >= minor);
}

function nodeDriver() {
  if (!nodeAtLeast(22, 12)) return null;
  let sqlite;
  try {
    sqlite = require("node:sqlite");
  } catch {
    return null;
  }
  if (!sqlite || typeof sqlite.DatabaseSync !== "function") return null;
  return {
    name: "node:sqlite",
    open(file) {
      let db;
      try {
        db = new sqlite.DatabaseSync(file, { readOnly: true });
        db.exec(`PRAGMA busy_timeout = ${BUSY_MS}`);
        db.exec("PRAGMA query_only = ON");
        if (Number(db.prepare("PRAGMA query_only").get().query_only) !== 1) throw new SqliteError("failed", "could not make the connection read-only");
      } catch (error) {
        try {
          if (db) db.close();
        } catch {
          /* closed already */
        }
        if (error instanceof SqliteError) throw error;
        throw new SqliteError(classify(error.message, error.errcode), error.message);
      }
      return {
        all(sql) {
          try {
            return db.prepare(sql).all();
          } catch (error) {
            throw new SqliteError(classify(error.message, error.errcode), error.message);
          }
        },
        close() {
          try {
            db.close();
          } catch {
            /* nothing left to release */
          }
        },
      };
    },
  };
}

function cliDriver() {
  const probe = spawnSync("sqlite3", ["-version"], { encoding: "utf8", windowsHide: true, timeout: 5000 });
  if (probe.error || probe.status !== 0) return null;
  return {
    name: "sqlite3",
    open(file) {
      const run = (sql) => {
        const answer = spawnSync("sqlite3", ["-readonly", "-batch", "-bail", "-json", "-cmd", `.timeout ${BUSY_MS}`, "-cmd", "PRAGMA query_only = ON", file, sql], {
          encoding: "utf8",
          windowsHide: true,
          timeout: 20000,
          maxBuffer: MAX_OUTPUT,
        });
        if (answer.error) throw new SqliteError("failed", answer.error.message);
        if (answer.status !== 0) {
          const message = (answer.stderr || "").trim() || `sqlite3 exited with ${answer.status}`;
          if (/unknown option/i.test(message)) throw new SqliteError("no-sqlite", "this sqlite3 is too old: it has no -json (3.33 or later is needed)");
          throw new SqliteError(classify(message), message);
        }
        const out = (answer.stdout || "").trim();
        if (!out) return [];
        try {
          return JSON.parse(out);
        } catch {
          throw new SqliteError("failed", "sqlite3 printed something that is not JSON");
        }
      };
      run("SELECT 1"); // fails here, not in the middle of a verb, when the file is locked or not a database
      return { all: run, close() {} };
    },
  };
}

/** The driver to use: `HANGAR_DB_DRIVER` (node, sqlite3 or none) narrows the choice, for tests. Null when none is there. */
function pickDriver(env = process.env) {
  const wanted = env.HANGAR_DB_DRIVER;
  if (wanted === "none") return null;
  if (wanted === "sqlite3") return cliDriver();
  if (wanted === "node") return nodeDriver();
  return nodeDriver() || cliDriver();
}

module.exports = { pickDriver, nodeDriver, cliDriver, SqliteError, classify, BUSY_MS };
