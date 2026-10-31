#!/usr/bin/env node
"use strict";
// Lists the TODO, FIXME, HACK and XXX markers of the current folder, as one JSON document on stdout.
//
//   node scan.js                      every marker of the folder, capped
//   node scan.js --blame=<file>       the markers of one file, each with its author and age by `git blame`
//
// Pure Node, read-only: it opens files to read them and runs `git blame` for one file when asked;
// it never writes, installs or follows a link. A marker is a comment that *starts* with the tag
// (`// TODO: …`, `# FIXME(ana) …`, `/* HACK … */`, a line inside a block comment), so the word in
// prose, in a string or in a name (`todo`, `TODOS`, `todo!()`) is not one.
const fs = require("node:fs");
const path = require("node:path");
const { spawnSync } = require("node:child_process");

const TAGS = ["TODO", "FIXME", "HACK", "XXX"];
const ALWAYS_SKIPPED = new Set([".git", "node_modules", "target", "dist"]);
const MAX_ITEMS = 1000; // markers listed
const MAX_COUNTED = 20000; // markers counted before the walk stops
const MAX_FILES = 50000;
const MAX_FILE_BYTES = 1024 * 1024;
const MAX_LINE = 2000; // a longer line is minified code, not a note
const MAX_TEXT = 200;
const BUDGET_MS = 40000;

// ---- Which files, and how their comments are written -------------------------------------------

const C_LIKE = { line: ["//"], block: [["/*", "*/"]], strings: ['"', "'", "`"] };
const TYPED = { line: ["//"], block: [["/*", "*/"]], strings: ['"', "'"], charOnly: ["'"] }; // 'x' is a char, 'a a lifetime
const GO = { ...TYPED, strings: ['"', "'", "`"] };
const HASH = { line: ["#"], block: [], strings: ['"', "'"] };
const MARKUP = { line: [], block: [["<!--", "-->"]], strings: [] };
const COMPONENT = { line: ["//"], block: [["<!--", "-->"], ["/*", "*/"]], strings: [] };

const FAMILIES = {
  js: C_LIKE,
  typed: TYPED,
  go: GO,
  php: { line: ["//", "#"], block: [["/*", "*/"]], strings: ['"', "'"] },
  css: { line: [], block: [["/*", "*/"]], strings: ['"', "'"] },
  scss: { line: ["//"], block: [["/*", "*/"]], strings: ['"', "'"] },
  hash: HASH,
  python: { ...HASH, triple: true },
  ini: { line: ["#", ";"], block: [], strings: ['"'] },
  powershell: { line: ["#"], block: [["<#", "#>"]], strings: ['"', "'"] },
  verse: { line: ["#"], block: [["<#", "#>"]], strings: ['"'] },
  julia: { line: ["#"], block: [["#=", "=#"]], strings: ['"'] },
  sql: { line: ["--"], block: [["/*", "*/"]], strings: ["'", '"'] },
  lua: { line: ["--"], block: [["--[[", "]]"]], strings: ['"', "'"] },
  haskell: { line: ["--"], block: [["{-", "-}"]], strings: ['"'] },
  markup: MARKUP,
  component: COMPONENT,
  tex: { line: ["%"], block: [], strings: [] },
  lisp: { line: [";"], block: [], strings: ['"'] },
};

const BY_EXTENSION = {};
const assign = (family, extensions) => extensions.split(" ").forEach((ext) => (BY_EXTENSION[`.${ext}`] = family));
assign("js", "js jsx mjs cjs ts tsx mts cts jsonc json5");
assign("typed", "rs c h cc cpp cxx hpp hh cs java kt kts swift scala dart zig groovy gradle proto m mm");
assign("go", "go");
assign("php", "php");
assign("css", "css");
assign("scss", "scss less styl");
assign("hash", "rb sh bash zsh fish yml yaml toml r pl pm ex exs tf hcl cmake mk nim cr coffee gitignore dockerignore");
assign("python", "py pyw pyi");
assign("ini", "ini cfg conf properties");
assign("powershell", "ps1 psm1 psd1");
assign("verse", "verse");
assign("julia", "jl");
assign("sql", "sql");
assign("lua", "lua");
assign("haskell", "hs elm");
assign("markup", "html htm xhtml xml svg md mdx");
assign("component", "vue svelte astro");
assign("tex", "tex sty bib");
assign("lisp", "clj cljs el lisp scm");

const BY_NAME = { Dockerfile: "hash", Makefile: "hash", Gemfile: "hash", Rakefile: "hash", "CMakeLists.txt": "hash", ".gitignore": "hash", ".dockerignore": "hash" };

const familyOf = (name) => FAMILIES[BY_NAME[name] || BY_EXTENSION[path.extname(name).toLowerCase()]] || null;

// ---- Finding the markers -----------------------------------------------------------------------

// A comment's own text starts with the tag, after the decoration comments carry (`*`, `/`, `#`, `!`, `-`).
const MARKER = /^[\s*#/!<>=_.~-]*?(TODO|FIXME|HACK|XXX)(?![A-Za-z0-9_])(?:\([^)\n]*\))?(?:[:\s-]|$)/;
const CHAR_LITERAL = /^'(?:\\[^']*|[^\\'])'/;
const clip = (value) => (value.length > MAX_TEXT ? `${value.slice(0, MAX_TEXT - 1)}…` : value);

/** The marker a comment's text opens with: `{ tag, text }`, or null. */
function markerIn(comment) {
  const found = MARKER.exec(comment);
  if (!found) return null;
  const tag = found[1];
  const after = comment
    .slice(found[0].length)
    .replace(/^[\s:-]+/, "")
    .replace(/(?:\*\/|-->|#>|=#|\]\]|-\})\s*$/, "")
    .trim();
  return { tag, text: clip(after) };
}

/**
 * The markers of a source text written in `family` (see FAMILIES): [{ line, tag, text }], by line.
 * Strings are skipped so that `"// TODO"` is not a comment; a block comment is followed over its lines.
 */
function findMarkers(source, family) {
  const found = [];
  const lines = source.split(/\r\n|\n|\r/);
  let block = null; // the closer of the block comment we are in
  let template = null; // the quote of a string running over several lines
  let triple = null;

  const take = (comment, lineNo) => {
    const marker = markerIn(comment);
    if (marker) found.push({ line: lineNo, ...marker });
  };

  for (let index = 0; index < lines.length; index++) {
    const line = lines[index];
    const lineNo = index + 1;
    if (line.length > MAX_LINE) continue;
    let i = 0;
    if (block) {
      const end = line.indexOf(block);
      take(end === -1 ? line : line.slice(0, end), lineNo);
      if (end === -1) continue;
      i = end + block.length;
      block = null;
    }
    while (i < line.length) {
      if (triple) {
        const end = line.indexOf(triple, i);
        if (end === -1) break;
        i = end + 3;
        triple = null;
        continue;
      }
      if (template) {
        let j = i;
        while (j < line.length && line[j] !== template) j += line[j] === "\\" ? 2 : 1;
        if (j >= line.length) break;
        i = j + 1;
        template = null;
        continue;
      }
      const rest = line.slice(i);
      const opener = family.block.find(([open]) => rest.startsWith(open));
      if (opener) {
        const end = line.indexOf(opener[1], i + opener[0].length);
        take(line.slice(i + opener[0].length, end === -1 ? line.length : end), lineNo);
        if (end === -1) {
          block = opener[1];
          break;
        }
        i = end + opener[1].length;
        continue;
      }
      const lineOpener = family.line.find((open) => rest.startsWith(open));
      if (lineOpener && (lineOpener.length > 1 || lineOpener === "//" || i === 0 || /\s/.test(line[i - 1]))) {
        take(line.slice(i + lineOpener.length), lineNo);
        break;
      }
      const quote = line[i];
      if (family.strings.includes(quote)) {
        if (family.triple && (rest.startsWith('"""') || rest.startsWith("'''"))) {
          triple = rest.slice(0, 3);
          i += 3;
          continue;
        }
        if (family.charOnly && family.charOnly.includes(quote)) {
          const literal = CHAR_LITERAL.exec(rest);
          i += literal ? literal[0].length : 1;
          continue;
        }
        let j = i + 1;
        while (j < line.length && line[j] !== quote) j += line[j] === "\\" ? 2 : 1;
        if (j >= line.length) {
          if (quote === "`") template = "`"; // runs on to the next line
          break;
        }
        i = j + 1;
        continue;
      }
      i++;
    }
  }
  return found;
}

// ---- What .gitignore leaves out ----------------------------------------------------------------

function globToRegex(glob) {
  let out = "";
  for (let i = 0; i < glob.length; i++) {
    const c = glob[i];
    if (c === "*") {
      if (glob[i + 1] === "*") {
        const slashAfter = glob[i + 2] === "/";
        const slashBefore = i === 0 || glob[i - 1] === "/";
        if (slashBefore && slashAfter) {
          out += "(?:.*/)?";
          i += 2;
        } else if (slashBefore && i + 2 === glob.length) {
          out += ".*";
          i += 1;
        } else {
          out += "[^/]*";
          i += 1;
        }
      } else out += "[^/]*";
    } else if (c === "?") out += "[^/]";
    else if (c === "[") {
      const end = glob.indexOf("]", i + 2);
      if (end === -1) out += "\\[";
      else {
        out += `[${glob.slice(i + 1, end).replace(/^!/, "^").replace(/\\/g, "\\\\")}]`;
        i = end;
      }
    } else if (c === "\\" && i + 1 < glob.length) out += glob[++i].replace(/[.*+?^${}()|[\]\\/]/g, "\\$&");
    else out += c.replace(/[.*+?^${}()|[\]\\/]/g, "\\$&");
  }
  return out;
}

/** The rules of one .gitignore text, for the folder `base` ("" or "src/"), in file order. */
function parseIgnore(text, base) {
  const rules = [];
  for (let raw of text.split(/\r?\n/)) {
    if (!raw.trim() || raw.startsWith("#")) continue;
    raw = raw.replace(/(?<!\\)\s+$/, "");
    let negate = false;
    if (raw.startsWith("!")) {
      negate = true;
      raw = raw.slice(1);
    } else if (raw.startsWith("\\!") || raw.startsWith("\\#")) raw = raw.slice(1);
    let dirOnly = false;
    if (raw.endsWith("/")) {
      dirOnly = true;
      raw = raw.slice(0, -1);
    }
    if (!raw) continue;
    const anchored = raw.includes("/");
    if (raw.startsWith("/")) raw = raw.slice(1);
    let regex;
    try {
      regex = new RegExp(`^${anchored ? "" : "(?:.*/)?"}${globToRegex(raw)}$`);
    } catch {
      continue;
    }
    rules.push({ base, negate, dirOnly, regex });
  }
  return rules;
}

/** Whether the rules leave `relative` out (a posix path from the scanned folder); the last rule that matches decides. */
function ignored(rules, relative, isDir) {
  let result = false;
  for (const rule of rules) {
    if (rule.dirOnly && !isDir) continue;
    if (!relative.startsWith(rule.base)) continue;
    if (rule.regex.test(relative.slice(rule.base.length))) result = !rule.negate;
  }
  return result;
}

// ---- The walk ----------------------------------------------------------------------------------

function readText(file) {
  let buffer;
  try {
    buffer = fs.readFileSync(file);
  } catch {
    return null;
  }
  if (buffer.subarray(0, 8000).includes(0)) return null; // binary
  return buffer.toString("utf8").replace(/^﻿/, "");
}

/** Scans `root`. `limit` caps the markers listed; the counts go on past it. */
function scan(root, { limit = MAX_ITEMS, counted = MAX_COUNTED, now = Date.now } = {}) {
  const started = now();
  const items = [];
  const counts = Object.fromEntries(TAGS.map((tag) => [tag, 0]));
  let total = 0;
  let files = 0;
  let stopped = false; // the walk was cut short: a budget, not the end of the folder

  const walk = (dir, relativeDir, inherited) => {
    if (stopped) return;
    let rules = inherited;
    const ignoreText = readText(path.join(dir, ".gitignore"));
    if (ignoreText) rules = inherited.concat(parseIgnore(ignoreText, relativeDir));
    let entries;
    try {
      entries = fs.readdirSync(dir, { withFileTypes: true });
    } catch {
      return;
    }
    entries.sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
    for (const entry of entries) {
      if (stopped) return;
      if (entry.isSymbolicLink()) continue;
      const relative = relativeDir + entry.name;
      if (entry.isDirectory()) {
        if (ALWAYS_SKIPPED.has(entry.name) || ignored(rules, relative, true)) continue;
        walk(path.join(dir, entry.name), `${relative}/`, rules);
      } else if (entry.isFile()) {
        const family = familyOf(entry.name);
        if (!family || ignored(rules, relative, false)) continue;
        if (++files > MAX_FILES || now() - started > BUDGET_MS || total >= counted) {
          stopped = true;
          return;
        }
        const full = path.join(dir, entry.name);
        let size;
        try {
          size = fs.statSync(full).size;
        } catch {
          continue;
        }
        if (size > MAX_FILE_BYTES) continue;
        const source = readText(full);
        if (source === null) continue;
        for (const marker of findMarkers(source, family)) {
          total++;
          counts[marker.tag]++;
          if (items.length < limit) items.push({ file: relative, line: marker.line, tag: marker.tag, text: marker.text });
        }
      }
    }
  };
  walk(root, "", []);

  return { schema: 1, at: now(), total, counts, truncated: total > items.length, incomplete: stopped, files, items };
}

// ---- Author and age, for one file --------------------------------------------------------------

/** `git blame --line-porcelain` text → Map(line → { author, at }) (at in milliseconds; null when not committed yet). */
function parseBlame(text) {
  const byLine = new Map();
  let line = 0;
  let author = null;
  let time = null;
  for (const row of text.split("\n")) {
    const head = /^[0-9a-f]{40} \d+ (\d+)/.exec(row);
    if (head) {
      line = Number(head[1]);
      author = null;
      time = null;
    } else if (row.startsWith("author ")) author = row.slice(7);
    else if (row.startsWith("author-time ")) time = Number(row.slice(12)) * 1000;
    else if (row.startsWith("\t")) {
      const notYet = author === "Not Committed Yet" || /^0{40}/.test(row);
      byLine.set(line, notYet ? { author: null, at: null } : { author: author ? author.slice(0, 100) : null, at: Number.isFinite(time) ? time : null });
    }
  }
  return byLine;
}

/** A path from the folder, as `--blame` takes it: inside the folder, a file, no way out. Null when it is not one. */
function safeRelative(value) {
  if (typeof value !== "string" || !value || value.length > 1024 || value.includes("\0")) return null;
  const parts = value.replace(/\\/g, "/").split("/");
  if (path.isAbsolute(value) || /^[A-Za-z]:/.test(value) || parts.some((part) => part === ".." || part === "")) return null;
  return parts.filter((part) => part !== ".").join("/") || null;
}

/** The markers of one file with their author and age: status "ok", "untracked" (git does not know the file), "no-git" or "error". */
function blame(root, value, { run = spawnSync } = {}) {
  const file = safeRelative(value);
  const answer = (status, items = [], file_ = file) => ({ schema: 1, file: file_, status, items });
  if (!file) return answer("error", [], null);
  const full = path.join(root, file);
  const family = familyOf(path.basename(file));
  let stat;
  try {
    stat = fs.lstatSync(full);
  } catch {
    return answer("error");
  }
  if (!stat.isFile() || stat.size > MAX_FILE_BYTES || !family) return answer("error");
  const source = readText(full);
  if (source === null) return answer("error");
  const markers = findMarkers(source, family);
  const plain = markers.map((marker) => ({ line: marker.line, tag: marker.tag, text: marker.text, author: null, at: null }));
  if (!markers.length) return answer("ok");

  const git = run("git", ["blame", "--line-porcelain", "--", file], { cwd: root, encoding: "utf8", timeout: 20000, maxBuffer: 32 * 1024 * 1024, windowsHide: true });
  if (git.error) return answer(git.error.code === "ENOENT" ? "no-git" : "error", plain);
  if (git.status !== 0) {
    const message = String(git.stderr || "");
    if (/not a git repository/i.test(message)) return answer("no-git", plain);
    if (/no such path|not.*(?:tracked|in HEAD)|bad revision|does not have any commits|no commits/i.test(message)) return answer("untracked", plain);
    return answer("error", plain);
  }
  const byLine = parseBlame(String(git.stdout));
  return answer(
    "ok",
    plain.map((item) => ({ ...item, ...(byLine.get(item.line) || {}) })),
  );
}

// ---- Entry -------------------------------------------------------------------------------------

function main(argv) {
  const root = process.cwd();
  const flag = argv.find((arg) => arg.startsWith("--blame="));
  const file = flag ? flag.slice("--blame=".length) : "";
  const limit = Number((argv.find((arg) => arg.startsWith("--limit=")) || "").slice(8));
  const document = file ? blame(root, file) : scan(root, Number.isInteger(limit) && limit > 0 ? { limit } : {});
  process.stdout.write(`${JSON.stringify(document)}\n`);
}

if (require.main === module) main(process.argv.slice(2));

module.exports = { scan, blame, findMarkers, markerIn, parseIgnore, ignored, parseBlame, safeRelative, familyOf, FAMILIES, TAGS, MAX_ITEMS };
