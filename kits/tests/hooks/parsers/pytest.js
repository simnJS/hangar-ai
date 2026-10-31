"use strict";
const { splitLines, messageOf, failure, capFailures, clip } = require("./common.js");

const ITEM = "\\d+ (?:passed|failed|skipped|errors?|xfailed|xpassed|warnings?|deselected|rerun)";
const FINAL = new RegExp(`^(?:=+\\s*)?((?:${ITEM})(?:,\\s*${ITEM})*|no tests ran)\\s+in\\s+([\\d.]+)s\\b`);
const SHORT = /^(FAILED|ERROR)\s+(\S+::\S.*?)(?:\s+-\s+(.*))?$/;
const BLOCK = /^_{3,}\s+(.+?)\s+_{3,}$/;
const SECTION = /^={3,}\s+(.+?)\s+={3,}$/;

function parse(lines) {
  let counts = null;
  let duration = null;
  for (const line of lines) {
    const match = FINAL.exec(line.trim());
    if (!match) continue;
    counts = { passed: 0, failed: 0, skipped: 0 };
    for (const item of match[1].matchAll(/(\d+) (passed|failed|skipped|errors?|xfailed|xpassed)/g)) {
      const word = item[2];
      if (word === "passed" || word === "xpassed") counts.passed += Number(item[1]);
      else if (word === "failed" || word.startsWith("error")) counts.failed += Number(item[1]);
      else counts.skipped += Number(item[1]);
    }
    duration = Math.round(Number(match[2]) * 1000);
  }

  // The blocks under FAILURES and ERRORS: a header, then the code and the lines "E   …".
  const blocks = [];
  let inside = false;
  let current = null;
  for (const line of lines) {
    const section = SECTION.exec(line.trim());
    if (section) {
      inside = /^(FAILURES|ERRORS)$/.test(section[1]);
      current = null;
      continue;
    }
    if (!inside) continue;
    const block = BLOCK.exec(line.trim());
    if (block) {
      current = { name: block[1], message: [], file: null, line: null };
      blocks.push(current);
    } else if (current) {
      const message = /^E {1,4}(.*)$/.exec(line);
      if (message) current.message.push(message[1]);
      else {
        const where = /^(\S+\.\w+):(\d+): /.exec(line);
        if (where) {
          current.file = where[1];
          current.line = Number(where[2]);
        }
      }
    }
  }
  const blockFor = (nodeId) => {
    const path = nodeId.split("::").slice(1);
    const dotted = path.join(".");
    return blocks.find((block) => block.name === dotted || block.name === path[path.length - 1] || block.name === nodeId);
  };

  const failures = [];
  for (const line of lines) {
    const match = SHORT.exec(line.trim());
    if (!match) continue;
    const nodeId = match[2];
    const block = blockFor(nodeId);
    const message = block && block.message.length ? messageOf(block.message) : match[3] || "";
    failures.push(failure(nodeId, message, block && block.file ? block.file : nodeId.split("::")[0], block && block.line ? block.line : undefined));
  }
  if (!failures.length) {
    for (const block of blocks) failures.push(failure(clip(block.name), messageOf(block.message), block.file || undefined, block.line || undefined));
  }

  if (!counts && !failures.length) return null;
  if (!counts) return { framework: "pytest", passed: null, failed: failures.length, skipped: null, duration: null, failures: capFailures(failures), partial: true };
  return { framework: "pytest", passed: counts.passed, failed: counts.failed, skipped: counts.skipped, duration, failures: capFailures(failures), partial: false };
}

module.exports = { framework: "pytest", parse: (text) => parse(splitLines(text)) };
