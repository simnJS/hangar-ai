"use strict";
// The kit's data folder, as the hook sees it.
//
//   refusals.jsonl      one JSON object a line: { type: "refusal" | "pass-used", at, … }.
//                       Appended by every hook at once, read by the tab.
//   passes/<key>.json   a one-time pass, written by the tab, taken by the hook.
//                       One file each, so taking one is one atomic delete: two agents
//                       cannot both use it, and the tab's writes touch no other pass.
//   errors.jsonl        the guard's own failures. Apart, so that a broken override
//                       repeated at every call pushes no refusal out of the log.

const fs = require("node:fs");
const path = require("node:path");
const crypto = require("node:crypto");

const LOG = "refusals.jsonl";
const ERRORS = "errors.jsonl";
const PASSES = "passes";
const PASS_TTL_MS = 2 * 60 * 1000;
const CLOCK_MARGIN_MS = 5 * 1000;
// Hangar hands a tab 1 MB at most: past the first, the log keeps its newest half.
const LOG_MAX_BYTES = 512 * 1024;
const LOG_KEEP_BYTES = 256 * 1024;
const ERRORS_MAX_BYTES = 64 * 1024;
const ERRORS_KEEP_BYTES = 32 * 1024;
const COMMAND_MAX = 2000;
const SWEEP_AFTER_MS = 10 * 60 * 1000;

/** What a pass is for: this agent, this tool, this exact command or file. */
function passKey(agentId, tool, subject) {
  return crypto.createHash("sha256").update(`${agentId}\0${tool}\0${subject}`).digest("hex");
}

function trimLog(file, max, keep) {
  const { size } = fs.statSync(file);
  if (size <= max) return;
  const text = fs.readFileSync(file, "utf8");
  const tail = text.slice(-keep);
  const kept = tail.slice(tail.indexOf("\n") + 1);
  const temp = `${file}.${process.pid}.tmp`;
  fs.writeFileSync(temp, kept);
  fs.renameSync(temp, file);
}

function appendEvent(dir, event) {
  fs.mkdirSync(dir, { recursive: true });
  const file = path.join(dir, LOG);
  fs.appendFileSync(file, `${JSON.stringify(event)}\n`);
  trimLog(file, LOG_MAX_BYTES, LOG_KEEP_BYTES);
}

function appendError(dir, message, now) {
  fs.mkdirSync(dir, { recursive: true });
  const file = path.join(dir, ERRORS);
  fs.appendFileSync(file, `${JSON.stringify({ at: now, message: String(message).slice(0, 500) })}\n`);
  trimLog(file, ERRORS_MAX_BYTES, ERRORS_KEEP_BYTES);
}

/**
 * Takes the pass for this key, if a good one is there: for this agent, not expired,
 * and not set further ahead than a pass can be. A bad one is removed and worth nothing.
 */
function takePass(dir, key, agentId, now) {
  const file = path.join(dir, PASSES, `${key}.json`);
  let pass;
  try {
    pass = JSON.parse(fs.readFileSync(file, "utf8").replace(/^﻿/, ""));
  } catch (error) {
    if (error && error.code === "ENOENT") return false;
    pass = null;
  }
  const good =
    pass !== null &&
    typeof pass === "object" &&
    pass.key === key &&
    pass.agentId === agentId &&
    Number.isFinite(pass.expiresAt) &&
    pass.expiresAt > now &&
    pass.expiresAt <= now + PASS_TTL_MS + CLOCK_MARGIN_MS;
  try {
    fs.unlinkSync(file);
  } catch {
    return false; // another call took it first
  }
  return good;
}

/** Removes passes nobody came back for. */
function sweepPasses(dir, now) {
  const folder = path.join(dir, PASSES);
  let names;
  try {
    names = fs.readdirSync(folder);
  } catch {
    return;
  }
  for (const name of names) {
    const file = path.join(folder, name);
    try {
      if (now - fs.statSync(file).mtimeMs > SWEEP_AFTER_MS) fs.unlinkSync(file);
    } catch {
      /* gone already */
    }
  }
}

module.exports = { LOG, ERRORS, PASSES, PASS_TTL_MS, COMMAND_MAX, passKey, appendEvent, appendError, takePass, sweepPasses };
