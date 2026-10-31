"use strict";
// node:test (`node --test`), in its spec reporter (a terminal's default, and a pipe's since
// Node 20) and in TAP.
const { splitLines, messageOf, failure, locate, capFailures, clip } = require("./common.js");

const COUNT = /^(?:ℹ|#) (tests|suites|pass|fail|cancelled|skipped|todo|duration_ms) ([\d.]+)\s*$/;
const FAILING = /^✖ failing tests:\s*$/;
const AT = /^test at (.+?)\s*$/;
const MARK = /^\s*✖ (.+?)(?: \([\d.]+m?s\))?\s*$/;

function counts(lines) {
  const found = {};
  for (const line of lines) {
    const match = COUNT.exec(line);
    if (match) found[match[1]] = Number(match[2]);
  }
  // A summary cut short (after `pass`, before `fail`) would read as a pass: all of it or none.
  const needed = ["tests", "pass", "fail", "cancelled", "skipped", "todo"];
  return needed.every((name) => Number.isFinite(found[name])) ? found : null;
}

/** The spec reporter's "failing tests:" section: "test at file:line:col", "✖ name (time)", then the error. */
function specFailures(lines) {
  const start = lines.findIndex((line) => FAILING.test(line));
  if (start < 0) return [];
  const failures = [];
  for (let i = start + 1; i < lines.length; i++) {
    const at = AT.exec(lines[i]);
    if (!at) continue;
    const where = locate(at[1]);
    let name = at[1];
    const body = [];
    i += 1;
    if (i < lines.length) {
      const mark = MARK.exec(lines[i]);
      if (mark) name = mark[1];
      i += 1;
    }
    for (; i < lines.length && !AT.test(lines[i]); i++) {
      if (/^\s+at\s/.test(lines[i])) {
        // The stack, then the error's own properties: neither is the message.
        while (i + 1 < lines.length && !AT.test(lines[i + 1])) i++;
        break;
      }
      body.push(lines[i]);
    }
    i -= 1;
    failures.push(failure(name, messageOf(body), where ? where.file : undefined, where ? where.line : undefined));
  }
  return failures;
}

function unquote(value) {
  const text = value.trim();
  if (text.startsWith("'") && text.endsWith("'")) return text.slice(1, -1).replace(/''/g, "'").replace(/\\\\/g, "\\");
  if (text.startsWith('"') && text.endsWith('"')) return text.slice(1, -1).replace(/\\n/g, "\n").replace(/\\"/g, '"').replace(/\\\\/g, "\\");
  return text;
}

/** TAP: "not ok 2 - name" and its YAML block, between "---" and "...". */
function tapFailures(lines) {
  const failures = [];
  for (let i = 0; i < lines.length; i++) {
    const head = /^(\s*)not ok \d+ - (.+?)\s*$/.exec(lines[i]);
    if (!head || /# (?:SKIP|TODO)/.test(head[2])) continue;
    const indent = head[1].length;
    const yaml = [];
    if (lines[i + 1] && lines[i + 1].trim() === "---") {
      for (i += 2; i < lines.length && lines[i].trim() !== "..."; i++) yaml.push(lines[i].slice(indent + 2));
    }
    const field = (name) => {
      const at = yaml.findIndex((row) => row.startsWith(`${name}:`));
      if (at < 0) return null;
      const rest = yaml[at].slice(name.length + 1).trim();
      if (/^[|>][+-]?$/.test(rest)) {
        const block = [];
        for (let j = at + 1; j < yaml.length && (yaml[j].startsWith("  ") || !yaml[j].trim()); j++) block.push(yaml[j].slice(2));
        return block.join("\n");
      }
      return unquote(rest);
    };
    if (field("failureType") === "subtestsFailed") continue; // its subtests are the failures
    const where = locate(field("location") || "");
    failures.push(failure(head[2], messageOf((field("error") || field("failureType") || "").split("\n")), where ? where.file : undefined, where ? where.line : undefined));
  }
  return failures;
}

function parse(lines) {
  const found = counts(lines);
  let failures = lines.some((line) => /^\s*not ok \d+ - /.test(line)) ? tapFailures(lines) : specFailures(lines);

  if (!found) {
    if (!failures.length) {
      // A run cut before its summary: the tree still marks what failed.
      const marks = lines.filter((line) => MARK.test(line) && !FAILING.test(line)).map((line) => MARK.exec(line)[1]);
      if (!marks.length) return null;
      failures = [...new Set(marks)].map((name) => failure(clip(name), ""));
    }
    return { framework: "node", passed: null, failed: failures.length, skipped: null, duration: null, failures: capFailures(failures), partial: true };
  }
  return {
    framework: "node",
    passed: found.pass,
    failed: (found.fail || 0) + (found.cancelled || 0),
    skipped: (found.skipped || 0) + (found.todo || 0),
    duration: Number.isFinite(found.duration_ms) ? Math.round(found.duration_ms) : null,
    failures: capFailures(failures),
    partial: false,
  };
}

module.exports = { framework: "node", parse: (text) => parse(splitLines(text)) };
