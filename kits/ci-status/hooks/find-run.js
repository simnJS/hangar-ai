"use strict";
// The detached half of the hook: finds the run(s) the push started, then asks
// Hangar to open a tab for each. Silent, and ends by itself in about half a minute.
process.on("uncaughtException", () => process.exit(0));

const { execFile } = require("node:child_process");
const { findRuns, openTab } = require("./lib.js");

function run(program, args) {
  return new Promise((resolve, reject) => {
    execFile(program, args, { timeout: 20000, windowsHide: true, maxBuffer: 1 << 20 }, (error, stdout, stderr) => {
      if (error) reject(Object.assign(error, { stderr: String(stderr || "") }));
      else resolve(String(stdout));
    });
  });
}

const io = {
  git: (args) => run("git", args),
  gh: (args) => run("gh", args),
  sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)),
  now: () => Date.now(),
};

(async () => {
  const ids = await findRuns(io);
  const { HANGAR_API_URL: url, HANGAR_API_TOKEN: token, HANGAR_PANE_ID: pane } = process.env;
  for (const id of ids) await openTab({ url, token, pane, id });
  process.exit(0);
})().catch(() => process.exit(0));
