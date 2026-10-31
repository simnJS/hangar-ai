#!/usr/bin/env node
"use strict";
// The kit's one declared command: `node {kitDir}/tools/audit.js`, run in the workspace folder.
//
// It finds the project's package managers from their lockfiles, runs each one's READ-ONLY audit
// and outdated commands, and prints one JSON document on stdout (see `run` for its shape). It
// never installs, never fixes, never updates and never writes a file: the commands it may run
// are the fixed list in `plan`, and a test fails if one of them is not on the read-only list.
//
// Exit code 0 whenever it printed a document, even for a project with nothing to audit or a tool
// that is not installed: those are answers. Anything else is a crash of this script.
const fs = require("node:fs");
const path = require("node:path");
const { spawn, spawnSync } = require("node:child_process");
const parsers = require("./parsers");

const STEP_TIMEOUT_MS = 90_000;
const OUTPUT_CAP = 8 * 1024 * 1024;
const LIST_CAP = 500;
const RANK = Object.fromEntries(parsers.SEVERITIES.map((name, index) => [name, index]));
const KIND_RANK = { major: 0, minor: 1, patch: 2 };

/** What a tool may be asked, per program: any other argument list is refused before it runs. */
const READ_ONLY = {
  pnpm: [["audit", "--json"], ["outdated", "--format", "json"]],
  npm: [["audit", "--json"], ["outdated", "--json"]],
  yarn: [["--version"], ["audit", "--json"], ["outdated", "--json"], ["npm", "audit", "--json"]],
  cargo: [["audit", "--json"], ["outdated", "--format", "json", "--workspace", "--exit-code", "0"]],
  "pip-audit": [["-r", "requirements.txt", "--no-deps", "--disable-pip", "-f", "json"]],
  pip: [["list", "--outdated", "--format", "json"]],
};

const isFile = (file) => {
  try {
    return fs.statSync(file).isFile();
  } catch {
    return false;
  }
};

/** A program on the PATH, by name: never from the project's folder, never from a relative entry. */
function which(name, env = process.env) {
  const win = process.platform === "win32";
  const dirs = String(env.PATH || env.Path || "").split(path.delimiter).filter((dir) => dir && path.isAbsolute(dir));
  const exts = win ? String(env.PATHEXT || ".COM;.EXE;.BAT;.CMD").split(";").filter(Boolean) : [""];
  for (const dir of dirs) {
    for (const ext of exts) {
      const file = path.join(dir, name + ext);
      if (isFile(file)) return file;
    }
  }
  return null;
}

/**
 * Runs `program args` in `cwd`: `{ missing: true }` when it is not installed, else
 * `{ code, stdout, stderr, timedOut }`. No shell reads the arguments; on Windows a `.cmd`
 * (npm, pnpm, yarn) can only start through cmd.exe, so its arguments must be plain words.
 */
function exec(program, args, { cwd, timeoutMs = STEP_TIMEOUT_MS, env = process.env } = {}) {
  const allowed = (READ_ONLY[program] || []).some((list) => list.length === args.length && list.every((item, index) => item === args[index]));
  if (!allowed) return Promise.reject(new Error(`refused: ${program} ${args.join(" ")} is not on the read-only list`));
  const file = which(program, env);
  if (!file) return Promise.resolve({ missing: true });
  const childEnv = { ...env, NO_COLOR: "1", FORCE_COLOR: "0", NO_UPDATE_NOTIFIER: "1", npm_config_update_notifier: "false", npm_config_fund: "false" };
  const batch = process.platform === "win32" && /\.(cmd|bat)$/i.test(file);
  const options = { cwd, env: childEnv, windowsHide: true, stdio: ["ignore", "pipe", "pipe"], detached: process.platform !== "win32" };
  let child;
  if (batch) {
    if (!args.every((arg) => /^[\w.=:@/-]+$/.test(arg))) throw new Error("refused: arguments must be plain words");
    child = spawn(env.ComSpec || "cmd.exe", ["/d", "/s", "/c", `""${file}" ${args.join(" ")}"`], { ...options, windowsVerbatimArguments: true });
  } else {
    child = spawn(file, args, options);
  }
  return new Promise((resolve) => {
    let stdout = "";
    let stderr = "";
    let timedOut = false;
    const take = (current, chunk) => (current.length < OUTPUT_CAP ? current + chunk : current);
    child.stdout.setEncoding("utf8").on("data", (chunk) => (stdout = take(stdout, chunk)));
    child.stderr.setEncoding("utf8").on("data", (chunk) => (stderr = take(stderr, chunk)));
    const timer = setTimeout(() => {
      timedOut = true;
      if (process.platform === "win32") spawnSync("taskkill", ["/pid", String(child.pid), "/t", "/f"], { windowsHide: true });
      else {
        try {
          process.kill(-child.pid, "SIGKILL");
        } catch {
          child.kill("SIGKILL");
        }
      }
    }, timeoutMs);
    const finish = (value) => {
      clearTimeout(timer);
      resolve(value);
    };
    child.on("error", (error) => finish(error.code === "ENOENT" ? { missing: true } : { code: null, stdout, stderr: String(error.message), timedOut }));
    child.on("close", (code) => finish({ code, stdout, stderr, timedOut }));
  });
}

const read = (cwd, name) => {
  try {
    return fs.readFileSync(path.join(cwd, name), "utf8");
  } catch {
    return null;
  }
};

/** The managers a folder holds a lockfile of: `[{ manager, lockfile }]`, JavaScript's one at most. */
function detect(cwd) {
  const found = [];
  const has = (name) => isFile(path.join(cwd, name));
  const javascript = [
    ["pnpm", "pnpm-lock.yaml"],
    ["yarn", "yarn.lock"],
    ["npm", "package-lock.json"],
    ["npm", "npm-shrinkwrap.json"],
  ].filter(([, lockfile]) => has(lockfile));
  if (javascript.length) {
    let declared = null;
    try {
      declared = /^(pnpm|yarn|npm)@/.exec(JSON.parse(read(cwd, "package.json") || "{}").packageManager || "");
    } catch {
      /* an unreadable package.json does not hide a lockfile */
    }
    const [manager, lockfile] = (declared && javascript.find(([name]) => name === declared[1])) || javascript[0];
    found.push({ manager, lockfile });
  }
  if (has("Cargo.lock")) found.push({ manager: "cargo", lockfile: "Cargo.lock" });
  if (has("requirements.txt")) found.push({ manager: "pip", lockfile: "requirements.txt" });
  return found;
}

/** The versions a package-lock.json holds, by package name. */
function lockedVersions(cwd, lockfile) {
  const installed = {};
  try {
    const lock = JSON.parse(read(cwd, lockfile));
    for (const [key, entry] of Object.entries(lock.packages || {})) {
      const name = key.split("node_modules/").pop();
      if (name && entry && entry.version && !(name in installed)) installed[name] = entry.version;
    }
  } catch {
    /* the audit still lists the packages, without their version */
  }
  return installed;
}

const lines = (text) => String(text || "").split(/\r?\n/).map((line) => line.trim()).filter(Boolean);

/** The tool a failed command says is absent (`cargo audit` without cargo-audit installed). */
function missingSubcommand(program, args, result) {
  if (program === "cargo" && result.code !== 0 && /no such (sub)?command/i.test(result.stderr || "")) return `cargo-${args[0]}`;
  return null;
}

async function step(runner, program, args, parse, cwd, emptyOk) {
  const result = await runner(program, args, { cwd, timeoutMs: STEP_TIMEOUT_MS });
  if (result.missing) return { state: "missing", tool: program };
  const absent = missingSubcommand(program, args, result);
  if (absent) return { state: "missing", tool: absent };
  if (result.timedOut) return { state: "failed", detail: "timed out" };
  if (emptyOk && result.code === 0 && !String(result.stdout).trim()) return { state: "ok", items: [] };
  try {
    return { state: "ok", items: parse(result.stdout) };
  } catch (error) {
    const shapeless = /^(no JSON|not an? )/.test(error.message);
    const detail = shapeless ? lines(result.stderr)[0] || lines(result.stdout)[0] || error.message : error.message;
    return { state: "failed", detail: String(detail).slice(0, 300) };
  }
}

/** The two steps of a manager: `{ audit, outdated }`, each `{ state, items | tool | detail }`. */
async function auditManager(runner, cwd, { manager, lockfile }) {
  if (manager === "npm") {
    const installed = lockedVersions(cwd, lockfile);
    return {
      audit: await step(runner, "npm", ["audit", "--json"], (text) => parsers.npmAudit(text, installed), cwd),
      outdated: await step(runner, "npm", ["outdated", "--json"], parsers.npmOutdated, cwd, true),
    };
  }
  if (manager === "pnpm") {
    return {
      audit: await step(runner, "pnpm", ["audit", "--json"], parsers.pnpmAudit, cwd),
      outdated: await step(runner, "pnpm", ["outdated", "--format", "json"], parsers.pnpmOutdated, cwd, true),
    };
  }
  if (manager === "yarn") {
    const version = await runner("yarn", ["--version"], { cwd, timeoutMs: STEP_TIMEOUT_MS });
    if (version.missing) return { audit: { state: "missing", tool: "yarn" }, outdated: { state: "missing", tool: "yarn" } };
    if (Number.parseInt(String(version.stdout).trim(), 10) >= 2) {
      // Berry has no `yarn outdated` of its own.
      return {
        audit: await step(runner, "yarn", ["npm", "audit", "--json"], parsers.yarnBerryAudit, cwd, true),
        outdated: { state: "unsupported", detail: "yarn 2 and later have no outdated command" },
      };
    }
    return {
      audit: await step(runner, "yarn", ["audit", "--json"], parsers.yarnAudit, cwd),
      outdated: await step(runner, "yarn", ["outdated", "--json"], parsers.yarnOutdated, cwd, true),
    };
  }
  if (manager === "cargo") {
    return {
      audit: await step(runner, "cargo", ["audit", "--json"], parsers.cargoAudit, cwd),
      outdated: await step(runner, "cargo", ["outdated", "--format", "json", "--workspace", "--exit-code", "0"], parsers.cargoOutdated, cwd),
    };
  }
  const names = parsers.requirementNames(read(cwd, lockfile));
  return {
    audit: await step(runner, "pip-audit", ["-r", "requirements.txt", "--no-deps", "--disable-pip", "-f", "json"], parsers.pipAudit, cwd),
    outdated: await step(runner, "pip", ["list", "--outdated", "--format", "json"], (text) => parsers.pipOutdated(text, names), cwd),
  };
}

const bySeverity = (a, b) => RANK[a.severity] - RANK[b.severity] || a.package.localeCompare(b.package) || String(a.id).localeCompare(String(b.id));
const byKind = (a, b) => KIND_RANK[a.kind] - KIND_RANK[b.kind] || a.package.localeCompare(b.package);

/**
 * The document: `{ schema: 1, at, status, counts, managers: [...] }`.
 * `status` is "no-lockfile" (nothing to audit), "tool-missing" (every manager's tool is absent),
 * or "ok". Each manager: `{ manager, lockfile, status: "ok" | "tool-missing" | "error", steps,
 * missing: [tool], errors: [{ step, detail }], vulnerabilities, outdated, truncated }`.
 * `runner` runs a program (tests hand it recorded outputs); `now` is the clock.
 */
async function run(cwd, { runner = exec, now = Date.now } = {}) {
  const managers = [];
  for (const entry of detect(cwd)) {
    const steps = await auditManager(runner, cwd, entry);
    const states = Object.values(steps).map((item) => item.state);
    const vulnerabilities = ((steps.audit.state === "ok" && steps.audit.items) || []).sort(bySeverity);
    const outdated = ((steps.outdated.state === "ok" && steps.outdated.items) || []).sort(byKind);
    const missing = [...new Set(Object.values(steps).filter((item) => item.state === "missing").map((item) => item.tool))];
    const errors = Object.entries(steps).filter(([, item]) => item.state === "failed").map(([name, item]) => ({ step: name, detail: item.detail }));
    const usable = states.filter((state) => state === "ok").length;
    managers.push({
      manager: entry.manager,
      lockfile: entry.lockfile,
      status: usable ? "ok" : missing.length && !errors.length ? "tool-missing" : "error",
      steps: Object.fromEntries(Object.entries(steps).map(([name, item]) => [name, item.state])),
      missing,
      errors,
      vulnerabilities: vulnerabilities.slice(0, LIST_CAP),
      outdated: outdated.slice(0, LIST_CAP),
      truncated: vulnerabilities.length > LIST_CAP || outdated.length > LIST_CAP,
    });
  }
  const counts = { vulnerabilities: { critical: 0, high: 0, moderate: 0, low: 0, unknown: 0 }, outdated: { major: 0, minor: 0, patch: 0 } };
  for (const item of managers) {
    for (const vuln of item.vulnerabilities) counts.vulnerabilities[vuln.severity]++;
    for (const pack of item.outdated) counts.outdated[pack.kind]++;
  }
  const status = !managers.length ? "no-lockfile" : managers.every((item) => item.status === "tool-missing") ? "tool-missing" : "ok";
  return { schema: 1, at: now(), status, counts, managers };
}

if (require.main === module) {
  run(process.cwd()).then(
    (result) => {
      process.stdout.write(`${JSON.stringify(result)}\n`);
    },
    (error) => {
      process.stderr.write(`${error && error.message}\n`);
      process.exitCode = 2;
    },
  );
}

module.exports = { run, detect, exec, which, READ_ONLY, STEP_TIMEOUT_MS };
