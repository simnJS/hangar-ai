"use strict";
// Does this command line run tests, and with which framework? Read from the command, not
// from its output: a hook that parsed every Bash call's output would be slow, and wrong on
// anything that merely prints the word "failed".
//
// Only the start of each command counts (`git commit -m "fix vitest"` does not run vitest),
// after what a command is often wrapped in: `npx`, `pnpm exec`, `uv run`, `python -m`...

const WRAPPERS = [
  /^&\s+/, // PowerShell's call operator
  /^(?:call|time|command|nice|sudo|exec)\s+/i,
  /^env\s+(?:-\S+\s+)*/,
  /^(?:[A-Za-z_][A-Za-z0-9_]*=\S*\s+)+/, // FOO=1 BAR=2 command
  /^(?:cross-env|dotenv(?:x)?)\s+(?:(?:[A-Za-z_][A-Za-z0-9_]*=\S*|--)\s+)*/,
  /^(?:npx|pnpx|bunx)\s+(?:-{1,2}[\w-]+\s+)*/,
  /^(?:pnpm|yarn|bun)\s+(?:exec|dlx)\s+/,
  /^(?:pnpm|yarn)\s+(?=(?:vitest|jest)(?:\s|$))/, // pnpm vitest run: the package manager runs the binary
  /^npm\s+exec\s+(?:--\s+)?/,
  /^(?:uv|poetry|pipenv|hatch|pdm)\s+run\s+/,
  /^(?:python3?|py)(?:\s+-\d(?:\.\d+)?)?\s+-m\s+/,
  /^xvfb-run\s+(?:-\S+\s+)*/,
];

const PACKAGE_MANAGERS = new Set(["pnpm", "npm", "yarn", "bun"]);
const FLAGS_WITH_VALUE = new Set(["--filter", "-F", "-C", "--dir", "--prefix", "-w", "--workspace", "--cwd"]);
const TEST_SCRIPT = /^(?:test|t|tests?:[\w:.-]+|test:[\w:.-]+)$/;

/** Quotes emptied, so that a quoted `&&` or a quoted word splits and matches nothing. */
const withoutQuotes = (command) => command.replace(/"(?:[^"\\]|\\.)*"|'[^']*'/g, '""');

function segmentsOf(command) {
  return withoutQuotes(command)
    .split(/&&|\|\||[;|\n&]/)
    .map((part) => part.trim())
    .filter(Boolean);
}

function unwrap(segment) {
  let text = segment;
  for (let guard = 0; guard < 8; guard++) {
    const before = text;
    for (const wrapper of WRAPPERS) text = text.replace(wrapper, "");
    if (text === before) break;
  }
  return text;
}

/** The test script of `pnpm -r --filter x run test:unit -- --flag`, or null. */
function packageScript(text) {
  const tokens = text.split(/\s+/);
  if (!PACKAGE_MANAGERS.has(tokens[0])) return null;
  let i = 1;
  while (i < tokens.length && tokens[i].startsWith("-")) i += FLAGS_WITH_VALUE.has(tokens[i]) ? 2 : 1;
  if (tokens[i] === "run" || tokens[i] === "run-script") i += 1;
  return tokens[i] && TEST_SCRIPT.test(tokens[i]) ? tokens[i] : null;
}

const DIRECT = [
  ["vitest", /^vitest\b/],
  ["jest", /^jest\b/],
  ["pytest", /^(?:pytest|py\.test)\b/],
  ["cargo", /^cargo(?:\s+\+\S+)?\s+test\b/],
  ["go", /^go\s+test\b/],
  ["node", /^node(?:\.exe)?(?:\s+\S+)*?\s--test(?=\s|$)/],
];

const NOT_A_RUN = /\s(?:--help|-h|--version|-V)(?:\s|$)/;

/**
 * `{ frameworks }` for a command that runs tests: the frameworks it names, or `[]` when it
 * runs a package script (`pnpm test`) whose framework only the output tells. Null for
 * anything else.
 */
function detect(command) {
  if (typeof command !== "string" || !command.trim() || command.length > 20000) return null;
  const named = new Set();
  let generic = false;
  for (const segment of segmentsOf(command)) {
    const text = unwrap(segment);
    if (NOT_A_RUN.test(` ${text} `)) continue;
    const direct = DIRECT.find(([, pattern]) => pattern.test(text));
    if (direct) named.add(direct[0]);
    else if (packageScript(text)) generic = true;
  }
  if (generic) return { frameworks: [] };
  return named.size ? { frameworks: [...named] } : null;
}

module.exports = { detect, segmentsOf };
