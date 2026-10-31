"use strict";
// Lists the TCP ports something is listening on, for the Services tab:
//
//   node ports.js
//
// Read-only. It asks the system for the list (PowerShell's Get-NetTCPConnection on
// Windows, lsof on macOS, ss then lsof on Linux), then for each program's start time
// (`ps`, or the same PowerShell call), and prints one line of JSON:
//
//   { "platform": "linux", "ports": [{ "port", "pid", "name", "startedAt", "addresses",
//     "loopback", "portBase", "pane" }], "notes": [] }
//
// `portBase` and `pane` say which Hangar worker's pane a program was started in, when
// the system lets us read that (Linux and macOS: the two HANGAR_ variables of its
// environment, and nothing else of it). It kills nothing and writes nothing.
// With no tool able to list ports it prints { "error": "no-tool" } and exits 2.
const { execFile } = require("node:child_process");
const fs = require("node:fs");
const parse = require("./parse.js");

const WINDOWS_SCRIPT = [
  "$ErrorActionPreference = 'Stop'",
  "$listen = @(Get-NetTCPConnection -State Listen | Select-Object LocalAddress, LocalPort, OwningProcess)",
  "$ids = @($listen | ForEach-Object { $_.OwningProcess } | Sort-Object -Unique)",
  "$procs = @(Get-CimInstance Win32_Process | Where-Object { $ids -contains [int]$_.ProcessId } | ForEach-Object {",
  "  [pscustomobject]@{ ProcessId = [int]$_.ProcessId; Name = $_.Name; Start = ([DateTimeOffset]$_.CreationDate).ToUnixTimeMilliseconds() }",
  "})",
  "[pscustomobject]@{ listen = $listen; procs = $procs } | ConvertTo-Json -Depth 4 -Compress",
].join("\n");

/** Runs a program with no shell. Resolves { code, stdout } and never rejects: a missing program is code null. */
function run(file, args, { timeout = 15000 } = {}) {
  return new Promise((resolve) => {
    execFile(
      file,
      args,
      { timeout, windowsHide: true, maxBuffer: 16 * 1024 * 1024, env: { ...process.env, LC_ALL: "C", LANG: "C" } },
      (error, stdout) => resolve({ code: error ? (typeof error.code === "number" ? error.code : null) : 0, stdout: String(stdout || "") }),
    );
  });
}

async function listWindows(sys) {
  const answer = await sys.run("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", WINDOWS_SCRIPT]);
  if (answer.code !== 0) return null;
  try {
    return { rows: parse.parseWindows(answer.stdout), owners: new Map(), notes: [] };
  } catch {
    return null;
  }
}

async function listUnix(sys, platform) {
  let rows = null;
  if (platform !== "darwin") {
    const ss = await sys.run("ss", ["-H", "-ltnp"]);
    if (ss.code === 0) rows = parse.parseSs(ss.stdout);
  }
  if (rows === null) {
    const lsof = await sys.run("lsof", ["-nP", "-iTCP", "-sTCP:LISTEN", "-F", "pcn"]);
    // lsof exits 1 when it finds nothing to list, which is an answer.
    if (lsof.code === 0 || (lsof.code === 1 && !lsof.stdout.trim())) rows = parse.parseLsof(lsof.stdout);
  }
  if (rows === null) return null;

  const pids = [...new Set(rows.map((row) => row.pid).filter((pid) => pid !== null))];
  const starts = new Map();
  const owners = new Map();
  if (pids.length) {
    const ps = await sys.run("ps", ["-o", "pid=,lstart=", "-p", pids.join(",")]);
    for (const [pid, at] of parse.parsePsStart(ps.stdout)) starts.set(pid, at);
    for (const pid of pids) {
      const text = platform === "darwin" ? (await sys.run("ps", ["eww", "-p", String(pid)])).stdout : sys.readEnviron(pid);
      const owner = parse.readOwner(text);
      if (owner.portBase !== null) owners.set(pid, owner);
    }
  }
  for (const row of rows) row.startedAt = row.pid === null ? null : starts.get(row.pid) || null;
  return { rows, owners, notes: rows.some((row) => row.pid === null) ? ["some-hidden"] : [] };
}

/** The system's way of reading a file or running a program, so tests can stand in for it. */
const realSystem = {
  run,
  readEnviron(pid) {
    try {
      return fs.readFileSync(`/proc/${pid}/environ`, "latin1");
    } catch {
      return "";
    }
  },
};

async function listPorts({ platform = process.platform, sys = realSystem } = {}) {
  const found = platform === "win32" ? await listWindows(sys) : await listUnix(sys, platform);
  if (!found) return { error: "no-tool" };
  const ports = parse.merge(found.rows).map((entry) => {
    const owner = entry.pid === null ? null : found.owners.get(entry.pid) || null;
    return {
      port: entry.port,
      pid: entry.pid,
      name: entry.name,
      startedAt: entry.startedAt,
      addresses: entry.addresses,
      loopback: entry.addresses.every(parse.isLoopback),
      portBase: owner ? owner.portBase : null,
      pane: owner ? owner.pane : null,
    };
  });
  return { platform, ports, notes: found.notes };
}

if (require.main === module) {
  listPorts().then((result) => {
    process.stdout.write(`${JSON.stringify(result)}\n`);
    process.exitCode = result.error ? 2 : 0;
  });
}

module.exports = { listPorts, WINDOWS_SCRIPT };
