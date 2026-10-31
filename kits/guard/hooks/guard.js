"use strict";
// PreToolUse hook on Bash, Read, Edit, Write and Grep. It answers on stdout with
// Claude Code's documented deny, or says nothing, and always exits 0. Anything that
// goes wrong in here lets the call through and is logged in the kit's data folder.
const { spawn } = require("node:child_process");
const path = require("node:path");

let finished = false;

function finish(answer) {
  if (finished) return;
  finished = true;
  // Written through a callback: on Windows, a pipe's write is not done when it returns.
  if (answer) process.stdout.write(JSON.stringify(answer), () => process.exit(0));
  else process.exit(0);
}

function failOpen(error) {
  try {
    const { dataFolder } = require("./context.js");
    const store = require("./store.js");
    store.appendError(dataFolder(process.env), (error && error.message) || error, Date.now());
  } catch {
    /* nowhere to say it */
  }
  finish(null);
}

process.on("uncaughtException", failOpen);
process.on("unhandledRejection", failOpen);
process.stdout.on("error", () => process.exit(0));

// Opening the tab is a local call of up to two seconds: a refusal does not wait for it.
function openTab() {
  const { HANGAR_API_URL, HANGAR_API_TOKEN, HANGAR_PANE_ID } = process.env;
  if (!HANGAR_API_URL || !HANGAR_API_TOKEN || !HANGAR_PANE_ID) return;
  const child = spawn(process.execPath, [path.join(__dirname, "open-tab.js")], {
    detached: true,
    stdio: "ignore",
    windowsHide: true,
    env: process.env,
  });
  child.on("error", () => undefined);
  child.unref();
}

let input = "";
process.stdin.setEncoding("utf8");
process.stdin.on("data", (chunk) => (input += chunk));
process.stdin.on("error", failOpen);
process.stdin.on("end", () => {
  const { decide } = require("./decide.js");
  finish(decide(input, { openTab }));
});
