"use strict";
// The page's logic on recorded gh output (fixtures/*.json come from
// `gh run view/list` on this very repository; run-running.json is the success
// run with its status turned to "in progress", as gh writes it).
const test = require("node:test");
const assert = require("node:assert/strict");
const logic = require("../tabs/ci/logic.js");
const { fixture, fakeUi } = require("./helpers.js");

const success = JSON.parse(fixture("run-success.json"));
const failure = JSON.parse(fixture("run-failure.json"));
const running = JSON.parse(fixture("run-running.json"));
const at = (iso) => Date.parse(iso);
const en = fakeUi("en").i18n(logic.STRINGS);
const fr = fakeUi("fr").i18n(logic.STRINGS);

test("every French string exists, with its placeholders, and carries accents where French needs them", () => {
  const { en: english, fr: french } = logic.STRINGS;
  assert.deepEqual(Object.keys(french).sort(), Object.keys(english).sort());
  for (const key of Object.keys(english)) {
    const holes = (text) => (text.match(/\{\w+\}/g) || []).sort().join();
    assert.equal(holes(french[key]), holes(english[key]), key);
  }
  assert.match(french.failure, /É/);
  assert.match(french["err.no-auth.title"], /n'est pas connecté/);
});

test("French speaks as Hangar does: tu, workspace, Réglages (the glossary in src/i18n.ts)", () => {
  for (const [key, text] of Object.entries(logic.STRINGS.fr)) {
    assert.doesNotMatch(text, /\b(?:vous|votre|vos)\b|[a-zéè]ez\b/i, `${key}: « tu »`);
    assert.doesNotMatch(text, /espace de travail/i, `${key}: « workspace »`);
    assert.doesNotMatch(text, /Paramètres/, `${key}: « Réglages »`);
    assert.doesNotMatch(text, /’/, `${key}: the apostrophe Hangar's strings use`);
  }
  assert.match(logic.STRINGS.fr["err.denied.hint"], /Réglages › Kits/);
});

test("a running run: badge running, jobs with progress and the step going", () => {
  const now = at(running.startedAt) + 104000;
  const view = logic.runView(running, now);
  assert.equal(view.kind, "running");
  assert.equal(view.done, false);
  assert.equal(view.endedAt, null);
  assert.equal(view.name, "CI");
  assert.deepEqual(logic.badgeFor(running, en), { state: "running", text: "running" });
  assert.deepEqual(logic.badgeFor(running, fr), { state: "running", text: "en cours" });

  const going = view.jobs.find((job) => job.kind === "running");
  assert.equal(going.name, "rust (macos-latest)");
  assert.equal(going.at, 3, "the step it is at, two being done");
  assert.equal(going.total, 13);
  assert.equal(going.step, "Keep Defender out of the build");
  assert.equal(going.failedStep, null);
  assert.equal(view.jobs.filter((job) => job.kind === "queued").length, 2);
  // Time elapsed runs on with the clock until the run ends.
  assert.equal(view.elapsed, 104000);
  assert.equal(logic.runView(running, now + 16000).elapsed, view.elapsed + 16000);
});

test("how long a job took: nothing before it starts, the clock while it runs, its own once over", () => {
  const now = at(running.startedAt) + 104000;
  const jobs = logic.runView(running, now).jobs;
  const going = jobs.find((job) => job.kind === "running");
  const queued = jobs.find((job) => job.kind === "queued");
  assert.equal(logic.jobDuration(queued, now), null);
  assert.equal(logic.jobDuration(going, now + 1000), logic.jobDuration(going, now) + 1000);
  const [over] = logic.runView(failure, now).failedJobs;
  assert.equal(logic.jobDuration(over, now), at("2026-10-31T17:02:43Z") - at("2026-10-31T17:00:20Z"));
  assert.equal(logic.jobDuration(over, now + 60000), logic.jobDuration(over, now), "a job over no longer counts");
});

test("a successful run: badge success, time elapsed is the run's, no failed job", () => {
  const view = logic.runView(success, Date.now());
  assert.equal(view.kind, "success");
  assert.equal(view.done, true);
  assert.equal(view.endedAt, at(success.updatedAt));
  assert.equal(view.failedJobs.length, 0);
  assert.equal(view.otherJobs.length, view.jobs.length);
  assert.deepEqual(logic.badgeFor(success, en), { state: "success", text: "passed" });
  assert.deepEqual(logic.badgeFor(success, fr), { state: "success", text: "réussie" });
  const first = success.jobs.map((job) => at(job.startedAt)).sort()[0];
  const last = Math.max(at(success.updatedAt), ...success.jobs.map((job) => at(job.completedAt)));
  assert.equal(view.elapsed, last - Math.min(first, at(success.startedAt)));
  assert.equal(logic.runView(success, 0).elapsed, view.elapsed, "a finished run no longer counts");
});

test("a failed run: badge failure, the failed job and its failed step found", () => {
  const view = logic.runView(failure, Date.now());
  assert.equal(view.kind, "failure");
  assert.deepEqual(view.failedJobs.map((job) => job.name), ["rust (macos-latest)"]);
  assert.equal(view.failedJobs[0].failedStep, "Tests", "not the steps skipped around it");
  assert.equal(view.otherJobs.length, 3);
  assert.deepEqual(logic.badgeFor(failure, en), { state: "failure", text: "failed" });
  assert.equal(view.elapsed, 256000);
});

test("conclusions that are neither a pass nor a failure are neutral", () => {
  for (const conclusion of ["cancelled", "skipped", "neutral", "stale", "action_required", ""]) {
    assert.equal(logic.badgeFor({ status: "completed", conclusion }, en).state, "neutral", conclusion);
  }
  for (const conclusion of ["failure", "timed_out", "startup_failure"]) {
    assert.equal(logic.badgeFor({ status: "completed", conclusion }, en).state, "failure", conclusion);
  }
  assert.equal(logic.badgeFor({ status: "queued", conclusion: "" }, en).state, "running");
  assert.equal(logic.badgeFor({ status: "waiting", conclusion: "" }, en).text, "queued");
});

test("the word for a state says what a neutral end was, when it is known", () => {
  assert.equal(logic.wordOf({ status: "completed", conclusion: "cancelled" }), "cancelled");
  assert.equal(logic.wordOf({ status: "completed", conclusion: "skipped" }), "skipped");
  assert.equal(logic.wordOf({ status: "completed", conclusion: "stale" }), "neutral");
  assert.equal(logic.wordOf({ status: "completed", conclusion: "failure" }), "failure");
  assert.equal(logic.wordOf({ status: "in_progress" }), "running");
  for (const word of ["cancelled", "skipped", "neutral", "queued", "running", "success", "failure"]) {
    assert.notEqual(en(word), word);
    assert.notEqual(fr(word), word);
    assert.notEqual(en(`job.${word}`), `job.${word}`);
    assert.notEqual(fr(`job.${word}`), `job.${word}`);
  }
  // In French a run is feminine and a job is not.
  assert.equal(fr("cancelled"), "Annulée");
  assert.equal(fr("job.cancelled"), "Annulé");
  assert.equal(fr("job.failure"), "Échoué");
});

test("badge text fits tab.setBadge (12 characters) and titles tab.setTitle (40)", () => {
  for (const t of [en, fr]) {
    for (const key of ["badgeRunning", "badgeQueued", "badgeSuccess", "badgeFailure", "badgeNeutral"]) {
      assert.ok(Array.from(t(key)).length <= 12, key);
    }
  }
  const title = logic.oneLine("x".repeat(100) + "\n", 40);
  assert.equal(Array.from(title).length, 40);
  assert.equal(logic.oneLine("  a\n\tb\u202e c ", 40), "a b c");
});

test("a job not started has no start: GitHub's 0001-01-01 is none", () => {
  assert.equal(logic.elapsedMs({ status: "queued", jobs: [{ startedAt: "0001-01-01T00:00:00Z" }] }, Date.now()), null);
  assert.equal(logic.jobView({ status: "queued", startedAt: "0001-01-01T00:00:00Z" }).startedAt, null);
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
  // Each failure has words in both languages.
  for (const kind of ["no-gh", "no-auth", "no-repo", "run-not-found", "denied", "unavailable", "timeout", "error"]) {
    for (const t of [en, fr]) {
      assert.notEqual(t(`err.${kind}.title`), `err.${kind}.title`);
      assert.notEqual(t(`err.${kind}.hint`), `err.${kind}.hint`);
    }
  }
  // And what can be done about the two a person can fix, to copy.
  assert.equal(logic.ERROR_COPY["no-auth"], "gh auth login");
  assert.equal(logic.ERROR_COPY["no-gh"], "https://cli.github.com");
  assert.match(fr("err.no-gh.hint"), /gh auth login/);
});

test("the first error line of a real failed log is the panic, not one of the « ... ok » above it", () => {
  const log = logic.readLog(fixture("log-failed.txt"));
  const first = log.lines[log.errors[0]];
  assert.equal(first.kind, "error");
  assert.equal(first.text, "thread 'accounts::tests::an_added_account_is_listed_after_main_and_logged_out' (18252) panicked at src/accounts.rs:688:9:");
  // Everything before it is the list it was buried under, and what names the failure.
  const before = log.lines.slice(0, log.errors[0]);
  assert.equal(before[0].kind, "head");
  assert.ok(before.filter((line) => line.text.endsWith(" ... ok")).length > 125);
  assert.ok(before.every((line) => line.kind !== "error"));
  // A few lines of context above it say which test this is.
  assert.equal(log.lines[log.errors[0] - 2].text, "---- accounts::tests::an_added_account_is_listed_after_main_and_logged_out stdout ----");
});

test("every line of that log that says what went wrong is marked, and only those", () => {
  const log = logic.readLog(fixture("log-failed.txt"));
  assert.deepEqual(log.errors.map((index) => log.lines[index].text), [
    "thread 'accounts::tests::an_added_account_is_listed_after_main_and_logged_out' (18252) panicked at src/accounts.rs:688:9:",
    "assertion `left == right` failed",
    "error: test failed, to rerun pass `--lib`",
    "test result: FAILED. 502 passed; 1 failed; 2 ignored; 0 measured; 0 filtered out; finished in 45.41s",
    "Process completed with exit code 101.",
  ]);
  for (const index of log.errors) assert.equal(log.lines[index].kind, "error");
  assert.equal(log.lines.filter((line) => line.kind === "error").length, log.errors.length);
});

test("what reads as an error in other tools' logs, and what only mentions one", () => {
  for (const line of [
    "error[E0599]: no method named `x`",
    "fatal: not a git repository",
    "test remote::thing ... FAILED",
    "FAIL  src/lib/kitTabs.test.ts > a test",
    "--- FAIL: TestThing (0.00s)",
    "FAILED tests/test_a.py::test_b - assert 1 == 2",
    "npm ERR! code ELIFECYCLE",
    "TypeError: x is not a function",
    "java.io.IOException: gone",
  ]) {
    assert.equal(logic.isErrorLine(line), true, line);
  }
  for (const line of [
    "test remote::tailscale::tests::a_missing_cli_is_an_error_to_run_not_a_panic ... ok",
    "test error: handled ... ok",
    "0 errors",
    "Compiling errors-kit v1.0.0",
    "no errors found",
    "handles_failure_mode",
    "failures:",
  ]) {
    assert.equal(logic.isErrorLine(line), false, line);
  }
});

test("the failed log reads as text: no colour, no timestamp, no mark of GitHub's", () => {
  const log = logic.readLog(fixture("log-failed.txt"));
  assert.equal(log.lines[0].kind, "head");
  assert.equal(log.lines[0].text, "rust (macos-latest) › Tests");
  assert.equal(log.lines.filter((line) => line.kind === "head").length, 1);
  const text = log.lines.map((line) => line.text).join("\n");
  assert.doesNotMatch(text, /\u001b|\[36;1m/, "no colour codes");
  assert.doesNotMatch(text, /\d{4}-\d\d-\d\dT\d\d:/, "no timestamps");
  assert.doesNotMatch(text, /##\[error\]/);
  assert.equal(log.truncated, false);
  assert.equal(logic.readLog("a\n", true).truncated, true);
});

test("nothing of a log is cut but a line beyond reason", () => {
  const log = logic.readLog(fixture("log-failed.txt"));
  assert.equal(log.lines.length, fixture("log-failed.txt").trimEnd().split("\n").length + 1, "each line, and one heading");
  const long = logic.readLog("x".repeat(5000));
  assert.equal(long.lines[0].text.length, 2000);
  assert.ok(long.lines[0].text.endsWith("…"));
});

test("GitHub's marks are shown as what they mean, and a step's change as a heading", () => {
  const log = logic.readLog(
    [
      "build\tInstall\t2026-10-31T17:02:37.1Z ##[group]Run pnpm install",
      "build\tInstall\t2026-10-31T17:02:37.2Z ##[endgroup]",
      "build\tInstall\t2026-10-31T17:02:37.3Z ##[warning]Old lock file",
      "build\tTests\t2026-10-31T17:02:37.4Z ##[error]It broke",
    ].join("\n"),
  );
  assert.deepEqual(log.lines, [
    { text: "build › Install", kind: "head" },
    { text: "Run pnpm install" },
    { text: "Old lock file", kind: "warning" },
    { text: "build › Tests", kind: "head" },
    { text: "It broke", kind: "error" },
  ]);
  assert.deepEqual(log.errors, [4]);
});

test("a log without gh's tab columns still reads", () => {
  const log = logic.readLog("line one\r\n\r\nline two\n");
  assert.deepEqual(log.lines.map((line) => line.text), ["line one", "line two"]);
  assert.equal(logic.readLog("").lines.length, 0);
  assert.deepEqual(logic.readLog("").errors, []);
});

test("the recent runs for the picker", () => {
  const rows = logic.runList(JSON.parse(fixture("run-list.json")));
  assert.equal(rows.length, 15);
  assert.equal(rows[0].id, "41231065926");
  assert.equal(rows[0].kind, "success");
  assert.equal(rows[0].branch, "main");
  assert.equal(rows[0].createdAt, at("2026-10-31T18:46:53Z"));
  assert.equal(rows.find((row) => row.id === "41222364482").kind, "failure");
  assert.ok(rows.every((row) => /^[0-9]+$/.test(row.id)));
  assert.deepEqual(logic.runList("nope"), []);
});

test("only digits make a run id", () => {
  for (const good of ["1", "41231065926"]) assert.ok(logic.isRunId(good));
  for (const bad of ["", "-1", "12a", " 1", 12, null, undefined, "1\n2"]) assert.ok(!logic.isRunId(bad), String(bad));
});
