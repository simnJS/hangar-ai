"use strict";
const { splitLines, messageOf, failure, toMs, locate, capFailures } = require("./common.js");

const FAIL_HEADER = /^\s*FAIL\s+(\S.*?)\s*$/;
const SEPARATOR = /^\s*⎯{2,}/;
const SUMMARY_FILES = /^\s*Test Files\s+(.+)$/;
const SUMMARY_TESTS = /^\s*Tests\s+(.+)$/;
const SUMMARY_DURATION = /^\s*Duration\s+(.+)$/;

function counts(text) {
  const out = { passed: 0, failed: 0, skipped: 0 };
  for (const match of text.matchAll(/(\d+)\s+(failed|passed|skipped|todo|pending)/g)) {
    const key = match[2] === "failed" ? "failed" : match[2] === "passed" ? "passed" : "skipped";
    out[key] += Number(match[1]);
  }
  return out;
}

function header(text) {
  const withoutProject = text.replace(/^\|[^|]*\|\s*/, "");
  const suite = /^(.*?)\s+\[\s*.*\]$/.exec(withoutProject); // "FAIL  a.test.js [ a.test.js ]": the file did not load
  if (suite) return { file: suite[1], name: suite[1] };
  const parts = withoutProject.split(" > ");
  const file = parts[0];
  return { file, name: parts.length > 1 ? parts.slice(1).join(" > ") : file };
}

function parse(lines) {
  let files = null;
  let tests = null;
  let duration = null;
  for (const line of lines) {
    let match;
    if ((match = SUMMARY_FILES.exec(line))) files = counts(match[1]);
    else if ((match = SUMMARY_TESTS.exec(line))) tests = /\d+\s+(failed|passed|skipped|todo)/.test(match[1]) ? counts(match[1]) : tests;
    else if ((match = SUMMARY_DURATION.exec(line))) duration = toMs(match[1].split("(")[0]);
  }

  const failures = [];
  for (let i = 0; i < lines.length; i++) {
    const match = FAIL_HEADER.exec(lines[i]);
    if (!match) continue;
    const head = header(match[1]);
    const body = [];
    let where = null;
    for (i += 1; i < lines.length && !FAIL_HEADER.test(lines[i]) && !SEPARATOR.test(lines[i]) && !SUMMARY_FILES.test(lines[i]); i++) {
      const stack = /^\s*❯\s+(\S+:\d+:\d+)\s*$/.exec(lines[i]);
      if (stack) {
        where = where || locate(stack[1]);
        break;
      }
      body.push(lines[i]);
    }
    while (i < lines.length && !FAIL_HEADER.test(lines[i]) && !SEPARATOR.test(lines[i]) && !SUMMARY_FILES.test(lines[i])) i++;
    i -= 1;
    failures.push(failure(head.name, messageOf(body), where ? where.file : head.file, where ? where.line : undefined));
  }

  const summary = tests || files;
  // Without its summary, only vitest's own marks make a FAIL line vitest's: jest and others print one too.
  const vitestMarks = lines.some((line) => /^\s*RUN\s+v\d/.test(line) || SEPARATOR.test(line));
  if (!summary && !(failures.length && vitestMarks)) return null;
  if (!summary) return { framework: "vitest", passed: null, failed: failures.length, skipped: null, duration, failures: capFailures(failures), partial: true };
  // A file that did not load has no test to count: the line of files says it.
  const useFiles = files && !tests && files.failed > 0;
  const total = useFiles ? files : summary;
  return { framework: "vitest", passed: total.passed, failed: total.failed, skipped: total.skipped, duration, failures: capFailures(failures), partial: false };
}

module.exports = { framework: "vitest", parse: (text) => parse(splitLines(text)) };
