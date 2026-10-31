"use strict";
// What the rules need to know of the machine: the rules themselves with the
// project's overrides, the project's folders, the home folder, the git state.

const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const paths = require("./paths.js");

const KIT = path.join(__dirname, "..");

function isPlainObject(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

/**
 * Overrides on the kit's rules. A list adds to the kit's, anything else replaces
 * it, and what the kit does not know is ignored. `report(message)` hears what was
 * ignored: a mistyped override must not pass for one that works.
 */
function mergeRules(defaults, overrides, report = () => undefined) {
  const merged = JSON.parse(JSON.stringify(defaults));
  if (!isPlainObject(overrides)) {
    report("the overrides are not a JSON object");
    return merged;
  }
  for (const [name, change] of Object.entries(overrides)) {
    if (!merged[name] || !isPlainObject(change)) {
      report(`unknown rule ${JSON.stringify(name)}`);
      continue;
    }
    for (const [field, value] of Object.entries(change)) {
      const current = merged[name][field];
      if (current === undefined) {
        report(`unknown field ${name}.${field}`);
      } else if (Array.isArray(current)) {
        if (!Array.isArray(value)) {
          report(`${name}.${field} must be a list`);
          continue;
        }
        const fits = (item) => (Array.isArray(current[0]) ? Array.isArray(item) && item.length > 0 && item.every((word) => typeof word === "string") : typeof item === "string" && item !== "");
        if (!value.every(fits)) report(`${name}.${field} holds something that is not one of its entries`);
        merged[name][field] = [...current, ...value.filter(fits)];
      } else if (typeof value === typeof current) {
        merged[name][field] = value;
      } else {
        report(`${name}.${field} must be a ${typeof current}`);
      }
    }
  }
  return merged;
}

function readRules(project, report) {
  const defaults = JSON.parse(fs.readFileSync(path.join(KIT, "rules.json"), "utf8"));
  if (!project) return defaults;
  const file = path.join(project, ".hangar", "guard.json");
  let text;
  try {
    text = fs.readFileSync(file, "utf8");
  } catch {
    return defaults;
  }
  try {
    return mergeRules(defaults, JSON.parse(text.replace(/^﻿/, "")), (message) => report(`.hangar/guard.json: ${message}`));
  } catch (error) {
    // A broken file leaves the kit's rules: it must not turn the guard off.
    report(`.hangar/guard.json is not valid JSON: ${error.message}`);
    return defaults;
  }
}

/** The folder holding `.git` above `start`, with the git folder itself. */
function findRepository(start) {
  let dir = start;
  for (let hops = 0; hops < 64; hops++) {
    const candidate = path.join(dir, ".git");
    try {
      const stat = fs.statSync(candidate);
      if (stat.isDirectory()) return { top: dir, gitDir: candidate };
      const match = /^gitdir:\s*(.+?)\s*$/m.exec(fs.readFileSync(candidate, "utf8")); // a worktree
      if (match) return { top: dir, gitDir: path.resolve(dir, match[1]) };
    } catch {
      /* not here */
    }
    const parent = path.dirname(dir);
    if (parent === dir) return null;
    dir = parent;
  }
  return null;
}

function readRef(file, prefix) {
  try {
    const text = fs.readFileSync(file, "utf8").trim();
    const full = `ref: ${prefix}`;
    return text.startsWith(full) ? text.slice(full.length) : null;
  } catch {
    return null;
  }
}

function currentBranch(cwd) {
  const repository = findRepository(cwd);
  return repository ? readRef(path.join(repository.gitDir, "HEAD"), "refs/heads/") : null;
}

/** The branch origin/HEAD names: the remote's own default. */
function originHead(cwd) {
  const repository = findRepository(cwd);
  if (!repository) return null;
  let common = repository.gitDir;
  try {
    common = path.resolve(repository.gitDir, fs.readFileSync(path.join(repository.gitDir, "commondir"), "utf8").trim());
  } catch {
    /* not a worktree */
  }
  return readRef(path.join(common, "refs", "remotes", "origin", "HEAD"), "refs/remotes/origin/");
}

function dataFolder(env) {
  return env.HANGAR_KITS_DATA ? path.join(env.HANGAR_KITS_DATA, "guard") : path.join(os.homedir(), ".hangar", "kit-data", "guard");
}

/**
 * @param {object} event     Claude Code's PreToolUse event
 * @param {object} [options] what a test sets instead of this machine's:
 *   env, platform, home, tmp, rules, dataDir, pluginRoot, currentBranch, originHead, findTop, report
 */
function createContext(event, options = {}) {
  const env = options.env || process.env;
  const platform = options.platform || process.platform;
  const report = options.report || (() => undefined);
  const norm = (value, base = null) => paths.normalize(value, base, platform);

  const home = norm(options.home || env.HOME || env.USERPROFILE || os.homedir()) || "/";
  const cwdRaw = (typeof event.cwd === "string" && event.cwd) || env.CLAUDE_PROJECT_DIR || process.cwd();
  const cwd = norm(cwdRaw, home);
  const projectRaw = env.CLAUDE_PROJECT_DIR || cwdRaw;
  const project = norm(projectRaw, home);
  const findTop = options.findTop || ((dir) => {
    const repository = findRepository(dir);
    return repository ? repository.top : null;
  });

  // In a worktree the project's folder stays the main one and `cwd` follows the
  // agent: the folder it works in is the project just as well.
  const top = findTop(cwdRaw);
  const roots = [];
  for (const candidate of [project, top ? norm(top, home) : cwd]) {
    if (candidate && !paths.isFilesystemRoot(candidate) && !roots.some((root) => paths.same(root, candidate, platform))) roots.push(candidate);
  }

  const rules = options.rules || readRules(projectRaw, report);
  const tmp = norm(options.tmp || env.TMPDIR || env.TEMP || env.TMP || os.tmpdir(), home) || "/tmp";

  const allowedOutside = [];
  for (const entry of rules.recursiveDelete.allowedOutsideProject) {
    const variable = /^\$([A-Za-z_][A-Za-z0-9_]*)$/.exec(entry);
    const value = variable ? env[variable[1]] : entry.replace(/^~(?=\/|$)/, home);
    if (value) allowedOutside.push(norm(value, project));
  }
  allowedOutside.push(tmp);
  if (typeof event.scratchpad_dir === "string" && event.scratchpad_dir) allowedOutside.push(norm(event.scratchpad_dir, home));

  const dataDir = norm(options.dataDir || dataFolder(env), home);
  const pluginRootRaw = options.pluginRoot === undefined ? env.CLAUDE_PLUGIN_ROOT || KIT : options.pluginRoot;
  const pluginRoot = pluginRootRaw ? norm(pluginRootRaw, home) : null;
  // A kit being written inside the project is the project's to change.
  const kitInProject = pluginRoot && roots.some((root) => paths.same(pluginRoot, root, platform) || paths.isUnder(pluginRoot, root, platform));

  let sqlPattern;
  try {
    sqlPattern = new RegExp(rules.dropDatabase.sql, "i");
  } catch (error) {
    report(`dropDatabase.sql is not a regular expression: ${error.message}`);
    sqlPattern = /\bdrop\s+(database|schema)\b/i;
  }

  return {
    env,
    platform,
    home,
    tmp,
    cwd,
    project,
    roots,
    rules,
    allowedOutside,
    sqlPattern,
    guardedFolders: [dataDir, ...(pluginRoot && !kitInProject ? [pluginRoot] : [])],
    guardedFiles: roots.map((root) => `${root}/.hangar/guard.json`),
    currentBranch: options.currentBranch || currentBranch,
    originHead: options.originHead || originHead,
  };
}

module.exports = { createContext, mergeRules, dataFolder, KIT };
