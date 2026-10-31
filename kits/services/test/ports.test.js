"use strict";
// The readers of tools/parse.js on each system's output, and tools/ports.js putting them
// together with a stand-in for the system: no real process is asked for anything.
const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const parse = require("../tools/parse.js");
const { listPorts, WINDOWS_SCRIPT } = require("../tools/ports.js");
const { fixture } = require("./helpers.js");

const environ = () => fs.readFileSync(path.join(__dirname, "fixtures", "linux-environ.bin"), "latin1");

test("Windows: ports with their program and start time, a program's two addresses counted once", () => {
  const merged = parse.merge(parse.parseWindows(fixture("windows-listen.json")));
  assert.deepEqual(
    merged.map((entry) => [entry.port, entry.pid, entry.name, entry.addresses.length]),
    [
      [135, 1012, "svchost.exe", 2],
      [445, 4, "System", 1],
      [5432, 3320, "postgres.exe", 2],
      [6379, 4188, "redis-server.exe", 1],
      [21100, 7716, "node.exe", 2],
      [21104, 9052, "python.exe", 1],
      [22003, 6844, "node.exe", 1],
      [49664, 1488, "lsass.exe", 1],
      [49665, 1488, "lsass.exe", 1],
    ],
  );
  assert.equal(merged.find((entry) => entry.port === 21100).startedAt, "2026-10-31T15:48:09.000Z");
});

test("Windows: PowerShell writes a lone item as an object, and nothing as null", () => {
  assert.deepEqual(parse.parseWindows(fixture("windows-single.json")).map((row) => [row.port, row.name]), [[21104, "python.exe"]]);
  assert.deepEqual(parse.parseWindows('{"listen":null,"procs":null}'), []);
  assert.deepEqual(parse.parseWindows("﻿" + '{"listen":[],"procs":[]}'), []);
});

test("Windows: a listener whose process is gone keeps its port, without a name", () => {
  const rows = parse.parseWindows('{"listen":[{"LocalAddress":"0.0.0.0","LocalPort":8080,"OwningProcess":99}],"procs":[]}');
  assert.deepEqual(rows, [{ address: "0.0.0.0", port: 8080, pid: 99, name: null, startedAt: null }]);
});

test("Linux ss: addresses of every shape, programs when the user may see them", () => {
  const rows = parse.parseSs(fixture("linux-ss.txt"));
  assert.equal(rows.length, 9);
  assert.deepEqual(rows[0], { address: "0.0.0.0", port: 25, pid: null, name: null });
  assert.deepEqual(rows[2], { address: "127.0.0.53", port: 53, pid: null, name: null }, "%lo is the interface, not the address");
  assert.deepEqual(rows[4], { address: "::", port: 25, pid: null, name: null });
  assert.deepEqual(rows[5], { address: "127.0.0.1", port: 21100, pid: 4121, name: "node" });
  assert.deepEqual(rows[6], { address: "::1", port: 21100, pid: 4121, name: "node" });
  assert.equal(rows[7].pid, 3977, "the first of two programs sharing a socket");
  assert.deepEqual(rows[8], { address: "0.0.0.0", port: 8080, pid: 2210, name: 'my "app"' }, "* is every address; a quoted name is kept whole");
});

test("macOS lsof -F: one program, several sockets, escaped bytes in a name", () => {
  const rows = parse.parseLsof(fixture("macos-lsof.txt"));
  assert.deepEqual(
    rows.map((row) => [row.port, row.pid, row.name, row.address]),
    [
      [21100, 4121, "node", "127.0.0.1"],
      [21100, 4121, "node", "::1"],
      [22003, 3977, "python3", "0.0.0.0"],
      [7000, 812, "ControlCe center", "0.0.0.0"],
      [7000, 812, "ControlCe center", "::"],
    ],
  );
});

test("ps start times, in the C locale", () => {
  const starts = parse.parsePsStart(fixture("linux-ps.txt"));
  assert.deepEqual([...starts.keys()], [4121, 3977, 2210]);
  assert.equal(new Date(starts.get(4121)).getMinutes(), 48);
  assert.equal(new Date(starts.get(2210)).getDate(), 3, "a day padded with a space reads as well");
  assert.equal(parse.parsePsStart("garbage\n 12 Foo Bar 1 2:3:4 5\n").size, 0);
});

test("the owner of a program: its two variables, and nothing else of its environment", () => {
  const linux = parse.readOwner(environ());
  assert.deepEqual(linux, { portBase: 21100, pane: "Gus" });
  assert.deepEqual(parse.readOwner(fixture("macos-ps-eww.txt")), { portBase: 21100, pane: "Gus" });
  assert.deepEqual(Object.keys(linux).sort(), ["pane", "portBase"], "nothing else is kept");
  assert.deepEqual(parse.readOwner("PATH=/usr/bin\0HOME=/x\0"), { portBase: null, pane: null });
  assert.deepEqual(parse.readOwner("HANGAR_PORT_BASE=abc\0"), { portBase: null, pane: null });
  assert.deepEqual(parse.readOwner("XHANGAR_PORT_BASE=21000\0"), { portBase: null, pane: null }, "a longer name is not ours");
});

test("endpoints", () => {
  assert.deepEqual(parse.splitEndpoint("[::1]:80"), { address: "::1", port: 80 });
  assert.deepEqual(parse.splitEndpoint("*:443"), { address: "0.0.0.0", port: 443 });
  assert.equal(parse.splitEndpoint("nonsense"), null);
  assert.equal(parse.splitEndpoint("1.2.3.4:70000"), null);
  assert.ok(parse.isLoopback("127.0.0.54") && parse.isLoopback("::1") && !parse.isLoopback("0.0.0.0") && !parse.isLoopback("::"));
});

/** A stand-in for the system: answers by program name from what was recorded. */
function standIn(answers, environs = {}) {
  const asked = [];
  return {
    asked,
    async run(file, args) {
      asked.push([file, ...args]);
      const answer = answers[file];
      if (!answer) return { code: null, stdout: "" };
      return typeof answer === "function" ? answer(args) : answer;
    },
    readEnviron: (pid) => environs[pid] || "",
  };
}

test("ports.js on Windows: one PowerShell call, no owner, JSON ready for the tab", async () => {
  const sys = standIn({ "powershell.exe": { code: 0, stdout: fixture("windows-listen.json") } });
  const result = await listPorts({ platform: "win32", sys });
  assert.equal(sys.asked.length, 1);
  assert.equal(sys.asked[0][0], "powershell.exe");
  assert.ok(sys.asked[0].includes("-NonInteractive"));
  assert.equal(result.platform, "win32");
  const node = result.ports.find((entry) => entry.port === 21100);
  assert.deepEqual(node, { port: 21100, pid: 7716, name: "node.exe", startedAt: "2026-10-31T15:48:09.000Z", addresses: ["::1", "127.0.0.1"], loopback: true, portBase: null, pane: null });
  assert.equal(result.ports.find((entry) => entry.port === 21104).loopback, false);
  assert.equal(result.ports.find((entry) => entry.port === 49664).loopback, false, "a LAN address is not loopback");
});

test("ports.js on Linux: ss, then ps for start times, then the two variables of each program", async () => {
  const sys = standIn({ ss: { code: 0, stdout: fixture("linux-ss.txt") }, ps: { code: 0, stdout: fixture("linux-ps.txt") } }, { 4121: environ() });
  const result = await listPorts({ platform: "linux", sys });
  assert.deepEqual(sys.asked[0], ["ss", "-H", "-ltnp"]);
  assert.deepEqual(sys.asked[1].slice(0, 3), ["ps", "-o", "pid=,lstart="]);
  const node = result.ports.find((entry) => entry.port === 21100);
  assert.equal(node.pane, "Gus");
  assert.equal(node.portBase, 21100);
  assert.ok(node.startedAt);
  assert.equal(result.ports.find((entry) => entry.port === 22003).pane, null);
  assert.deepEqual(result.notes, ["some-hidden"], "ports of other accounts have no program to name");
});

test("ports.js on Linux without ss falls back to lsof", async () => {
  const sys = standIn({ lsof: { code: 0, stdout: fixture("macos-lsof.txt") }, ps: { code: 0, stdout: fixture("linux-ps.txt") } });
  const result = await listPorts({ platform: "linux", sys });
  assert.deepEqual(sys.asked[0], ["ss", "-H", "-ltnp"]);
  assert.deepEqual(sys.asked[1], ["lsof", "-nP", "-iTCP", "-sTCP:LISTEN", "-F", "pcn"]);
  assert.deepEqual(result.ports.map((entry) => entry.port), [7000, 21100, 22003]);
});

test("ports.js on macOS: lsof, never ss, and ps eww for the owner", async () => {
  const sys = standIn({
    lsof: { code: 0, stdout: fixture("macos-lsof.txt") },
    ps: (args) => ({ code: 0, stdout: args[0] === "eww" ? (args[2] === "4121" ? fixture("macos-ps-eww.txt") : "") : fixture("linux-ps.txt") }),
  });
  const result = await listPorts({ platform: "darwin", sys });
  assert.ok(!sys.asked.some(([file]) => file === "ss"));
  assert.equal(result.ports.find((entry) => entry.port === 21100).pane, "Gus");
  assert.equal(result.ports.find((entry) => entry.port === 7000).pane, null);
});

test("lsof exiting 1 with nothing to say means nothing is listening", async () => {
  const sys = standIn({ lsof: { code: 1, stdout: "" } });
  assert.deepEqual(await listPorts({ platform: "darwin", sys }), { platform: "darwin", ports: [], notes: [] });
});

test("no tool at all: an error the tab can name, not an empty list", async () => {
  assert.deepEqual(await listPorts({ platform: "linux", sys: standIn({}) }), { error: "no-tool" });
  assert.deepEqual(await listPorts({ platform: "win32", sys: standIn({ "powershell.exe": { code: 1, stdout: "" } }) }), { error: "no-tool" });
  assert.deepEqual(await listPorts({ platform: "win32", sys: standIn({ "powershell.exe": { code: 0, stdout: "not json" } }) }), { error: "no-tool" });
});

test("the tool only reads: no kill, no stop, no write in it", () => {
  const sources = ["ports.js", "parse.js"].map((name) => fs.readFileSync(path.join(__dirname, "..", "tools", name), "utf8")).join("\n");
  assert.doesNotMatch(sources, /process\.kill|Stop-Process|taskkill|docker (stop|rm|kill)|writeFile|appendFile|unlink|rmSync|spawn\(|(?<![.\w])exec\(|shell:\s*true/);
  assert.doesNotMatch(WINDOWS_SCRIPT, /Stop-|Remove-|Set-|New-|Invoke-|Start-/);
});

test("the real tool runs here and says something true about this computer", async () => {
  const result = await listPorts();
  if (result.error) return; // a CI image without the system's listing tool
  assert.equal(result.platform, process.platform);
  assert.ok(Array.isArray(result.ports));
  for (const entry of result.ports) {
    assert.ok(Number.isInteger(entry.port) && entry.port >= 1 && entry.port <= 65535);
    assert.ok(entry.addresses.length >= 1);
  }
});
