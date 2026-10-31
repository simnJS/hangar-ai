"use strict";
// What the page draws for each state, on a fake DOM: Hangar UI's classes, text and no
// markup from a program or a container, both languages, every state designed.
const test = require("node:test");
const assert = require("node:assert/strict");
const logic = require("../tabs/services/logic.js");
const view = require("../tabs/services/view.js");
const { createController } = require("../tabs/services/controller.js");
const { fixture, fakeClock, fakeHangar, fakeUi, fakeDocument, FakeNode, settle, refusal } = require("./helpers.js");

const NOW = Date.parse("2026-10-31T17:02:00Z");
const portsOut = JSON.stringify({
  platform: "linux",
  ports: [
    { port: 5432, pid: 1, name: "postgres", startedAt: "2026-10-30T08:00:00.000Z", loopback: true, portBase: null, pane: null },
    { port: 21100, pid: 4, name: "node", startedAt: "2026-10-31T15:48:09.000Z", loopback: true, portBase: 21100, pane: "Gus" },
    { port: 21104, pid: 5, name: "<img src=x onerror=alert(1)>", startedAt: null, loopback: false, portBase: 21100, pane: "Lou" },
    { port: 21140, pid: null, name: null, startedAt: null, loopback: false, portBase: null, pane: null },
  ],
  notes: ["some-hidden"],
});
const PANES = [
  { name: "Jack", agent: "claude", role: "orchestrator", state: "idle", canSend: true },
  { name: "Gus", agent: "claude", role: "worker", state: "working", canSend: true },
  { name: "Shell", agent: "shell", role: null, state: null, canSend: false },
];
const reply = (stdout, extra) => () => ({ stdout, ...extra });
const okCommands = (extra = {}) => ({ ports: reply(portsOut), "docker-list": reply(fixture("docker-ps.jsonl")), "docker-logs": reply(fixture("docker-logs.txt")), ...extra });
const refuse = (code) => () => {
  throw refusal(code);
};

async function drawn({ commands = okCommands(), lang = "en", panes = PANES, deny = [], wait = true } = {}) {
  const clock = fakeClock(NOW);
  const hangar = fakeHangar({ commands, panes, deny });
  const root = new FakeNode("div");
  const ui = fakeUi(lang);
  const t = ui.i18n(logic.STRINGS);
  const calls = [];
  const controllerRef = {};
  const actions = {
    refresh: () => controllerRef.c.refresh(),
    filter: (id) => controllerRef.c.setFilter(id),
    logs: (id) => controllerRef.c.showLogs(id),
    ask: (key, target) => controllerRef.c.openAsk(key, target),
    pick: (name) => controllerRef.c.pickPane(name),
    send: () => controllerRef.c.sendAsk(() => "2 h ago"),
    closeAsk: () => controllerRef.c.closeAsk(),
  };
  const draw = (state) => view.render(root, state, { doc: fakeDocument, logic, ui, t, now: clock.now, actions });
  const controller = createController({ hangar, logic, t, now: clock.now, setTimer: clock.setTimer, clearTimer: clock.clearTimer, onChange: draw });
  controllerRef.c = controller;
  const started = controller.start();
  if (wait) {
    await started;
    await settle();
  }
  return { root, controller, hangar, ui, clock, draw };
}
const click = async (node) => {
  node.onclick();
  await settle();
};
const buttonLabelled = (root, label) => root.find((node) => node.tag === "button" && node.text === label)[0];
const rowTitles = (root) => root.all("kit-row__title").map((node) => node.text);

test("while it reads for the first time: one line saying so, no table of nothing", async () => {
  const { root } = await drawn({ wait: false });
  assert.match(root.text, /Reading ports and containers…/);
  assert.equal(root.all("kit-row").length, 0);
});

test("ports are rows: the number first, the program, pid, age and reach, and whose block it is", async () => {
  const { root } = await drawn();
  const rows = root.all("svc-row").filter((row) => row.all("svc-port").length);
  assert.deepEqual(rows.map((row) => row.all("svc-port")[0].text), ["5432", "21100", "21104", "21140"]);
  const node = rows[1];
  assert.equal(node.all("svc-program")[0].text, "node");
  assert.match(node.text, /pid 4 · started <74 min ago> · local only/);
  assert.match(node.text, /Port block of Gus/, "the worker is named");
  assert.match(rows[2].text, /Port block of Lou, whose pane is closed/, "a leftover says its worker is gone");
  assert.match(rows[2].text, /reachable from the network/);
  assert.match(rows[3].text, /Worker port block 21120–21139|Worker port block 21140–21159/);
  assert.match(rows[3].text, /Unknown program/);
  assert.doesNotMatch(rows[0].text, /block/i, "a port outside the pool is nobody's");
  assert.match(root.text, /Some programs belong to another account/);
  assert.equal(root.all("kit-section__count")[0].text, "4");
});

test("what a program is called is text, never markup", async () => {
  const { root } = await drawn();
  const program = root.all("svc-program").find((node) => node.text.includes("<img"));
  assert.equal(program.text, "<img src=x onerror=alert(1)>");
  assert.equal(program.title, "<img src=x onerror=alert(1)>");
  assert.equal(root.find((node) => node.tag === "img").length, 0);
});

test("a filter shows the worker ports alone, and says how many", async () => {
  const { root } = await drawn();
  const chips = root.all("kit-filter");
  assert.deepEqual(chips.map((chip) => chip.text), ["All 4", "Worker ports 3"]);
  assert.equal(chips[0].attributes["aria-pressed"], "true");
  await click(chips[1]);
  assert.deepEqual(root.all("svc-port").map((node) => node.text), ["21100", "21104", "21140"]);
  assert.equal(root.all("kit-filter")[1].attributes["aria-pressed"], "true");
});

test("containers: state as a glyph and a word, image, ports, and the port block they publish into", async () => {
  const { root } = await drawn();
  const rows = root.all("svc-row").filter((row) => row.all("kit-glyph").length);
  assert.deepEqual(rows.map((row) => row.all("kit-row__title")[0].text), ["shop-db-1", "shop-api", "cache"]);
  assert.match(rows[0].text, /Running · postgres:16/);
  assert.equal(rows[0].all("kit-glyph")[0].attributes["aria-label"], "Running");
  assert.match(rows[0].text, /0\.0\.0\.0:5432->5432\/tcp/);
  assert.match(rows[1].text, /Stopped \(1\)/);
  assert.match(rows[1].all("kit-glyph")[0].className, /kit-glyph--failure/);
  assert.match(rows[1].text, /Worker port block 21100–21119/);
  assert.match(rows[2].text, /No published port/);
  assert.match(root.text, /Container shop-db-1/, "the port row of a published port names the container");
});

test("logs open in a log block on their first error, from the container's id", async () => {
  const { root, hangar } = await drawn();
  await click(buttonLabelled(root, "Logs"));
  assert.deepEqual(hangar.callsTo("command.run").at(-1).args, { name: "docker-logs", args: { id: "3f1c9a7b2d44" } });
  const host = root.all("svc-logbox")[0];
  assert.equal(host.logged.label, "Logs of shop-db-1");
  assert.equal(host.logged.lines.length, 6);
  assert.match(host.logged.lines[3], /^2026.*Error: connect ECONNREFUSED/);
  assert.match(root.text, /The last 200 lines\./);
  await click(buttonLabelled(root, "Hide logs"));
  assert.equal(root.all("svc-logbox").length, 0);
});

test("an empty log and a failing log are said", async () => {
  const empty = await drawn({ commands: okCommands({ "docker-logs": reply("") }) });
  await click(buttonLabelled(empty.root, "Logs"));
  assert.match(empty.root.text, /This container has written nothing\./);
  const failing = await drawn({ commands: okCommands({ "docker-logs": reply("", { exitCode: 1, stderr: "Error response from daemon: No such container" }) }) });
  await click(buttonLabelled(failing.root, "Logs"));
  assert.match(failing.root.text, /The logs could not be read: Error response from daemon/);
});

test("asking an agent: the question opens under the row, shows the brief, and sends it to the pane picked", async () => {
  const { root, hangar, controller } = await drawn();
  const ask = root.all("svc-item")[1].find((node) => node.tag === "button" && node.text === "Ask an agent to stop this")[0];
  assert.equal(ask.title, "Ask an agent to stop port 21100");
  await click(ask);
  const panel = root.all("svc-ask")[0];
  assert.match(panel.text, /Ask an agent to stop port 21100/);
  const options = panel.find((node) => node.tag === "option").map((node) => node.text);
  assert.deepEqual(options, ["—", "Jack", "Gus"], "only panes that can be sent to");
  assert.equal(panel.find((node) => node.tag === "option")[2].selected, true, "the worker who owns it is preselected");
  assert.match(panel.all("svc-brief")[0].text, /Please stop what listens on port 21100: node, pid 4, started <74 min ago>/);
  await click(buttonLabelled(panel, "Send"));
  assert.equal(hangar.callsTo("panes.send")[0].args.pane, "Gus");
  assert.match(root.all("svc-ask")[0].text, /Sent to Gus\. It acts when it is free\./);
  assert.equal(buttonLabelled(root.all("svc-ask")[0], "Send").disabled, true, "sent once, not twice");
  controller.closeAsk();
  assert.equal(root.all("svc-ask").length, 0);
});

test("asking when no agent can be reached, or when the tab may not send", async () => {
  const none = await drawn({ panes: [PANES[2]] });
  await click(none.root.all("svc-item")[1].find((node) => node.tag === "button" && node.text === "Ask an agent to stop this")[0]);
  assert.match(none.root.all("svc-ask")[0].text, /No agent can be reached/);
  const denied = await drawn({ deny: ["panes.list", "panes.send"] });
  await click(denied.root.all("svc-item")[1].find((node) => node.tag === "button" && node.text === "Ask an agent to stop this")[0]);
  assert.match(denied.root.all("svc-ask")[0].text, /may not send to a pane/);
});

test("Docker absent, Docker stopped, a Docker error: each its own state, the ports stay", async () => {
  const absent = await drawn({ commands: okCommands({ "docker-list": refuse("not_found") }) });
  assert.match(absent.root.text, /Docker is not installed/);
  assert.equal(absent.root.all("svc-port").length, 4);
  const stopped = await drawn({ commands: okCommands({ "docker-list": reply("", { exitCode: 1, stderr: fixture("docker-down.txt") }) }) });
  assert.match(stopped.root.text, /Docker is not running/);
  assert.match(stopped.root.text, /Start Docker Desktop or the Docker service, then refresh/);
  assert.ok(buttonLabelled(stopped.root, "Try again"));
  const broken = await drawn({ commands: okCommands({ "docker-list": reply("", { exitCode: 1, stderr: "permission denied" }) }) });
  assert.match(broken.root.text, /Docker answered with an error/);
  assert.match(broken.root.text, /permission denied/);
});

test("nothing listening, and no container: said, with what makes it change", async () => {
  const { root } = await drawn({ commands: okCommands({ ports: reply('{"platform":"linux","ports":[],"notes":[]}'), "docker-list": reply("") }) });
  assert.match(root.text, /Nothing is listening/);
  assert.match(root.text, /A server started later shows here\./);
  assert.match(root.text, /No container/);
  assert.equal(root.all("kit-filter").length, 0);
});

test("denied: one state for the whole tab, with what to accept and where, and a lock", async () => {
  const { root } = await drawn({ commands: okCommands({ ports: refuse("denied"), "docker-list": refuse("denied") }), deny: ["panes.list"] });
  assert.equal(root.all("kit-empty__title").filter((node) => node.text === "This tab may not read this").length, 1);
  assert.match(root.text, /Accept the commands of Services in Settings › Kits/);
  assert.equal(root.all("kit-icon--lock").length, 1);
  const sections = root.all("kit-section");
  assert.equal(sections[1].hidden, true, "no second section saying the same");
});

test("denied for one of the two: that part says so, the other still shows", async () => {
  const { root } = await drawn({ commands: okCommands({ "docker-list": refuse("denied") }) });
  assert.equal(root.all("svc-port").length, 4);
  assert.equal(root.all("kit-empty__title").filter((node) => node.text === "This tab may not read this").length, 1);
});

test("the ports could not be listed: Node missing, no tool, with a way to try again", async () => {
  const noNode = await drawn({ commands: okCommands({ ports: refuse("not_found") }) });
  assert.match(noNode.root.text, /Node is not installed/);
  const noTool = await drawn({ commands: okCommands({ ports: reply('{"error":"no-tool"}') }) });
  assert.match(noTool.root.text, /No tool lists the ports here/);
  assert.match(noTool.root.text, /PowerShell on Windows, lsof on macOS, ss or lsof on Linux/);
  assert.ok(buttonLabelled(noTool.root, "Try again"));
});

test("the 10 s redraw changes nothing the person is using", async () => {
  const { root, clock, controller } = await drawn();
  await click(buttonLabelled(root, "Logs"));
  const before = { list: root.all("kit-rows")[0], logbox: root.all("svc-logbox")[0], docker: root.all("svc-body")[1].children[0] };
  await clock.advance(10000);
  assert.equal(root.all("kit-rows")[0], before.list, "the port list is the same element");
  assert.equal(root.all("svc-logbox")[0], before.logbox, "the log keeps its scroll");
  assert.equal(root.all("svc-body")[1].children[0], before.docker);
  assert.equal(controller.state.refreshing, false);
});

test("the refresh button reads « Reading… » and waits while it reads", async () => {
  const { root, controller } = await drawn();
  assert.equal(buttonLabelled(root, "Refresh").disabled, false);
  const reading = controller.refresh();
  assert.equal(buttonLabelled(root, "Reading…").disabled, true);
  await reading;
  await settle();
  assert.ok(buttonLabelled(root, "Refresh"));
});

test("French: the same page in the interface's words, « Demander à un agent d'arrêter ça » included", async () => {
  const { root } = await drawn({ lang: "fr" });
  assert.match(root.text, /Ports/);
  assert.match(root.text, /Conteneurs/);
  assert.match(root.text, /Bloc de ports de Gus/);
  assert.match(root.text, /le panneau est fermé|dont le panneau est fermé/);
  assert.ok(buttonLabelled(root, "Actualiser"));
  assert.ok(buttonLabelled(root, "Journal"));
  assert.ok(root.find((node) => node.tag === "button" && node.text === "Demander à un agent d'arrêter ça").length >= 3);
  assert.match(root.text, /S'actualise toutes les 10 s/);
});

test("the page never writes markup, and owns nothing outside Hangar UI's classes", () => {
  const fs = require("node:fs");
  const path = require("node:path");
  const text = fs.readFileSync(path.join(__dirname, "..", "tabs", "services", "view.js"), "utf8");
  assert.doesNotMatch(text, /innerHTML|outerHTML|insertAdjacentHTML|document\.write|\.style\./);
});
