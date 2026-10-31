"use strict";
const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { spawnSync } = require("node:child_process");
const parsers = require("../tools/parsers");
const audit = require("../tools/audit");
const { fixture, tempFolder, KIT } = require("./helpers");

// Recorded: npm-*, pnpm-* (npm 11.6 and pnpm 10.28 on a project pinning lodash 4.17.15, minimist
// 1.2.0, left-pad 1.1.0 and semver 7.0.0). Written from each tool's documented output: yarn-*,
// cargo-*, pip-*, which were not installed where this kit was made.

test("npm audit: one entry per advisory, severity, title, fixed version and the lockfile's version", () => {
  const lock = JSON.parse(fixture("npm-package-lock.json"));
  const installed = { lodash: lock.packages["node_modules/lodash"].version, minimist: lock.packages["node_modules/minimist"].version };
  const list = parsers.npmAudit(fixture("npm-audit.json"), installed);
  const minimist = list.filter((item) => item.package === "minimist");
  assert.ok(minimist.length >= 1);
  assert.deepEqual(minimist.find((item) => item.severity === "critical"), {
    package: "minimist",
    installed: "1.2.0",
    severity: "critical",
    title: "Prototype Pollution in minimist",
    id: "GHSA-xvch-5gv4-984h",
    url: "https://github.com/advisories/GHSA-xvch-5gv4-984h",
    fixed: "1.2.6",
    vulnerable: ">=1.0.0 <1.2.6",
  });
  const lodash = list.filter((item) => item.package === "lodash");
  assert.ok(lodash.length >= 3);
  assert.ok(lodash.every((item) => item.installed === "4.17.15" && item.url && item.title));
  assert.ok(lodash.some((item) => item.fixed === "4.17.21"));
});

test("npm audit: a registry error is a failure, never an empty list", () => {
  assert.throws(() => parsers.npmAudit(fixture("npm-error.json")), /ENOTFOUND/);
  assert.throws(() => parsers.npmAudit("not json at all"), /no JSON/);
  assert.deepEqual(parsers.npmAudit(fixture("npm-clean-audit.json")), []);
});

test("npm outdated: major, minor or patch from the version in use to the latest", () => {
  const list = parsers.npmOutdated(fixture("npm-outdated.json"));
  assert.deepEqual(list.map((item) => [item.package, item.current, item.latest, item.kind]), [
    ["left-pad", "1.1.0", "1.3.0", "minor"],
    ["lodash", "4.17.15", "4.18.1", "minor"],
    ["minimist", "1.2.0", "1.2.8", "patch"],
  ]);
});

test("pnpm audit: advisories with their patched version", () => {
  const list = parsers.pnpmAudit(fixture("pnpm-audit.json"));
  assert.equal(list.length, 9);
  const minimist = list.find((item) => item.package === "minimist" && item.severity === "critical");
  assert.equal(minimist.installed, "1.2.0");
  assert.equal(minimist.fixed, "1.2.6");
  assert.equal(minimist.id, "GHSA-xvch-5gv4-984h");
  assert.deepEqual(parsers.severity("medium"), "moderate");
  assert.ok(list.every((item) => ["critical", "high", "moderate", "low"].includes(item.severity)));
  assert.throws(() => parsers.pnpmAudit('{"nothing":true}'), /not a pnpm audit report/);
});

test("pnpm outdated: dev dependencies are marked", () => {
  const list = parsers.pnpmOutdated(fixture("pnpm-outdated.json"));
  assert.equal(list.length, 4);
  assert.equal(list.find((item) => item.package === "semver").dev, true);
  assert.equal(list.find((item) => item.package === "lodash").dev, false);
});

test("yarn classic: advisories from ndjson, a table of outdated packages, an error is a failure", () => {
  const list = parsers.yarnAudit(fixture("yarn-audit.ndjson"));
  assert.deepEqual(list.map((item) => [item.package, item.severity, item.installed, item.fixed]), [
    ["minimist", "critical", "1.2.0", "1.2.6"],
    ["semver", "high", "7.0.0", "7.5.2"],
  ]);
  const outdated = parsers.yarnOutdated(fixture("yarn-outdated.ndjson"));
  assert.deepEqual(outdated.map((item) => [item.package, item.kind, item.dev]), [
    ["left-pad", "minor", false],
    ["semver", "minor", true],
    ["react", "major", false],
  ]);
  assert.throws(() => parsers.yarnAudit(fixture("yarn-error.ndjson")), /ENOTFOUND/);
  assert.deepEqual(parsers.yarnOutdated('{"type":"info","data":"nothing"}\n'), []);
});

test("yarn berry: one line per package", () => {
  const [item] = parsers.yarnBerryAudit(fixture("yarn-berry-audit.ndjson"));
  assert.deepEqual([item.package, item.installed, item.severity, item.fixed], ["minimist", "1.2.0", "critical", "1.2.6"]);
});

test("cargo: the advisory and its patched version; a crate version behind; RustSec's severity is unknown", () => {
  const [item] = parsers.cargoAudit(fixture("cargo-audit.json"));
  assert.deepEqual(item, {
    package: "time",
    installed: "0.1.45",
    severity: "unknown",
    title: "Potential segfault in the time crate",
    id: "RUSTSEC-2020-0071",
    url: "https://github.com/time-rs/time/issues/293",
    fixed: "0.2.23",
    vulnerable: null,
  });
  assert.deepEqual(parsers.cargoAudit(fixture("cargo-audit-clean.json")), []);
  const outdated = parsers.cargoOutdated(fixture("cargo-outdated.json"));
  assert.deepEqual(outdated.map((entry) => [entry.package, entry.current, entry.latest, entry.kind, entry.dev]), [
    ["serde", "1.0.100", "1.0.219", "patch", false],
    ["rand", "0.7.3", "0.9.1", "minor", false],
    ["tempfile", "3.1.0", "3.20.0", "minor", true],
  ]);
});

test("pip: pip-audit's findings, and only the outdated packages the requirements name", () => {
  const [item] = parsers.pipAudit(fixture("pip-audit.json"));
  assert.deepEqual([item.package, item.installed, item.id, item.fixed, item.severity], ["flask", "0.5", "PYSEC-2018-66", "0.12.3", "unknown"]);
  assert.match(item.title, /^The Pallets Project flask/);
  const names = parsers.requirementNames(fixture("requirements.txt"));
  assert.deepEqual(names, ["flask", "requests"]);
  const outdated = parsers.pipOutdated(fixture("pip-list-outdated.json"), names);
  assert.deepEqual(outdated.map((entry) => [entry.package, entry.kind]), [["requests", "minor"], ["Flask", "major"]]);
});

test("version kinds", () => {
  assert.equal(parsers.kindOf("1.2.3", "2.0.0"), "major");
  assert.equal(parsers.kindOf("1.2.3", "1.3.0"), "minor");
  assert.equal(parsers.kindOf("1.2.3", "1.2.4"), "patch");
  assert.equal(parsers.kindOf("1.2.3", "1.2.3"), null);
  assert.equal(parsers.kindOf("2.0.0", "1.9.0"), null, "ahead of the latest is not behind");
  assert.equal(parsers.kindOf("workspace:*", "1.0.0"), null);
  assert.equal(parsers.fixedFromPatched("<0.0.0"), null);
  assert.equal(parsers.fixedFromPatched(">=1.2.6"), "1.2.6");
  assert.equal(parsers.fixedFromVulnerable(">=3.7.0 <4.17.19"), "4.17.19");
});

// ---- the command ----------------------------------------------------------------------------

function project(t, files) {
  const dir = tempFolder(t);
  for (const [name, text] of Object.entries(files)) fs.writeFileSync(path.join(dir, name), text);
  return dir;
}

/** A runner answering with recorded outputs by "program arg arg", and noting every call. */
function recorded(answers) {
  const calls = [];
  const runner = async (program, args) => {
    const key = [program, ...args].join(" ");
    calls.push(key);
    if (!(key in answers)) throw new Error(`unexpected command: ${key}`);
    return answers[key];
  };
  return { runner, calls };
}
const out = (stdout, code = 1, stderr = "") => ({ code, stdout, stderr, timedOut: false });

test("detects the manager from its lockfile, and says so when there is none", (t) => {
  assert.deepEqual(audit.detect(project(t, { "pnpm-lock.yaml": "", "Cargo.lock": "" })), [
    { manager: "pnpm", lockfile: "pnpm-lock.yaml" },
    { manager: "cargo", lockfile: "Cargo.lock" },
  ]);
  assert.deepEqual(audit.detect(project(t, { "yarn.lock": "" })), [{ manager: "yarn", lockfile: "yarn.lock" }]);
  assert.deepEqual(audit.detect(project(t, { "npm-shrinkwrap.json": "{}" })), [{ manager: "npm", lockfile: "npm-shrinkwrap.json" }]);
  assert.deepEqual(audit.detect(project(t, { "requirements.txt": "flask==0.5\n" })), [{ manager: "pip", lockfile: "requirements.txt" }]);
  assert.deepEqual(audit.detect(project(t, { "package.json": "{}" })), [], "a package.json alone is not a lockfile");
});

test("two JavaScript lockfiles: the one package.json declares, else pnpm, yarn, npm", (t) => {
  const both = { "pnpm-lock.yaml": "", "package-lock.json": "{}" };
  assert.equal(audit.detect(project(t, both))[0].manager, "pnpm");
  assert.equal(audit.detect(project(t, { ...both, "package.json": '{"packageManager":"npm@11.0.0"}' }))[0].manager, "npm");
  assert.equal(audit.detect(project(t, { ...both, "package.json": "{broken" }))[0].manager, "pnpm");
});

test("a project with no lockfile: nothing run, status no-lockfile", async (t) => {
  const { runner, calls } = recorded({});
  const result = await audit.run(project(t, { "package.json": "{}" }), { runner, now: () => 5 });
  assert.deepEqual(result, {
    schema: 1,
    at: 5,
    status: "no-lockfile",
    counts: { vulnerabilities: { critical: 0, high: 0, moderate: 0, low: 0, unknown: 0 }, outdated: { major: 0, minor: 0, patch: 0 } },
    managers: [],
  });
  assert.deepEqual(calls, []);
});

test("npm: recorded audit and outdated, sorted by severity then by kind", async (t) => {
  const dir = project(t, { "package-lock.json": fixture("npm-package-lock.json") });
  const { runner } = recorded({
    "npm audit --json": out(fixture("npm-audit.json")),
    "npm outdated --json": out(fixture("npm-outdated.json")),
  });
  const result = await audit.run(dir, { runner });
  assert.equal(result.status, "ok");
  const [npm] = result.managers;
  assert.deepEqual([npm.manager, npm.lockfile, npm.status, npm.steps], ["npm", "package-lock.json", "ok", { audit: "ok", outdated: "ok" }]);
  const order = npm.vulnerabilities.map((item) => item.severity);
  assert.deepEqual(order, [...order].sort((a, b) => parsers.SEVERITIES.indexOf(a) - parsers.SEVERITIES.indexOf(b)));
  assert.equal(npm.vulnerabilities[0].severity, "critical");
  assert.equal(npm.vulnerabilities[0].installed, "1.2.0", "the lockfile gives the version npm audit leaves out");
  assert.deepEqual(npm.outdated.map((item) => item.kind), ["minor", "minor", "patch"]);
  assert.deepEqual(result.counts.outdated, { major: 0, minor: 2, patch: 1 });
  assert.equal(result.counts.vulnerabilities.critical, 1);
});

test("pnpm with nothing outdated prints nothing: that is an empty list, not a failure", async (t) => {
  const dir = project(t, { "pnpm-lock.yaml": "" });
  const { runner } = recorded({
    "pnpm audit --json": out(JSON.stringify({ advisories: {}, metadata: {} }), 0),
    "pnpm outdated --format json": out("", 0),
  });
  const [pnpm] = (await audit.run(dir, { runner })).managers;
  assert.deepEqual([pnpm.status, pnpm.vulnerabilities, pnpm.outdated, pnpm.errors], ["ok", [], [], []]);
});

test("a tool that is absent is told apart from a clean project", async (t) => {
  const dir = project(t, { "pnpm-lock.yaml": "" });
  const { runner } = recorded({ "pnpm audit --json": { missing: true }, "pnpm outdated --format json": { missing: true } });
  const result = await audit.run(dir, { runner });
  assert.equal(result.status, "tool-missing");
  assert.deepEqual(result.managers[0].missing, ["pnpm"]);
  assert.equal(result.managers[0].status, "tool-missing");
  assert.deepEqual(result.managers[0].vulnerabilities, []);
});

test("cargo without the cargo-audit plugin: cargo is there, the subcommand is not", async (t) => {
  const dir = project(t, { "Cargo.lock": "" });
  const stderr = "error: no such command: `audit`\n";
  const { runner } = recorded({
    "cargo audit --json": out("", 101, stderr),
    "cargo outdated --format json --workspace --exit-code 0": out(fixture("cargo-outdated.json"), 0),
  });
  const [cargo] = (await audit.run(dir, { runner })).managers;
  assert.equal(cargo.status, "ok", "outdated still answered");
  assert.deepEqual(cargo.missing, ["cargo-audit"]);
  assert.deepEqual(cargo.steps, { audit: "missing", outdated: "ok" });
  assert.equal(cargo.outdated.length, 3);
});

test("pip: pip-audit and pip, the outdated list cut to the requirements", async (t) => {
  const dir = project(t, { "requirements.txt": fixture("requirements.txt") });
  const { runner } = recorded({
    "pip-audit -r requirements.txt --no-deps --disable-pip -f json": out(fixture("pip-audit.json")),
    "pip list --outdated --format json": out(fixture("pip-list-outdated.json"), 0),
  });
  const [pip] = (await audit.run(dir, { runner })).managers;
  assert.equal(pip.vulnerabilities.length, 1);
  assert.deepEqual(pip.outdated.map((item) => item.package), ["Flask", "requests"], "major first");
});

test("yarn: classic and berry are told apart by yarn --version", async (t) => {
  const dir = project(t, { "yarn.lock": "" });
  const classic = recorded({
    "yarn --version": out("1.22.22\n", 0),
    "yarn audit --json": out(fixture("yarn-audit.ndjson")),
    "yarn outdated --json": out(fixture("yarn-outdated.ndjson")),
  });
  const [one] = (await audit.run(dir, { runner: classic.runner })).managers;
  assert.deepEqual([one.vulnerabilities.length, one.outdated.map((item) => item.kind)], [2, ["major", "minor", "minor"]]);
  const berry = recorded({ "yarn --version": out("4.5.0\n", 0), "yarn npm audit --json": out(fixture("yarn-berry-audit.ndjson")) });
  const [two] = (await audit.run(dir, { runner: berry.runner })).managers;
  assert.equal(two.vulnerabilities.length, 1);
  assert.equal(two.steps.outdated, "unsupported");
  assert.equal(two.status, "ok");
});

test("a failing audit (offline) is an error with its reason, never a clean bill", async (t) => {
  const dir = project(t, { "package-lock.json": "{}" });
  const { runner } = recorded({
    "npm audit --json": out(fixture("npm-error.json")),
    "npm outdated --json": { code: null, stdout: "", stderr: "", timedOut: true },
  });
  const result = await audit.run(dir, { runner });
  const [npm] = result.managers;
  assert.equal(npm.status, "error");
  assert.equal(result.status, "ok", "the tool ran: the failure is the manager's");
  assert.match(npm.errors.find((item) => item.step === "audit").detail, /ENOTFOUND/);
  assert.equal(npm.errors.find((item) => item.step === "outdated").detail, "timed out");
});

test("the lists are capped, and it says so", async (t) => {
  const advisories = {};
  for (let i = 0; i < 600; i++) advisories[i] = { module_name: `p${i}`, severity: "low", title: "t", url: "u", findings: [{ version: "1.0.0" }], patched_versions: ">=2.0.0" };
  const dir = project(t, { "pnpm-lock.yaml": "" });
  const { runner } = recorded({ "pnpm audit --json": out(JSON.stringify({ advisories })), "pnpm outdated --format json": out("{}", 0) });
  const [pnpm] = (await audit.run(dir, { runner })).managers;
  assert.equal(pnpm.vulnerabilities.length, 500);
  assert.equal(pnpm.truncated, true);
});

// ---- read-only ------------------------------------------------------------------------------

test("every command it may run is read-only, and nothing else can be run", async () => {
  const forbidden = /^(install|i|ci|add|remove|update|upgrade|fix|dedupe|prune|uninstall|audit-fix|fetch|publish)$/;
  for (const [program, lists] of Object.entries(audit.READ_ONLY)) {
    for (const args of lists) {
      assert.ok(!args.some((arg) => forbidden.test(arg)), `${program} ${args.join(" ")}`);
      assert.ok(!args.includes("--fix") && !args.includes("--force"), `${program} ${args.join(" ")}`);
    }
  }
  await assert.rejects(() => audit.exec("npm", ["install"], { cwd: __dirname }), /not on the read-only list/);
  await assert.rejects(() => audit.exec("npm", ["audit", "fix"], { cwd: __dirname }), /not on the read-only list/);
  await assert.rejects(() => audit.exec("node", ["-e", "1"], { cwd: __dirname }), /not on the read-only list/);
});

test("the script's source never names an installing command", () => {
  const source = fs.readFileSync(path.join(KIT, "tools", "audit.js"), "utf8");
  assert.doesNotMatch(source, /writeFile|appendFile|unlink|rmSync|rename|mkdir|copyFile/);
  assert.doesNotMatch(source, /["'`](install|update|upgrade|fix|add|remove)["'`]/);
});

test("the program is looked up on the PATH only: never in the project's folder", (t) => {
  const dir = tempFolder(t);
  const name = process.platform === "win32" ? "pnpm.cmd" : "pnpm";
  fs.writeFileSync(path.join(dir, name), "echo planted\n");
  const previous = process.cwd();
  process.chdir(dir);
  try {
    assert.equal(audit.which("pnpm", { PATH: `.${path.delimiter}relative${path.delimiter}`, PATHEXT: ".CMD" }), null);
  } finally {
    process.chdir(previous);
  }
  assert.equal(audit.which("pnpm", { PATH: dir, PATHEXT: ".CMD" }) !== null, true, "an absolute PATH entry is searched");
});

// ---- as the process Hangar starts -----------------------------------------------------------

function runScript(cwd, env = {}) {
  const base = { ...process.env, ...env };
  const result = spawnSync(process.execPath, [path.join(KIT, "tools", "audit.js")], { cwd, env: base, encoding: "utf8" });
  return { code: result.status, stdout: result.stdout, stderr: result.stderr };
}

test("as a process, in a project with no lockfile: one JSON document, exit 0", (t) => {
  const dir = project(t, { "package.json": "{}" });
  const result = runScript(dir);
  assert.equal(result.code, 0);
  const document = JSON.parse(result.stdout);
  assert.equal(document.status, "no-lockfile");
  assert.deepEqual(fs.readdirSync(dir), ["package.json"], "it wrote nothing");
});

test("as a process, with the tool absent: tool-missing, and the lockfile untouched", (t) => {
  const lock = '{"lockfileVersion":3,"packages":{}}';
  const dir = project(t, { "package-lock.json": lock });
  const empty = tempFolder(t);
  const result = runScript(dir, { PATH: empty, Path: empty });
  assert.equal(result.code, 0);
  const document = JSON.parse(result.stdout);
  assert.equal(document.status, "tool-missing");
  assert.deepEqual(document.managers[0].missing, ["npm"]);
  assert.equal(fs.readFileSync(path.join(dir, "package-lock.json"), "utf8"), lock);
  assert.deepEqual(fs.readdirSync(dir), ["package-lock.json"]);
});
