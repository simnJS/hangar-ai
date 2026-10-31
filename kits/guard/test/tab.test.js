"use strict";
// The tab against a fake window.hangar: its logic, its controller, what it draws, in
// both languages, and what it does when Hangar denies it.
const test = require("node:test");
const assert = require("node:assert/strict");
const logic = require("../tabs/guard/logic.js");
const view = require("../tabs/guard/view.js");
const { createController, POLL_MS } = require("../tabs/guard/controller.js");
const { WHY } = require("../hooks/rules.js");
const { fakeClock, fakeHangar, fakeDocument, FakeNode, settle } = require("./helpers.js");

const NOW = Date.parse("2026-10-31T18:00:00Z");
const KEY_A = "a".repeat(64);
const KEY_B = "b".repeat(64);

const refusal = (key, at, extra = {}) => ({
  type: "refusal",
  at,
  key,
  agent: "Ava",
  agentId: "pane-1",
  tool: "Bash",
  command: "git reset --hard",
  length: 16,
  rule: "git-reset-hard",
  kind: "any",
  subject: "",
  why: "git reset --hard throws away uncommitted work, which git cannot bring back",
  folder: "C:\\work\\project",
  ...extra,
});
const lines = (...events) => events.map((event) => JSON.stringify(event)).join("\n") + "\n";

async function started({ lang = "en", files = {}, ...options } = {}) {
  const clock = fakeClock(NOW);
  const hangar = fakeHangar({ files, ...options });
  const root = new FakeNode("div");
  const actions = {};
  const controller = createController({
    hangar, logic, lang, now: clock.now, setTimer: clock.setTimer, clearTimer: clock.clearTimer,
    onChange: (state) => view.render(root, state, { doc: fakeDocument, logic, lang, now: clock.now, actions }),
  });
  Object.assign(actions, {
    allow: (key) => controller.allow(key),
    clear: () => controller.clear(),
    retry: () => controller.retry(),
  });
  await controller.start();
  await settle();
  return { clock, hangar, root, controller };
}
const buttons = (root) => root.find((node) => node.tag === "button");
const buttonNamed = (root, name) => buttons(root).find((node) => node.text === name);

test("languages", () => {
  assert.equal(logic.pickLang("fr-FR"), "fr");
  assert.equal(logic.pickLang("FR"), "fr");
  assert.equal(logic.pickLang("en-US"), "en");
  assert.equal(logic.pickLang("de"), "en");
  assert.equal(logic.pickLang(undefined), "en");
  assert.deepEqual(Object.keys(logic.STRINGS.fr).filter((key) => !key.startsWith("why.")).sort(), Object.keys(logic.STRINGS.en).filter((key) => !key.startsWith("why.")).sort());
});

test("every reason the hook can give reads in French, and in English as it told the agent", () => {
  for (const code of Object.keys(WHY)) {
    assert.ok(logic.STRINGS.fr[`why.${code}`], code);
    const [rule, kind] = code.split("/");
    const row = { rule, kind, subject: ".env", why: WHY[code].replace("{subject}", ".env") };
    assert.match(logic.whyFor(row, "en"), /^[A-Z].*\.$/, code);
    assert.ok(logic.whyFor(row, "en").toLowerCase().startsWith(row.why.slice(0, 5).toLowerCase()), code);
    if (WHY[code].includes("{subject}")) assert.ok(logic.whyFor(row, "fr").includes(".env"), code);
  }
  assert.equal(logic.whyFor({ rule: "new-rule", kind: "x", why: "it does a new thing" }, "fr"), "It does a new thing.", "a rule the tab does not know yet");
});

test("parsing the log passes over what is not a whole line", () => {
  const text = `${JSON.stringify(refusal(KEY_A, 1))}\n{"type":"refusal","at":2,"key":"cut sho\n\n42\nnull\n{"no":"time"}\n`;
  assert.equal(logic.parseLines(text).length, 1);
  assert.deepEqual(logic.parseLines(""), []);
  assert.deepEqual(logic.parseLines(undefined), []);
});

test("rows: one per agent and command, newest first, until a pass is used", () => {
  const rows = logic.buildRows([
    refusal(KEY_A, 1),
    refusal(KEY_B, 2, { command: "git clean -fd" }),
    refusal(KEY_A, 3),
  ]);
  assert.deepEqual(rows.map((row) => [row.key, row.count, row.at]), [[KEY_A, 2, 3], [KEY_B, 1, 2]]);

  const after = logic.buildRows([refusal(KEY_A, 1), { type: "pass-used", at: 2, key: KEY_A }, refusal(KEY_A, 3)]);
  assert.deepEqual(after.map((row) => [row.count, row.usedAt]), [[1, null], [1, 2]], "refused again after: a new row");

  assert.equal(logic.buildRows([refusal(KEY_A, 1), refusal(KEY_B, 5)], 3).length, 1, "what was cleared stays out");
  assert.equal(logic.buildRows([refusal("../../evil", 1), refusal("A".repeat(64), 1)]).length, 0, "a key is 64 hex digits, or nothing");
  assert.equal(logic.buildRows([{ type: "pass-used", at: 1, key: KEY_A }]).length, 0);
  assert.equal(logic.buildRows([refusal(KEY_A, 1, { agent: 42, command: null })])[0].agent, "?");
});

test("the pass: the row's key and agent, two minutes", () => {
  const pass = logic.passFor(logic.buildRows([refusal(KEY_A, 1)])[0], NOW);
  assert.equal(pass.key, KEY_A);
  assert.equal(pass.agentId, "pane-1");
  assert.equal(pass.expiresAt - pass.createdAt, 120000);
  assert.equal(logic.passPath(KEY_A), `passes/${KEY_A}.json`);
});

test("where a row stands", () => {
  const row = logic.buildRows([refusal(KEY_A, NOW)])[0];
  const grant = { grantedAt: NOW + 1, expiresAt: NOW + 120001 };
  assert.deepEqual(logic.passState(row, undefined, NOW), { state: "none" });
  assert.deepEqual(logic.passState(row, grant, NOW + 1001), { state: "active", remainingMs: 119000 });
  assert.deepEqual(logic.passState(row, grant, NOW + 120001), { state: "expired" });
  assert.deepEqual(logic.passState(row, { grantedAt: NOW - 5, expiresAt: NOW + 100 }, NOW), { state: "none" }, "refused since the click");
  assert.deepEqual(logic.passState({ ...row, usedAt: 9 }, grant, NOW), { state: "used", at: 9 });
});

test("a pass read back from its file", () => {
  const good = JSON.stringify({ key: KEY_A, createdAt: NOW, expiresAt: NOW + 1000 });
  assert.deepEqual(logic.grantFromPass(good, KEY_A, NOW), { grantedAt: NOW, expiresAt: NOW + 1000 });
  assert.equal(logic.grantFromPass(good, KEY_A, NOW + 1000), null);
  assert.equal(logic.grantFromPass(good, KEY_B, NOW), null);
  assert.equal(logic.grantFromPass("", KEY_A, NOW), null);
  assert.equal(logic.grantFromPass("{", KEY_A, NOW), null);
});

test("small things", () => {
  assert.equal(logic.formatRemaining(120000), "2:00");
  assert.equal(logic.formatRemaining(61001), "1:02");
  assert.equal(logic.formatRemaining(0), "0:00");
  assert.equal(logic.folderName("C:\\work\\project\\"), "project");
  assert.equal(logic.folderName("/home/dev/project"), "project");
  assert.equal(logic.folderName(""), "");
  assert.equal(logic.relativeTime(NOW - 10000, NOW, "en"), "just now");
  assert.equal(logic.relativeTime(NOW - 45000, NOW, "en"), "45 s ago");
  assert.equal(logic.relativeTime(NOW - 180000, NOW, "fr"), "il y a 3 min");
  assert.equal(logic.relativeTime(NOW - 7200000, NOW, "en"), "2 h ago");
  assert.equal(logic.relativeTime(NOW - 172800000, NOW, "fr"), "il y a 2 j");
  assert.equal(logic.t("en", "nope"), "nope");
});

test("the empty list: says so, no badge, and the limits are there", async (t) => {
  const { root, hangar, controller } = await started();
  t.after(() => controller.dispose());
  assert.match(root.text, /Nothing refused yet/);
  assert.match(root.text, /It reads the command line, and nothing else/);
  assert.match(root.text, /It is no sandbox: a script can do anything once started/);
  assert.equal(buttons(root).length, 0);
  assert.deepEqual(hangar.callsTo("tab.setBadge").pop().args, { state: null });
  assert.deepEqual(hangar.callsTo("tab.setTitle").pop().args, { title: "Guardrails" });
});

test("a refusal: agent, command, why, when, and one button", async (t) => {
  const files = { [logic.LOG]: lines(refusal(KEY_A, NOW - 180000, { agent: "Ava · Explore" })) };
  const { root, hangar, controller } = await started({ files });
  t.after(() => controller.dispose());
  assert.match(root.text, /Ava · Explore/);
  assert.equal(root.find((node) => node.tag === "code")[0].text, "git reset --hard");
  assert.match(root.text, /Git reset --hard throws away uncommitted work/);
  assert.match(root.text, /3 min ago/);
  assert.match(root.text, /project/);
  assert.ok(buttonNamed(root, "Allow once"));
  assert.deepEqual(hangar.callsTo("tab.setBadge").pop().args, { state: "failure", text: "1" });
});

test("in French, with accents, and the limits too", async (t) => {
  const files = { [logic.LOG]: lines(refusal(KEY_A, NOW - 180000)) };
  const { root, hangar, controller } = await started({ files, lang: "fr" });
  t.after(() => controller.dispose());
  assert.deepEqual(hangar.callsTo("tab.setTitle").pop().args, { title: "Garde-fous" });
  assert.match(root.text, /Appels refusés/);
  assert.match(root.text, /jette le travail non commité/);
  assert.match(root.text, /il y a 3 min/);
  assert.ok(buttonNamed(root, "Autoriser une fois"));
  assert.match(root.text, /Ce n’est pas un bac à sable : un script peut tout faire une fois lancé/);
});

test("the click writes the pass, and the row says what to do next", async (t) => {
  const files = { [logic.LOG]: lines(refusal(KEY_A, NOW - 1000)) };
  const { root, hangar, clock, controller } = await started({ files });
  t.after(() => controller.dispose());
  await buttonNamed(root, "Allow once").onclick();
  await settle();

  const write = hangar.callsTo("data.write").pop();
  assert.equal(write.args.path, `passes/${KEY_A}.json`);
  const pass = JSON.parse(write.args.text);
  assert.equal(pass.key, KEY_A);
  assert.equal(pass.agentId, "pane-1");
  assert.equal(pass.expiresAt, NOW + 120000);

  assert.match(root.text, /Allowed once, 2:00 left\. Tell Ava to retry this exact command\./);
  assert.equal(buttonNamed(root, "Allow once"), undefined);

  await clock.advance(POLL_MS * 5);
  assert.match(root.text, /1:50 left/, "the time left goes down");

  await clock.advance(120000);
  assert.match(root.text, /expired unused/);
  assert.ok(buttonNamed(root, "Allow once"), "and can be given again");
});

test("nothing is ever allowed without its own click: no such button, no such pass", async (t) => {
  const files = { [logic.LOG]: lines(refusal(KEY_A, NOW - 1000), refusal(KEY_B, NOW - 500, { command: "git clean -fd" })) };
  const { root, hangar, controller } = await started({ files });
  t.after(() => controller.dispose());
  assert.equal(buttons(root).filter((node) => node.text === "Allow once").length, 2);
  assert.ok(buttons(root).every((node) => !/all|always|every/i.test(node.text.replace("Allow once", "").replace("Clear the list", ""))));
  assert.equal(hangar.callsTo("data.write").length, 0);
  await controller.allow(KEY_A);
  const paths = hangar.callsTo("data.write").map((call) => call.args.path);
  assert.deepEqual(paths, [`passes/${KEY_A}.json`], "that one");
  await controller.allow("../../evil");
  await controller.allow(KEY_B.replace("b", "c"));
  assert.equal(hangar.callsTo("data.write").length, 1, "a key no row holds writes nothing");
});

test("a pass being written, or a row already through, writes nothing more", async (t) => {
  const files = { [logic.LOG]: lines(refusal(KEY_A, NOW - 1000)) };
  const { hangar, controller } = await started({ files });
  t.after(() => controller.dispose());
  const first = controller.allow(KEY_A);
  assert.equal(controller.state.pending[KEY_A], true);
  await Promise.all([first, controller.allow(KEY_A)]);
  assert.equal(hangar.callsTo("data.write").length, 1);
  assert.equal(controller.state.pending[KEY_A], undefined);

  files[logic.LOG] += lines({ type: "pass-used", at: NOW, key: KEY_A });
  await controller.retry();
  await controller.allow(KEY_A);
  assert.equal(hangar.callsTo("data.write").length, 1);
});

test("a retry seen in the log: the row says so, the badge drops", async (t) => {
  const files = { [logic.LOG]: lines(refusal(KEY_A, NOW - 10000)) };
  const { root, hangar, clock, controller } = await started({ files });
  t.after(() => controller.dispose());
  await controller.allow(KEY_A);
  files[logic.LOG] += lines({ type: "pass-used", at: NOW + 1000, key: KEY_A });
  await clock.advance(POLL_MS);
  assert.match(root.text, /Let through once, just now/);
  assert.deepEqual(hangar.callsTo("tab.setBadge").pop().args, { state: null });
  assert.equal(buttonNamed(root, "Allow once"), undefined);
});

test("a new refusal shows up by itself", async (t) => {
  const files = {};
  const { root, hangar, clock, controller } = await started({ files });
  t.after(() => controller.dispose());
  assert.match(root.text, /Nothing refused yet/);
  files[logic.LOG] = lines(refusal(KEY_A, NOW));
  await clock.advance(POLL_MS);
  assert.match(root.text, /git reset --hard/);
  assert.deepEqual(hangar.callsTo("tab.setBadge").pop().args, { state: "failure", text: "1" });
  files[logic.LOG] += lines(refusal(KEY_B, NOW, { command: "git clean -fd" }));
  await clock.advance(POLL_MS);
  assert.deepEqual(hangar.callsTo("tab.setBadge").pop().args, { state: "failure", text: "2" });
});

test("the badge is only set when it changes", async (t) => {
  const files = { [logic.LOG]: lines(refusal(KEY_A, NOW)) };
  const { hangar, clock, controller } = await started({ files });
  t.after(() => controller.dispose());
  await clock.advance(POLL_MS * 10);
  assert.equal(hangar.callsTo("tab.setBadge").length, 1);
});

test("a reload finds its passes, from their files", async (t) => {
  const files = {
    [logic.LOG]: lines(refusal(KEY_A, NOW - 30000)),
    [logic.passPath(KEY_A)]: JSON.stringify({ key: KEY_A, createdAt: NOW - 20000, expiresAt: NOW + 100000 }),
  };
  const { root, controller } = await started({ files });
  t.after(() => controller.dispose());
  assert.match(root.text, /Allowed once, 1:40 left/);
});

test("clearing: the list empties, stays so after a reload, newer ones show", async (t) => {
  const files = { [logic.LOG]: lines(refusal(KEY_A, NOW - 1000)) };
  const { root, hangar, clock, controller } = await started({ files });
  t.after(() => controller.dispose());
  await buttonNamed(root, "Clear the list").onclick();
  await settle();
  assert.match(root.text, /Nothing refused yet/);
  assert.deepEqual(hangar.callsTo("tab.setBadge").pop().args, { state: null });
  assert.deepEqual(JSON.parse(files[logic.STATE]), { clearedAt: NOW });
  assert.ok(files[logic.LOG], "the log itself is the hook's, and stays");

  const again = await started({ files });
  t.after(() => again.controller.dispose());
  assert.match(again.root.text, /Nothing refused yet/);

  await clock.advance(1000);
  files[logic.LOG] += lines(refusal(KEY_B, clock.now(), { command: "git clean -fd" }));
  await clock.advance(POLL_MS);
  assert.match(root.text, /git clean -fd/);
});

test("denied reading: says what to do, no badge, and comes back with the grant", async (t) => {
  const { root, hangar, controller } = await started({ denyRead: true });
  t.after(() => controller.dispose());
  assert.match(root.text, /This tab may not read the kit’s data/);
  assert.match(root.text, /Kits menu/);
  assert.match(root.text, /It is no sandbox/, "the limits are still said");
  assert.equal(root.find((node) => node.attributes.role === "alert").length, 1);
  assert.deepEqual(hangar.callsTo("tab.setBadge").pop().args, { state: null });
  assert.ok(buttonNamed(root, "Try again"));
});

test("a grant taken back while it runs: the next read says so", async (t) => {
  const files = { [logic.LOG]: lines(refusal(KEY_A, NOW)) };
  const clock = fakeClock(NOW);
  let denied = false;
  const inner = fakeHangar({ files });
  const hangar = { ...inner, call: async (method, args) => {
    if (denied && method === "data.read") throw Object.assign(new Error("no"), { code: "denied" });
    return inner.call(method, args);
  } };
  const controller = createController({ hangar, logic, now: clock.now, setTimer: clock.setTimer, clearTimer: clock.clearTimer });
  t.after(() => controller.dispose());
  await controller.start();
  assert.equal(controller.state.view, "list");
  denied = true;
  await clock.advance(POLL_MS);
  assert.equal(controller.state.view, "denied");
  assert.deepEqual(inner.callsTo("tab.setBadge").pop().args, { state: null });
  denied = false;
  await clock.advance(POLL_MS);
  assert.equal(controller.state.view, "list", "granted again, it comes back by itself");
  assert.equal(controller.state.rows.length, 1);
  assert.deepEqual(inner.callsTo("tab.setBadge").pop().args, { state: "failure", text: "1" });
});

test("denied writing: the list still shows, the row says why nothing happened", async (t) => {
  const files = { [logic.LOG]: lines(refusal(KEY_A, NOW)) };
  const { root, controller } = await started({ files, denyWrite: true });
  t.after(() => controller.dispose());
  await buttonNamed(root, "Allow once").onclick();
  await settle();
  assert.match(root.text, /may not write passes: accept data\.write in the Kits menu/);
  assert.ok(buttonNamed(root, "Allow once"), "still there to try again");
  assert.equal(controller.state.grants[KEY_A], undefined);
  await buttonNamed(root, "Clear the list").onclick();
  await settle();
  assert.match(root.text, /could not be cleared/);
  assert.match(root.text, /git reset --hard/);
});

test("over the rate limit, or a failed write: said on the row", async (t) => {
  const files = { [logic.LOG]: lines(refusal(KEY_A, NOW)) };
  const limited = await started({ files, writeError: "rate_limited" });
  t.after(() => limited.controller.dispose());
  await limited.controller.allow(KEY_A);
  assert.match(limited.root.text, /Too many writes in a minute/);
  const failed = await started({ files, writeError: "failed", lang: "fr" });
  t.after(() => failed.controller.dispose());
  await failed.controller.allow(KEY_A);
  assert.match(failed.root.text, /Le laissez-passer n’a pas pu être écrit : could not: failed/);
});

test("a read that fails: says so, tries again, and keeps what it shows", async (t) => {
  const first = await started({ readError: "failed" });
  t.after(() => first.controller.dispose());
  assert.match(first.root.text, /The refusals could not be read/);
  assert.ok(buttonNamed(first.root, "Try again"));

  const files = { [logic.LOG]: lines(refusal(KEY_A, NOW)) };
  const clock = fakeClock(NOW);
  let broken = false;
  const inner = fakeHangar({ files });
  const hangar = { ...inner, call: async (method, args) => {
    if (broken && method === "data.read") throw Object.assign(new Error("no"), { code: "failed" });
    return inner.call(method, args);
  } };
  const root = new FakeNode("div");
  const controller = createController({
    hangar, logic, now: clock.now, setTimer: clock.setTimer, clearTimer: clock.clearTimer,
    onChange: (state) => view.render(root, state, { doc: fakeDocument, logic, lang: "en", now: clock.now, actions: {} }),
  });
  t.after(() => controller.dispose());
  await controller.start();
  broken = true;
  await clock.advance(POLL_MS);
  assert.match(root.text, /The last read failed/);
  assert.match(root.text, /git reset --hard/);
  broken = false;
  await clock.advance(POLL_MS);
  assert.doesNotMatch(root.text, /The last read failed/);
});

test("the guard's own failures are said, not hidden", async (t) => {
  const files = {
    [logic.ERRORS]: lines({ at: NOW - 2, message: "first" }, { at: NOW - 1, message: ".hangar/guard.json is not valid JSON" }),
  };
  const { root, controller } = await started({ files });
  t.after(() => controller.dispose());
  assert.match(root.text, /The guard failed by itself/);
  assert.match(root.text, /2 call\(s\) went through unchecked/);
  assert.match(root.text, /guard\.json is not valid JSON/);
  assert.doesNotMatch(root.text, /first/);
  assert.equal(root.find((node) => node.attributes.role === "alert").length, 1);
});

test("what an agent wrote is text, never markup", async (t) => {
  const hostile = "<img src=x onerror=alert(1)><script>alert(1)</script>";
  const files = { [logic.LOG]: lines(refusal(KEY_A, NOW, { command: hostile, agent: hostile, folder: `/x/${hostile}`, why: hostile, subject: hostile })) };
  const { root, controller } = await started({ files });
  t.after(() => controller.dispose());
  assert.ok(root.text.includes(hostile));
  assert.equal(root.find((node) => ["img", "script"].includes(node.tag)).length, 0);
});

test("a command cut for the log says so", async (t) => {
  const files = { [logic.LOG]: lines(refusal(KEY_A, NOW, { command: "x".repeat(2000), length: 5123 })) };
  const { root, controller } = await started({ files });
  t.after(() => controller.dispose());
  assert.match(root.text, /Cut for display: 5123 characters in all/);
});

test("a tab that is gone stops polling", async () => {
  const { clock, hangar, controller } = await started();
  const before = hangar.callsTo("data.read").length;
  controller.dispose();
  assert.equal(clock.pending(), 0);
  await clock.advance(POLL_MS * 3);
  assert.equal(hangar.callsTo("data.read").length, before);
});
