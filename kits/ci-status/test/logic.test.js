"use strict";
// The page's logic on recorded gh output (fixtures/*.json come from
// `gh run view/list` on this very repository; run-running.json is the success
// run with its status turned to "in progress", as gh writes it).
const test = require("node:test");
const assert = require("node:assert/strict");
const logic = require("../tabs/ci/logic.js");
const { fixture } = require("./helpers.js");

const success = JSON.parse(fixture("run-success.json"));
const failure = JSON.parse(fixture("run-failure.json"));
const running = JSON.parse(fixture("run-running.json"));
const at = (iso) => Date.parse(iso);

test("language follows navigator.language", () => {
  assert.equal(logic.pickLang("fr-FR"), "fr");
  assert.equal(logic.pickLang("fr"), "fr");
  assert.equal(logic.pickLang("en-US"), "en");
  assert.equal(logic.pickLang(undefined), "en");
  assert.equal(logic.t("fr", "success"), "Réussie");
  assert.equal(logic.t("en", "steps", { done: 3, total: 13 }), "3/13 steps");
});

test("every French string exists, with its placeholders, and carries accents where French needs them", () => {
  const { en, fr } = logic.STRINGS;
  assert.deepEqual(Object.keys(fr).sort(), Object.keys(en).sort());
  for (const key of Object.keys(en)) {
    const holes = (text) => (text.match(/\{\w+\}/g) || []).sort().join();
    assert.equal(holes(fr[key]), holes(en[key]), key);
  }
  assert.match(fr.failure, /É/);
  assert.match(fr["err.no-auth.title"], /n’est pas connecté/);
});

test("a running run: badge running, jobs with progress and the step going", () => {
  const now = at(running.startedAt) + 104000;
  const view = logic.runView(running, now);
  assert.equal(view.kind, "running");
  assert.equal(view.done, false);
  assert.equal(view.name, "CI");
  assert.deepEqual(logic.badgeFor(running, "en"), { state: "running", text: "running" });
  assert.deepEqual(logic.badgeFor(running, "fr"), { state: "running", text: "en cours" });

  const going = view.jobs.find((job) => job.kind === "running");
  assert.equal(going.name, "rust (macos-latest)");
  assert.equal(going.done, 2);
  assert.equal(going.total, 13);
  assert.equal(typeof going.step, "string");
  assert.equal(view.jobs.filter((job) => job.kind === "queued").length, 2);
  // Time elapsed runs on with the clock until the run ends.
  assert.equal(logic.formatDuration(view.elapsed), "1m 44s");
  assert.equal(logic.runView(running, now + 16000).elapsed, view.elapsed + 16000);
});

test("a successful run: badge success, time elapsed is the run's, no failed job", () => {
  const view = logic.runView(success, Date.now());
  assert.equal(view.kind, "success");
  assert.equal(view.done, true);
  assert.equal(view.failedJobs.length, 0);
  assert.deepEqual(logic.badgeFor(success, "en"), { state: "success", text: "passed" });
  assert.deepEqual(logic.badgeFor(success, "fr"), { state: "success", text: "réussie" });
  const first = success.jobs.map((job) => at(job.startedAt)).sort()[0];
  const last = Math.max(at(success.updatedAt), ...success.jobs.map((job) => at(job.completedAt)));
  assert.equal(view.elapsed, last - Math.min(first, at(success.startedAt)));
  assert.equal(logic.runView(success, 0).elapsed, view.elapsed, "a finished run no longer counts");
});

test("a failed run: badge failure, the failed job found", () => {
  const view = logic.runView(failure, Date.now());
  assert.equal(view.kind, "failure");
  assert.deepEqual(view.failedJobs.map((job) => job.name), ["rust (macos-latest)"]);
  assert.deepEqual(logic.badgeFor(failure, "en"), { state: "failure", text: "failed" });
  assert.equal(logic.formatDuration(view.elapsed), "4m 16s");
});

test("conclusions that are neither a pass nor a failure are neutral", () => {
  for (const conclusion of ["cancelled", "skipped", "neutral", "stale", "action_required", ""]) {
    assert.equal(logic.badgeFor({ status: "completed", conclusion }, "en").state, "neutral", conclusion);
  }
  for (const conclusion of ["failure", "timed_out", "startup_failure"]) {
    assert.equal(logic.badgeFor({ status: "completed", conclusion }, "en").state, "failure", conclusion);
  }
  assert.equal(logic.badgeFor({ status: "queued", conclusion: "" }, "en").state, "running");
  assert.equal(logic.badgeFor({ status: "waiting", conclusion: "" }, "en").text, "queued");
});

test("badge text fits tab.setBadge (12 characters) and titles tab.setTitle (40)", () => {
  for (const lang of ["en", "fr"]) {
    for (const key of ["badgeRunning", "badgeQueued", "badgeSuccess", "badgeFailure", "badgeNeutral"]) {
      assert.ok(Array.from(logic.t(lang, key)).length <= 12, `${lang} ${key}`);
    }
  }
  const title = logic.oneLine("x".repeat(100) + "\n", 40);
  assert.equal(Array.from(title).length, 40);
  assert.equal(logic.oneLine("  a\n\tb\u202e c ", 40), "a b c");
});

test("durations", () => {
  assert.equal(logic.formatDuration(0), "0s");
  assert.equal(logic.formatDuration(59999), "59s");
  assert.equal(logic.formatDuration(83000), "1m 23s");
  assert.equal(logic.formatDuration(3725000), "1h 02m");
  assert.equal(logic.formatDuration(null), "–");
  // A job not started has GitHub's "0001-01-01" start: it is no start.
  assert.equal(logic.elapsedMs({ status: "queued", jobs: [{ startedAt: "0001-01-01T00:00:00Z" }] }, Date.now()), null);
});

test("not logged in, no gh, no repository, no such run: told apart", () => {
  const stderr = fixture("not-logged-in.stderr");
  assert.equal(logic.classify({ exitCode: 4, stderr }), "no-auth");
  assert.equal(logic.classify({ code: "not_found" }), "no-gh");
  assert.equal(logic.classify({ exitCode: 1, stderr: "fatal: not a git repository (or any of the parent directories): .git" }), "no-repo");
  assert.equal(logic.classify({ exitCode: 1, stderr: "failed to run git: no git remotes found" }), "no-repo");
  assert.equal(logic.classify({ exitCode: 1, stderr: "could not resolve to a Run with the databaseId of 1: HTTP 404" }), "run-not-found");
  assert.equal(logic.classify({ code: "denied" }), "denied");
  assert.equal(logic.classify({ exitCode: null, timedOut: true }), "timeout");
  assert.equal(logic.classify({ exitCode: 1, stderr: "something odd" }), "error");
  assert.ok(logic.isFatal("no-auth") && !logic.isFatal("timeout") && !logic.isFatal("error"));
  // Each failure has words in both languages, with the fix in them.
  for (const kind of ["no-gh", "no-auth", "no-repo", "run-not-found", "denied", "unavailable", "timeout", "error"]) {
    for (const lang of ["en", "fr"]) {
      assert.notEqual(logic.t(lang, `err.${kind}.title`), `err.${kind}.title`);
      assert.notEqual(logic.t(lang, `err.${kind}.hint`), `err.${kind}.hint`);
    }
  }
  assert.match(logic.t("en", "err.no-auth.hint"), /gh auth login/);
  assert.match(logic.t("fr", "err.no-gh.hint"), /cli\.github\.com/);
});

test("the failed log is cut to its end, readable, errors marked", () => {
  const log = logic.trimLog(fixture("log-failed.txt"), 40);
  assert.equal(log.shown, 40);
  assert.ok(log.total > 40);
  assert.equal(log.lines[0].kind, "head");
  assert.match(log.lines[0].text, /rust \(macos-latest\) › Tests/);
  const text = log.lines.map((line) => line.text).join("\n");
  assert.doesNotMatch(text, /\u001b|\[36;1m/, "no colour codes");
  assert.doesNotMatch(text, /\d{4}-\d\d-\d\dT\d\d:/, "no timestamps");
  assert.doesNotMatch(text, /##\[error\]/);
  assert.ok(log.lines.some((line) => line.kind === "error"), "the error stands out");
  assert.ok(Math.max(...log.lines.map((line) => line.text.length)) <= 400);
});

test("a log without gh's tab columns still reads", () => {
  const log = logic.trimLog("line one\r\n\r\nline two\n", 40);
  assert.deepEqual(log.lines.map((line) => line.text), ["line one", "line two"]);
  assert.equal(logic.trimLog("").lines.length, 0);
});

test("the recent runs for the picker", () => {
  const rows = logic.runList(JSON.parse(fixture("run-list.json")));
  assert.equal(rows.length, 15);
  assert.equal(rows[0].id, "41231065926");
  assert.equal(rows[0].kind, "success");
  assert.equal(rows.find((row) => row.id === "41222364482").kind, "failure");
  assert.ok(rows.every((row) => /^[0-9]+$/.test(row.id)));
  assert.equal(logic.relativeTime(at("2026-10-31T18:46:53Z"), at("2026-10-31T19:00:00Z"), "en"), "13 min ago");
  assert.equal(logic.relativeTime(at("2026-10-31T18:46:53Z"), at("2026-10-31T19:00:00Z"), "fr"), "il y a 13 min");
  assert.equal(logic.relativeTime(at("2026-10-31T18:46:53Z"), at("2026-10-31T18:47:00Z"), "fr"), "à l’instant");
  assert.deepEqual(logic.runList("nope"), []);
});

test("only digits make a run id", () => {
  for (const good of ["1", "41231065926"]) assert.ok(logic.isRunId(good));
  for (const bad of ["", "-1", "12a", " 1", 12, null, undefined, "1\n2"]) assert.ok(!logic.isRunId(bad), String(bad));
});
