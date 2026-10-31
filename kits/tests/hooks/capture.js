"use strict";
// A tool call turned into the record to keep: pure but for the clock, so the tests can
// feed it events without a process.
const path = require("node:path");

const COMMAND_MAX = 600;

/** The text a tool call produced, whatever shape this Claude Code gives it. */
function outputOf(event) {
  const parts = [];
  const add = (value) => {
    if (typeof value === "string") parts.push(value);
    else if (Array.isArray(value)) for (const item of value) add(item && typeof item === "object" ? item.text : item);
  };
  const response = event.tool_response;
  if (typeof response === "string") add(response);
  else if (response && typeof response === "object") {
    add(response.stdout);
    add(response.stderr);
    add(response.output);
    add(response.content);
  }
  add(event.error);
  return parts.join("\n");
}

function exitCodeOf(event, output) {
  const response = event.tool_response;
  for (const value of [response && response.exit_code, response && response.exitCode, response && response.returnCode]) {
    if (Number.isFinite(value)) return value;
  }
  const match = /^Exit code (-?\d+)/.exec(String(event.error || output));
  return match ? Number(match[1]) : null;
}

/** Builds the record to keep, or null when this call has nothing to do with tests. Pure but for the clock. */
function recordFor(event, env, now) {
  if (!event || (event.tool_name !== "Bash" && event.tool_name !== "PowerShell")) return null;
  const command = event.tool_input && event.tool_input.command;
  const { detect } = require("./detect.js");
  const found = detect(command);
  if (!found) return null;

  const { parseRun } = require("./parse.js");
  const { runKey } = require("./store.js");
  const output = outputOf(event);
  const exitCode = exitCodeOf(event, output);
  const result = parseRun(output.replace(/^Exit code -?\d+\n?/, ""), found.frameworks, exitCode);

  const projectDir = env.CLAUDE_PROJECT_DIR || event.cwd || process.cwd();
  const agent = env.HANGAR_PANE_NAME || (env.HANGAR_PANE_ID ? String(env.HANGAR_PANE_ID).slice(0, 8) : String(event.session_id || "agent").slice(0, 8));
  const project = path.basename(String(projectDir).replace(/[\\/]+$/, "")) || String(projectDir);
  const run = {
    version: 1,
    key: runKey(projectDir, agent),
    at: now,
    ...result,
    exitCode,
    command: command.length > COMMAND_MAX ? `${command.slice(0, COMMAND_MAX - 1)}…` : command,
    cwd: String(event.cwd || projectDir),
    project,
    agent,
  };
  return run;
}

module.exports = { recordFor, outputOf, exitCodeOf, COMMAND_MAX };
