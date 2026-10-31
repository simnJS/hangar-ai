"use strict";
// The kit's data folder, as the hook writes it and the tab reads it.
//
//   index.json        { version: 1, runs: [ { key, at, status, framework, passed, failed,
//                     skipped, project, agent } ] }, newest first. What the tab reads
//                     first: it has no way to list a folder.
//   runs/<key>.json   the last run of one agent in one project, whole, failures included.
//                     One file each, so a hook writing its run touches no other.
//
// A key is the project and the agent, hashed: the next run of the same agent in the same
// project replaces the last. Hooks of several agents run at the same time, so the index
// is changed under a lock (a folder, which only one process can make).

const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const crypto = require("node:crypto");

const INDEX = "index.json";
const RUNS = "runs";
const LOCK = ".lock";
const KEEP_RUNS = 40;
const KEEP_MS = 3 * 24 * 60 * 60 * 1000;
const LOCK_WAIT_MS = 1500;
const LOCK_STALE_MS = 4000;

function dataFolder(env = process.env) {
  return env.HANGAR_KITS_DATA ? path.join(env.HANGAR_KITS_DATA, "tests") : path.join(os.homedir(), ".hangar", "kit-data", "tests");
}

const runKey = (project, agent) => crypto.createHash("sha256").update(`${project}\0${agent}`).digest("hex").slice(0, 16);

const sleep = (ms) => Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);

/** A file written whole: a temp file in the same folder, then a rename. */
function writeAtomic(file, text) {
  const temp = `${file}.${process.pid}.${Date.now()}.tmp`;
  fs.writeFileSync(temp, text);
  for (let attempt = 0; ; attempt++) {
    try {
      fs.renameSync(temp, file);
      return;
    } catch (error) {
      // On Windows a reader holding the file open makes the rename fail for a moment.
      if (attempt >= 4) {
        try {
          fs.unlinkSync(temp);
        } catch {
          /* gone */
        }
        throw error;
      }
      sleep(10 * (attempt + 1));
    }
  }
}

function readJson(file) {
  try {
    return JSON.parse(fs.readFileSync(file, "utf8").replace(/^﻿/, ""));
  } catch {
    return null;
  }
}

/** Runs `work` holding the lock. After LOCK_WAIT_MS it goes ahead without: a late write beats none. */
function locked(dir, work) {
  const lock = path.join(dir, LOCK);
  const until = Date.now() + LOCK_WAIT_MS;
  let held = false;
  for (;;) {
    try {
      fs.mkdirSync(lock);
      held = true;
      break;
    } catch (error) {
      if (!error || error.code !== "EEXIST") break;
      try {
        if (Date.now() - fs.statSync(lock).mtimeMs > LOCK_STALE_MS) fs.rmdirSync(lock);
      } catch {
        /* someone else cleared it */
      }
      if (Date.now() > until) break;
      sleep(15);
    }
  }
  try {
    return work();
  } finally {
    if (held) {
      try {
        fs.rmdirSync(lock);
      } catch {
        /* already gone */
      }
    }
  }
}

function summaryOf(run) {
  return {
    key: run.key,
    at: run.at,
    status: run.status,
    framework: run.framework,
    passed: run.passed,
    failed: run.failed,
    skipped: run.skipped,
    project: run.project,
    agent: run.agent,
  };
}

/** Writes the run, and puts it first in the index, forgetting the runs nobody needs. */
function saveRun(dir, run) {
  fs.mkdirSync(path.join(dir, RUNS), { recursive: true });
  writeAtomic(path.join(dir, RUNS, `${run.key}.json`), JSON.stringify(run));
  locked(dir, () => {
    const current = readJson(path.join(dir, INDEX));
    const known = current && Array.isArray(current.runs) ? current.runs : [];
    const all = [summaryOf(run), ...known.filter((entry) => entry && entry.key !== run.key)];
    const kept = all.filter((entry, position) => position < KEEP_RUNS && run.at - entry.at < KEEP_MS);
    for (const entry of all) {
      if (kept.includes(entry) || !/^[0-9a-f]{16}$/.test(String(entry.key))) continue;
      try {
        fs.unlinkSync(path.join(dir, RUNS, `${entry.key}.json`));
      } catch {
        /* gone already */
      }
    }
    writeAtomic(path.join(dir, INDEX), JSON.stringify({ version: 1, runs: kept }));
  });
}

/** For the hook's own failures: apart from the runs, bounded, never thrown from. */
function logError(dir, message) {
  try {
    fs.mkdirSync(dir, { recursive: true });
    const file = path.join(dir, "errors.log");
    let size = 0;
    try {
      size = fs.statSync(file).size;
    } catch {
      /* none yet */
    }
    if (size > 32 * 1024) fs.writeFileSync(file, "");
    fs.appendFileSync(file, `${new Date().toISOString()} ${String(message).slice(0, 400)}\n`);
  } catch {
    /* nowhere to say it */
  }
}

module.exports = { INDEX, RUNS, KEEP_RUNS, KEEP_MS, dataFolder, runKey, saveRun, readJson, logError };
