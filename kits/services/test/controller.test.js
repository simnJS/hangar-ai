"use strict";
// The controller on a fake Hangar: what it reads, when, and what it does when Docker is
// off, a command is denied or the person asks an agent to stop something.
const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const logic = require("../tabs/services/logic.js");
const { createController } = require("../tabs/services/controller.js");
const { fixture, fakeClock, fakeHangar, fakeUi, settle, refusal } = require("./helpers.js");

const t = fakeUi("en").i18n(logic.STRINGS);
const portsOut = JSON.stringify({
  platform: "linux",
  ports: [
    { port: 5432, pid: 1, name: "postgres", startedAt: "2026-10-30T08:00:00.000Z", loopback: true, portBase: null, pane: null },
    { port: 21100, pid: 4, name: "node", startedAt: "2026-10-31T15:48:09.000Z", loopback: true, portBase: 21100, pane: "Gus" },
    { port: 21104, pid: 5, name: "python3", startedAt: "2026-10-31T16:31:40.000Z", loopback: false, portBase: 21100, pane: "Lou" },
  ],
  notes: [],
});
const PANES = [
  { name: "Jack", agent: "claude", role: "orchestrator", state: "idle", canSend: true },
  { name: "Gus", agent: "claude", role: "worker", state: "working", canSend: true },
  { name: "Shell", agent: "shell", role: null, state: null, canSend: false },
];
const okCommands = (extra = {}) => ({
  ports: () => ({ stdout: portsOut }),
  "docker-list": () => ({ stdout: fixture("docker-ps.jsonl") }),
  "docker-logs": () => ({ stdout: fixture("docker-logs.txt") }),
  ...extra,
});

async function opened({ commands = okCommands(), panes = PANES, deny = [], sendError = null } = {}) {
  const clock = fakeClock();
  const hangar = fakeHangar({ commands, panes, deny, sendError });
  const controller = createController({ hangar, logic, t, now: clock.now, setTimer: clock.setTimer, clearTimer: clock.clearTimer });
  await controller.start();
  await settle();
  return { hangar, controller, clock, state: controller.state };
}

test("it reads the ports, the containers and the panels once, in parallel", async () => {
  const { hangar, state } = await opened();
  assert.equal(state.loaded, true);
  assert.deepEqual(hangar.callsTo("command.run").map((call) => call.args.name).sort(), ["docker-list", "ports"]);
  assert.equal(hangar.callsTo("panes.list").length, 1);
  assert.deepEqual(state.ports.items.map((p) => p.port), [5432, 21100, 21104]);
  assert.equal(state.docker.items.length, 3);
  assert.equal(state.updatedAt, Date.parse("2026-10-31T17:02:00Z"));
});

test("every 10 s while shown, never while hidden, and at once when shown again", async () => {
  const { hangar, controller, clock } = await opened();
  const reads = () => hangar.callsTo("command.run").filter((call) => call.args.name === "ports").length;
  assert.equal(reads(), 1);
  await clock.advance(9999);
  assert.equal(reads(), 1);
  await clock.advance(1);
  assert.equal(reads(), 2);
  controller.setVisible(false);
  assert.equal(clock.pending(), 0, "no timer waits behind another tab");
  await clock.advance(60000);
  assert.equal(reads(), 2);
  controller.setVisible(true);
  await settle();
  assert.equal(reads(), 3, "it reads as soon as it is seen");
  assert.equal(clock.pending(), 1);
  controller.dispose();
  assert.equal(clock.pending(), 0);
});

test("a refresh going on is not doubled by a click", async () => {
  const { hangar, controller } = await opened();
  const first = controller.refresh();
  controller.refresh();
  await first;
  await settle();
  assert.equal(hangar.callsTo("command.run").filter((call) => call.args.name === "ports").length, 2);
});

test("Docker not installed: containers say so, ports are fine", async () => {
  const { state } = await opened({ commands: okCommands({ "docker-list": () => { throw refusal("not_found"); } }) });
  assert.equal(state.docker.status, "absent");
  assert.equal(state.ports.status, "ok");
});

test("Docker stopped, as Docker Desktop says it on Windows and the engine on Linux", async () => {
  for (const text of [fixture("docker-down.txt"), fixture("docker-down-unix.txt")]) {
    const { state } = await opened({ commands: okCommands({ "docker-list": () => ({ exitCode: 1, stderr: text }) }) });
    assert.equal(state.docker.status, "stopped");
    assert.equal(state.ports.items.length, 3);
  }
});

test("another Docker failure is an error with docker's words, not « stopped »", async () => {
  const { state } = await opened({ commands: okCommands({ "docker-list": () => ({ exitCode: 1, stderr: "permission denied while trying to connect to the docker API" }) }) });
  assert.equal(state.docker.status, "error");
  assert.match(state.docker.detail, /permission denied/);
});

test("denied: each command on its own, the panels too", async () => {
  const { state } = await opened({ commands: okCommands({ ports: () => { throw refusal("denied"); }, "docker-list": () => { throw refusal("denied"); } }), deny: ["panes.list"] });
  assert.equal(state.ports.status, "denied");
  assert.equal(state.docker.status, "denied");
  assert.equal(state.panes, null);
  assert.equal(state.panesDenied, true);
});

test("ports: no Node, no listing tool, an answer nobody can read", async () => {
  const noNode = await opened({ commands: okCommands({ ports: () => { throw refusal("not_found"); } }) });
  assert.deepEqual([noNode.state.ports.status, noNode.state.ports.kind], ["error", "no-node"]);
  const noTool = await opened({ commands: okCommands({ ports: () => ({ exitCode: 2, stdout: '{"error":"no-tool"}' }) }) });
  assert.equal(noTool.state.ports.kind, "no-tool");
  const noToolOk = await opened({ commands: okCommands({ ports: () => ({ stdout: '{"error":"no-tool"}' }) }) });
  assert.equal(noToolOk.state.ports.kind, "no-tool");
  const garbage = await opened({ commands: okCommands({ ports: () => ({ stdout: "hello" }) }) });
  assert.equal(garbage.state.ports.kind, "failed");
  const slow = await opened({ commands: okCommands({ ports: () => ({ exitCode: null, timedOut: true }) }) });
  assert.equal(slow.state.ports.kind, "timeout");
});

test("nothing listening and no container are answers, not errors", async () => {
  const { state } = await opened({ commands: okCommands({ ports: () => ({ stdout: '{"platform":"linux","ports":[],"notes":[]}' }), "docker-list": () => ({ stdout: "" }) }) });
  assert.deepEqual([state.ports.status, state.ports.items.length, state.docker.status, state.docker.items.length], ["ok", 0, "ok", 0]);
});

test("logs: asked by the container's id only, stdout and stderr both, and a second click closes them", async () => {
  const { hangar, controller, state } = await opened({ commands: okCommands({ "docker-logs": () => ({ stdout: "out line\n", stderr: "Error: boom\n" }) }) });
  await controller.showLogs("a81e0c55d9f3");
  assert.deepEqual(hangar.callsTo("command.run").at(-1).args, { name: "docker-logs", args: { id: "a81e0c55d9f3" } });
  assert.deepEqual(state.logs, { id: "a81e0c55d9f3", status: "ok", lines: ["out line", "Error: boom"], detail: "" });
  await controller.showLogs("a81e0c55d9f3");
  assert.equal(state.logs, null);
});

test("logs: an id that is not an id is never sent", async () => {
  const { hangar, controller, state } = await opened();
  for (const id of ["--rm", "../x", "3f1c9a7b2d44 --all", "", undefined]) await controller.showLogs(id);
  assert.equal(hangar.callsTo("command.run").filter((call) => call.args.name === "docker-logs").length, 0);
  assert.equal(state.logs, null);
});

test("logs: docker failing says why, and a container that went away takes its logs with it", async () => {
  const { controller, state } = await opened({ commands: okCommands({ "docker-logs": () => ({ exitCode: 1, stderr: "Error response from daemon: No such container" }) }) });
  await controller.showLogs("a81e0c55d9f3");
  assert.equal(state.logs.status, "error");
  assert.match(state.logs.detail, /No such container/);
});

test("logs of a container no longer listed close on the next refresh", async () => {
  let list = fixture("docker-ps.jsonl");
  const { controller, state } = await opened({ commands: okCommands({ "docker-list": () => ({ stdout: list }) }) });
  await controller.showLogs("a81e0c55d9f3");
  list = list.split("\n").filter((line) => !line.includes("a81e0c55d9f3")).join("\n");
  await controller.refresh();
  assert.equal(state.logs, null);
});

test("asking an agent: the worker who owns the port is preselected, and the brief is what is sent", async () => {
  const { hangar, controller, state } = await opened();
  const target = { type: "port", row: logic.portRows(state.ports.items, state.panes, state.docker.items).find((row) => row.port === 21100) };
  controller.openAsk("port:21100:4", target);
  assert.equal(state.ask.pane, "Gus");
  await controller.sendAsk(() => "2 h ago");
  const [sent] = hangar.callsTo("panes.send");
  assert.equal(sent.args.pane, "Gus");
  assert.match(sent.args.text, /port 21100: node, pid 4, started 2 h ago/);
  assert.match(sent.args.text, /started in the pane of Gus/);
  assert.equal(state.ask.status, "sent");
});

test("asking about a leftover whose worker is gone: nobody preselected, the person picks, and nothing is sent without a pick", async () => {
  const { hangar, controller, state } = await opened();
  const row = logic.portRows(state.ports.items, state.panes, [])[2];
  assert.equal(row.owner.gone, true);
  controller.openAsk("port:21104:5", { type: "port", row });
  assert.equal(state.ask.pane, "");
  await controller.sendAsk(() => "");
  assert.equal(hangar.callsTo("panes.send").length, 0);
  controller.pickPane("Jack");
  await controller.sendAsk(() => "");
  assert.equal(hangar.callsTo("panes.send")[0].args.pane, "Jack");
});

test("asking again closes the question; a port that vanished takes an unsent question with it", async () => {
  let ports = portsOut;
  const { controller, state } = await opened({ commands: okCommands({ ports: () => ({ stdout: ports }) }) });
  const row = logic.portRows(state.ports.items, state.panes, [])[0];
  controller.openAsk("k", { type: "port", row });
  controller.openAsk("k", { type: "port", row });
  assert.equal(state.ask, null);
  controller.openAsk("k", { type: "port", row });
  ports = JSON.stringify({ platform: "linux", ports: [], notes: [] });
  await controller.refresh();
  assert.equal(state.ask, null);
});

test("a refused send says so, denied included; the kit itself calls nothing that stops anything", async () => {
  const denied = await opened({ sendError: "denied" });
  denied.controller.openAsk("k", { type: "container", container: denied.state.docker.items[0] });
  denied.controller.pickPane("Jack");
  await denied.controller.sendAsk(() => "");
  assert.deepEqual([denied.state.ask.status, denied.state.ask.detail], ["error", "denied"]);
  const calls = [...denied.hangar.calls.map((call) => call.method), ...denied.hangar.callsTo("command.run").map((call) => call.args.name)];
  assert.deepEqual([...new Set(calls)].sort(), ["command.run", "docker-list", "panes.list", "panes.send", "ports"]);
});

test("panes.changed reads the panels again, nothing else", async () => {
  const { hangar, state } = await opened();
  hangar.panes = [{ name: "Ava", agent: "claude", role: "worker", state: null, canSend: true }];
  hangar.emit("panes.changed", {});
  await settle();
  assert.deepEqual(state.panes.map((p) => p.name), ["Ava"]);
  assert.equal(hangar.callsTo("command.run").length, 2);
});

test("the page's code calls only what the manifest grants", () => {
  const tab = path.join(__dirname, "..", "tabs", "services");
  const manifest = JSON.parse(fs.readFileSync(path.join(__dirname, "..", "hangar-kit.json"), "utf8"));
  const granted = new Set(manifest.tabs[0].permissions);
  const methods = new Set();
  const names = new Set();
  for (const file of fs.readdirSync(tab).filter((name) => name.endsWith(".js"))) {
    const text = fs.readFileSync(path.join(tab, file), "utf8");
    for (const [, method] of text.matchAll(/hangar\.call\("([\w.]+)"/g)) methods.add(method);
    for (const [, name] of text.matchAll(/command\("([\w-]+)"/g)) names.add(name);
  }
  assert.deepEqual([...methods].sort(), ["command.run", "panes.list", "panes.send"]);
  assert.ok(granted.has("panes.read") && granted.has("panes.send"));
  assert.deepEqual([...names].sort(), ["docker-list", "docker-logs", "ports"]);
  for (const name of names) assert.ok(granted.has(`command:${name}`), name);
});
