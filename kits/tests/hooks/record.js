"use strict";
// PostToolUse (and PostToolUseFailure, which is where a failing test run arrives: Claude Code
// reports a command that exits non-zero as a failed tool call) on Bash and PowerShell.
// When the command runs tests, it reads the output and keeps the result in the kit's data
// folder for the tab. It says nothing to the agent, and always exits 0.

const INPUT_MAX = 24 * 1024 * 1024;

let finished = false;
function finish() {
  if (finished) return;
  finished = true;
  process.exit(0);
}

function failQuietly(error) {
  try {
    const { dataFolder, logError } = require("./store.js");
    logError(dataFolder(process.env), (error && error.stack) || error);
  } catch {
    /* nowhere to say it */
  }
  finish();
}

process.on("uncaughtException", failQuietly);
process.on("unhandledRejection", failQuietly);
process.stdout.on("error", finish);

let input = "";
let size = 0;
process.stdin.setEncoding("utf8");
process.stdin.on("data", (chunk) => {
  size += chunk.length;
  if (size <= INPUT_MAX) input += chunk;
});
process.stdin.on("error", failQuietly);
process.stdin.on("end", () => {
  try {
    if (size > INPUT_MAX) return finish();
    const { recordFor } = require("./capture.js");
    const run = recordFor(JSON.parse(input), process.env, Date.now());
    if (run) {
      const { dataFolder, saveRun } = require("./store.js");
      saveRun(dataFolder(process.env), run);
    }
  } catch (error) {
    return failQuietly(error);
  }
  finish();
});
