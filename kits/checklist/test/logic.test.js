"use strict";
const test = require("node:test");
const assert = require("node:assert/strict");
const { logic, LIST, LIST_PATH } = require("./helpers.js");

test("the path param is read like form input", () => {
  assert.deepEqual(logic.readPath({ path: "recette/login.json" }), { ok: true, path: "recette/login.json" });
  assert.deepEqual(logic.readPath({ path: " recette\\login.json " }), { ok: true, path: "recette/login.json" });
  assert.equal(logic.readPath({}).kind, "missing");
  assert.equal(logic.readPath({ path: "  " }).kind, "missing");
  for (const bad of ["../x.json", "/etc/passwd", "C:/x.json", "a/../b.json", ".hidden/x.json", "a/.x.json", "a//b.json", "a:b", "a\nb", "a/"]) {
    assert.equal(logic.readPath({ path: bad }).kind, "path", bad);
  }
  assert.equal(logic.readPath({ path: "a".repeat(1025) }).kind, "path");
  assert.equal(logic.readPath({ path: ["a"] }).kind, "path");
});

test("a good list is read, trimmed, and named after its file when it has no title", () => {
  const parsed = logic.parseList(JSON.stringify(LIST), LIST_PATH);
  assert.equal(parsed.ok, true);
  assert.equal(parsed.list.title, "Login and logout");
  assert.equal(parsed.list.steps.length, 3);
  const bare = logic.parseList(JSON.stringify({ steps: [{ text: "  Do it  " }] }), "recette/login.json");
  assert.deepEqual(bare.list, { title: "login.json", steps: [{ text: "Do it", expect: "" }] });
  assert.equal(logic.parseList("\uFEFF" + JSON.stringify(LIST), LIST_PATH).ok, true, "a BOM is dropped");
});

test("a malformed list says what is wrong", () => {
  const read = (text) => logic.parseList(text, "a.json");
  assert.equal(read("").kind, "empty");
  assert.equal(read("   \n").kind, "empty");
  assert.equal(read("{not json").kind, "json");
  assert.ok(read("{not json").detail.length > 0);
  assert.equal(read("[1]").code, "notObject");
  assert.equal(read("null").code, "notObject");
  assert.equal(read('{"steps":[]}').code, "noSteps");
  assert.equal(read('{"steps":"x"}').code, "noSteps");
  assert.equal(read('{"title":3,"steps":[{"text":"a"}]}').code, "titleType");
  assert.deepEqual([read('{"steps":["a"]}').code, read('{"steps":["a"]}').vars], ["stepNotObject", { n: 1 }]);
  const second = read('{"steps":[{"text":"a"},{"expect":"b"}]}');
  assert.deepEqual([second.code, second.vars], ["stepText", { n: 2 }]);
  assert.equal(read('{"steps":[{"text":"  "}]}').code, "stepText");
  assert.equal(read('{"steps":[{"text":"a","expect":4}]}').code, "stepExpect");
  assert.equal(read(JSON.stringify({ steps: [{ text: "x".repeat(1001) }] })).code, "stepLong");
  const many = read(JSON.stringify({ steps: Array.from({ length: 201 }, () => ({ text: "a" })) }));
  assert.deepEqual([many.code, many.vars], ["tooMany", { max: 200, n: 201 }]);
  for (const code of ["notObject", "noSteps", "tooMany", "stepNotObject", "stepText", "stepExpect", "stepLong", "titleType"]) {
    for (const lang of ["en", "fr"]) assert.notEqual(logic.t(lang, `shape.${code}`), `shape.${code}`, `${lang} ${code}`);
  }
});

test("a step's key follows its text, not its place", () => {
  const steps = [{ text: "a" }, { text: "b" }, { text: "a" }];
  const keys = logic.stepKeys(steps);
  assert.equal(new Set(keys).size, 3, "two identical texts are two steps");
  assert.equal(logic.stepKeys([{ text: "b" }, { text: "a" }, { text: "a" }])[0], keys[1]);
  assert.equal(logic.stepKeys([{ text: "a", expect: "other" }])[0], keys[0], "expect is not part of it");
});

test("the data path is derived from the list's path, and is safe", () => {
  const a = logic.dataPath("recette/login.json");
  assert.match(a, /^answers\/[A-Za-z0-9._-]+\.json$/);
  assert.notEqual(a, logic.dataPath("recette/login_json"));
  assert.notEqual(logic.dataPath("a/b.json"), logic.dataPath("a_b.json"), "a different path never shares answers");
  assert.equal(a, logic.dataPath("recette/login.json"));
  assert.ok(logic.dataPath("é/".repeat(500) + "x.json").length < 100);
});

test("saved answers are read safely", () => {
  const keys = logic.stepKeys(LIST.steps);
  const text = JSON.stringify({
    v: 1,
    answers: { [keys[0]]: { status: "ok" }, [keys[1]]: { status: "fail", note: "boom" }, nope: { status: "ok" }, [keys[2]]: { status: "weird" } },
    redo: { [keys[0]]: "ignored: answered", [keys[2]]: "was bad" },
    sentAt: 12,
  });
  const saved = logic.readSaved(text, keys);
  assert.deepEqual(saved.answers, { [keys[0]]: { status: "ok" }, [keys[1]]: { status: "fail", note: "boom" } });
  assert.deepEqual(saved.redo, { [keys[2]]: "was bad" });
  assert.equal(saved.sentAt, 12);
  for (const junk of ["", "not json", "null", "[]", '{"answers":3}']) {
    assert.deepEqual(logic.readSaved(junk, keys), { answers: {}, redo: {}, sentAt: null }, junk);
  }
  assert.equal(JSON.parse(logic.writeSaved(saved)).v, 1);
});

test("after the result went out, the failed steps come back to redo with their note", () => {
  const keys = logic.stepKeys(LIST.steps);
  const saved = { answers: { [keys[0]]: { status: "ok" }, [keys[1]]: { status: "fail", note: "blank page" }, [keys[2]]: { status: "skip" } }, redo: {}, sentAt: 5 };
  assert.equal(logic.markFailedForRedo(saved), true);
  assert.deepEqual(saved, { answers: { [keys[0]]: { status: "ok" }, [keys[2]]: { status: "skip" } }, redo: { [keys[1]]: "blank page" }, sentAt: null });
  assert.equal(logic.markFailedForRedo(saved), false, "nothing to do once done");
  const unsent = { answers: { [keys[1]]: { status: "fail", note: "x" } }, redo: {}, sentAt: null };
  assert.equal(logic.markFailedForRedo(unsent), false, "a failure not sent stays a failure");
  assert.equal(logic.statusOf(unsent, keys[1]), "fail");
});

test("progress, badge and title", () => {
  const keys = logic.stepKeys(LIST.steps);
  const t = (key, vars) => logic.t("en", key, vars);
  const saved = { answers: {}, redo: {}, sentAt: null };
  assert.deepEqual(logic.badgeFor(logic.progress(keys, saved), t), { state: "running", text: "0/3" });
  saved.answers[keys[0]] = { status: "ok" };
  saved.redo[keys[1]] = "x";
  const mid = logic.progress(keys, saved);
  assert.deepEqual([mid.answered, mid.redo, mid.todo, mid.state], [1, 1, 1, "running"]);
  saved.answers[keys[1]] = { status: "skip" };
  delete saved.redo[keys[1]];
  saved.answers[keys[2]] = { status: "fail", note: "" };
  const done = logic.progress(keys, saved);
  assert.equal(done.state, "failure");
  assert.deepEqual(logic.badgeFor(done, t), { state: "failure", text: "1 failed" });
  saved.answers[keys[2]] = { status: "ok" };
  assert.deepEqual(logic.badgeFor(logic.progress(keys, saved), t), { state: "success", text: "3/3" });
  assert.ok(logic.badgeFor(done, (k, v) => logic.t("fr", k, v)).text.length <= 12);
  assert.ok(logic.tabTitle({ title: "x".repeat(100) }).length <= 40);
});

test("the summary is plain, says what failed with the notes, then what was skipped", () => {
  const keys = logic.stepKeys(LIST.steps);
  const saved = {
    answers: { [keys[0]]: { status: "ok" }, [keys[1]]: { status: "fail", note: "The page stays blank." }, [keys[2]]: { status: "skip" } },
    redo: {},
    sentAt: null,
  };
  const text = logic.buildSummary(LIST, keys, saved, LIST_PATH);
  assert.match(text, /^Checklist results: "Login and logout" \(recette\/login\.json\)\./);
  assert.match(text, /3 steps: 1 OK, 1 failed, 1 skipped\./);
  assert.match(text, /FAILED:\n- Step 2: Sign in with the test account\.\n {2}Expected: The dashboard shows\.\n {2}Note: The page stays blank\./);
  assert.match(text, /SKIPPED:\n- Step 3: Press Sign out, then Back\./);
  assert.match(text, /Passed: steps 1\./);
  assert.match(text, /kit_tab_open/);
  assert.ok(text.indexOf("FAILED") < text.indexOf("SKIPPED"));
  assert.doesNotMatch(text, /NOT ANSWERED/);
  assert.doesNotMatch(text, /[*#`]/, "no markup: it is read as it is");
});

test("the summary names unanswered steps, steps to redo, and a missing note", () => {
  const keys = logic.stepKeys(LIST.steps);
  const saved = { answers: { [keys[0]]: { status: "fail", note: "" } }, redo: { [keys[1]]: "blank page" }, sentAt: null };
  const text = logic.buildSummary(LIST, keys, saved, LIST_PATH);
  assert.match(text, /Note: \(none\)/);
  assert.match(text, /NOT ANSWERED:\n- Step 3:/);
  assert.match(text, /NOT ANSWERED \(failed before, to redo\):\n- Step 2: .*\n {2}Failed before: blank page/);
  const clean = logic.buildSummary(LIST, keys, { answers: Object.fromEntries(keys.map((k) => [k, { status: "ok" }])), redo: {}, sentAt: null }, LIST_PATH);
  assert.match(clean, /Nothing failed\.$/);
});

test("a summary stays under what panes.send takes, whatever the list holds", () => {
  const steps = Array.from({ length: 200 }, (_, i) => ({ text: `step ${i} ` + "x".repeat(990), expect: "y".repeat(990) }));
  const keys = logic.stepKeys(steps);
  const saved = { answers: Object.fromEntries(keys.map((k) => [k, { status: "fail", note: "n".repeat(2000) }])), redo: {}, sentAt: null };
  const text = logic.buildSummary({ title: "t", steps }, keys, saved, "a.json");
  assert.ok(text.length <= 20000);
});

test("both languages hold the same keys, with their accents", () => {
  assert.deepEqual(Object.keys(logic.STRINGS.fr).sort(), Object.keys(logic.STRINGS.en).sort());
  assert.equal(logic.pickLang("fr-FR"), "fr");
  assert.equal(logic.pickLang("en-GB"), "en");
  assert.equal(logic.pickLang(undefined), "en");
  assert.equal(logic.t("fr", "send"), "Envoyer à l'agent");
  assert.equal(logic.t("fr", "problem"), "Problème");
  assert.equal(logic.t("fr", "stateFailure", { n: 1 }), "1 étape en échec");
  assert.equal(logic.t("fr", "stateFailure", { n: 3 }), "3 étapes en échec");
  assert.equal(logic.t("en", "stateFailure", { n: 3 }), "3 steps failed");
});
