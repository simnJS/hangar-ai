"use strict";
const test = require("node:test");
const assert = require("node:assert/strict");
const logic = require("../tabs/services/logic.js");
const { fixture, fakeUi } = require("./helpers.js");

const t = fakeUi("en").i18n(logic.STRINGS);

test("both languages have the same keys, and French speaks as Hangar does (the glossary in src/i18n.ts)", () => {
  assert.deepEqual(Object.keys(logic.STRINGS.fr).sort(), Object.keys(logic.STRINGS.en).sort());
  for (const [key, text] of Object.entries(logic.STRINGS.fr)) {
    assert.doesNotMatch(text, /\b(?:vous|votre|vos)\b|[a-zéè]ez\b(?!\s*:)/i, `${key}: « tu »`);
    assert.doesNotMatch(text, /espace de travail/i, `${key}: « workspace »`);
    assert.doesNotMatch(text, /Paramètres/, `${key}: « Réglages »`);
    assert.doesNotMatch(text.replace(/\{\w+\}/g, ""), /\bpane\b|\bpanel\b/i, `${key}: « panneau »`);
    assert.doesNotMatch(text, /’/, `${key}: the apostrophe Hangar's strings use`);
  }
  assert.match(logic.STRINGS.fr.deniedHint, /Réglages › Kits/);
  assert.equal(logic.STRINGS.fr.ask, "Demander à un agent d'arrêter ça");
  for (const [key, text] of Object.entries(logic.STRINGS.en)) assert.doesNotMatch(text, /[àâçéèêëîïôûù]/i, `${key}: English`);
});

test("a port belongs to the worker block it falls in: 21000, 21020…", () => {
  assert.equal(logic.blockOf(21000), 21000);
  assert.equal(logic.blockOf(21019), 21000);
  assert.equal(logic.blockOf(21020), 21020);
  assert.equal(logic.blockOf(21100), 21100);
  assert.equal(logic.blockOf(26999), 26980);
  for (const outside of [20999, 27000, 3000, 80, 65535, 0, -1, 21100.5, NaN]) assert.equal(logic.blockOf(outside), null, String(outside));
});

test("ports.js output → rows, sorted, with nothing but what the tab uses", () => {
  const ports = JSON.stringify({
    platform: "linux",
    ports: [
      { port: 8080, pid: 5, name: "b", startedAt: "2026-10-31T10:00:00.000Z", loopback: false, portBase: null, pane: null, extra: "dropped" },
      { port: 21100, pid: 4, name: "node", startedAt: "not a date", loopback: true, portBase: 21100, pane: "Gus" },
      { port: "x" },
    ],
    notes: ["some-hidden", 3],
  });
  const parsed = logic.parsePorts(ports);
  assert.deepEqual(parsed.ports.map((row) => row.port), [8080, 21100]);
  assert.equal(parsed.ports[1].startedAt, null);
  assert.equal(parsed.ports[0].extra, undefined);
  assert.deepEqual(parsed.notes, ["some-hidden"]);
  assert.deepEqual(logic.parsePorts('{"error":"no-tool"}'), { error: "no-tool" });
  assert.deepEqual(logic.parsePorts("not json"), { error: "unreadable" });
  assert.deepEqual(logic.parsePorts('{"ports":3}'), { error: "unreadable" });
});

test("docker ps: one object a line, state, exit code, published ports; broken ids are dropped", () => {
  const containers = logic.parseDocker(fixture("docker-ps.jsonl"));
  assert.deepEqual(containers.map((c) => [c.name, c.state, c.exitCode]), [["shop-db-1", "running", null], ["shop-api", "exited", 1], ["cache", "created", null]]);
  const db = containers[0];
  assert.equal(db.image, "postgres:16");
  assert.deepEqual(db.ports, [{ host: 5432, container: 5432, proto: "tcp" }], "the IPv6 twin counts once");
  assert.deepEqual(containers[1].ports, [{ host: 21104, container: 3000, proto: "tcp" }]);
  assert.deepEqual(containers[2].ports, []);
  assert.deepEqual(logic.parseDocker(""), [], "docker answers, with no container");
  assert.equal(logic.parseDocker("{not json"), null);
  const array = JSON.stringify([{ ID: "0123456789ab", Names: "a,b", State: "running" }, { ID: "--rm", Names: "evil" }]);
  assert.deepEqual(logic.parseDocker(array).map((c) => [c.id, c.name]), [["0123456789ab", "a"]], "an id that is not hex is no container");
});

test("published ports: ranges, protocols, no host port", () => {
  assert.deepEqual(logic.publishedPorts("0.0.0.0:21100-21102->3000-3002/tcp").map((p) => [p.host, p.container]), [[21100, 3000], [21101, 3001], [21102, 3002]]);
  assert.deepEqual(logic.publishedPorts("3000/tcp, 127.0.0.1:53->53/udp"), [{ host: 53, container: 53, proto: "udp" }]);
  assert.deepEqual(logic.publishedPorts(""), []);
  assert.equal(logic.publishedPorts("0.0.0.0:1-60000->1/tcp").length, 100, "a wild range is cut");
});

test("a container's state is a glyph and a word", () => {
  const of = (state, exitCode = null) => logic.containerState({ state, exitCode });
  assert.deepEqual(of("running"), { glyph: "success", key: "state_running" });
  assert.equal(of("exited", 1).glyph, "failure");
  assert.equal(of("exited", 0).glyph, "neutral");
  assert.equal(of("created").glyph, "waiting");
  assert.equal(of("restarting").glyph, "running");
  assert.equal(of("weird").key, "state_other");
});

test("whose a port is: the pane that started it, else the block it sits in, else nobody", () => {
  const panes = [{ name: "Gus", canSend: true }, { name: "Jack", canSend: true }];
  const base = { portBase: null, pane: null };
  assert.deepEqual(logic.ownerOf({ ...base, port: 21100, pane: "gus", portBase: 21100 }, panes), { kind: "worker", name: "Gus", gone: false, base: 21100 });
  assert.deepEqual(logic.ownerOf({ ...base, port: 21100, pane: "Lou", portBase: 21100 }, panes), { kind: "worker", name: "Lou", gone: true, base: 21100 }, "its panel is closed: a leftover");
  assert.equal(logic.ownerOf({ ...base, port: 21100, pane: "Lou", portBase: 21100 }, null).gone, false, "without the panes nothing is claimed gone");
  assert.deepEqual(logic.ownerOf({ ...base, port: 21107 }, panes), { kind: "block", base: 21100 });
  assert.equal(logic.ownerOf({ ...base, port: 5432 }, panes), null);
});

test("a container publishing a port is shown on that port's row", () => {
  const containers = logic.parseDocker(fixture("docker-ps.jsonl"));
  const ports = [{ port: 5432, pid: 1, name: "com.docker.backend" }, { port: 21104, pid: 2, name: "x" }, { port: 9, pid: 3, name: "y" }].map((p) => ({ pane: null, portBase: null, ...p }));
  const rows = logic.portRows(ports, [], containers);
  assert.deepEqual(rows.map((r) => r.container && r.container.name), ["shop-db-1", "shop-api", null]);
  assert.equal(rows[1].owner.kind, "block");
});

test("the brief to an agent says what to check, what to do and what to report", () => {
  const since = () => "2 h ago";
  const row = { port: 21104, pid: 9052, name: "python.exe", startedAt: "2026-10-31T10:00:00Z", owner: { kind: "worker", name: "Lou", base: 21100 } };
  const port = logic.briefFor({ type: "port", row }, t, since);
  assert.match(port, /port 21104: python\.exe, pid 9052, started 2 h ago/);
  assert.match(port, /started in the pane of Lou/);
  assert.match(port, /Check that it is yours/);
  assert.match(port, /If it is not yours, leave it/);
  const free = logic.briefFor({ type: "port", row: { port: 21104, pid: null, name: null, startedAt: null, owner: { kind: "block", base: 21100 } } }, t, since);
  assert.match(free, /Unknown program, pid \?\./);
  assert.match(free, /worker port block 21100–21119/);
  const container = logic.briefFor({ type: "container", container: { name: "shop-api", id: "a81e0c55d9f3aaaa", image: "shop-api:dev", status: "Exited (1) 12 minutes ago", state: "exited" } }, t, since);
  assert.match(container, /Docker container shop-api \(id a81e0c55d9f3, image shop-api:dev, Exited \(1\) 12 minutes ago\)/);
  assert.match(logic.briefFor({ type: "port", row }, fakeUi("fr").i18n(logic.STRINGS), since), /Arrête ce qui écoute sur le port 21104/);
});

test("what a refusal means, and whether docker is off or just failing", () => {
  assert.equal(logic.classify({ code: "denied" }), "denied");
  assert.equal(logic.classify({ code: "not_found" }), "missing");
  assert.equal(logic.classify({ code: "busy" }), "busy");
  assert.equal(logic.classify({ timedOut: true }), "timeout");
  assert.equal(logic.classify({}), "failed");
  assert.equal(logic.dockerFault(fixture("docker-down.txt")), "stopped", "recorded on Windows with Docker Desktop off");
  assert.equal(logic.dockerFault(fixture("docker-down-unix.txt")), "stopped");
  assert.equal(logic.dockerFault("permission denied while trying to connect to the docker API"), "failed");
});

test("a log: ANSI gone, the last 200 lines, no empty tail", () => {
  assert.deepEqual(logic.logLines("a\n\u001b[31mred\u001b[0m\n\n"), ["a", "red"]);
  const many = Array.from({ length: 500 }, (_, i) => `line ${i}`).join("\n");
  const lines = logic.logLines(many);
  assert.equal(lines.length, 200);
  assert.equal(lines[0], "line 300");
  assert.deepEqual(logic.logLines(""), []);
});

test("the strict id pattern", () => {
  for (const good of ["3f1c9a7b2d44", "a".repeat(64)]) assert.ok(logic.ID_PATTERN.test(good), good);
  for (const bad of ["", "abc", "-3f1c9a7b2d44", "3F1C9A7B2D44", "3f1c9a7b2d4g", "3f1c9a7b2d44 --rm", "a".repeat(65), "3f1c9a7b2d44\n"]) assert.ok(!logic.ID_PATTERN.test(bad), JSON.stringify(bad));
});
