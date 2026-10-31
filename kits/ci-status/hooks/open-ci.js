"use strict";
// PostToolUse hook on Bash. After a `git push` it starts, detached, the search
// for the CI run the push started (find-run.js), and returns at once: the agent
// never waits for it. Whatever goes wrong, it exits 0 and says nothing.
process.on("uncaughtException", () => process.exit(0));

const { spawn } = require("node:child_process");
const path = require("node:path");
const { isGitPush } = require("./lib.js");

let input = "";
process.stdin.setEncoding("utf8");
process.stdin.on("data", (chunk) => (input += chunk));
process.stdin.on("error", () => process.exit(0));
process.stdin.on("end", () => {
  try {
    const event = JSON.parse(input);
    const { HANGAR_API_URL, HANGAR_API_TOKEN, HANGAR_PANE_ID } = process.env;
    if (!HANGAR_API_URL || !HANGAR_API_TOKEN || !HANGAR_PANE_ID) process.exit(0);
    if (!isGitPush(event && event.tool_input && event.tool_input.command)) process.exit(0);
    const cwd = typeof event.cwd === "string" && event.cwd ? event.cwd : process.cwd();
    const child = spawn(process.execPath, [path.join(__dirname, "find-run.js")], {
      cwd,
      detached: true,
      stdio: "ignore",
      windowsHide: true,
      env: process.env,
    });
    child.on("error", () => undefined);
    child.unref();
  } catch {
    // Not JSON, no cwd…: nothing to do.
  }
  process.exit(0);
});
