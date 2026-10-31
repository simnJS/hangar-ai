"use strict";
// The hook: which commands count as a push, which runs belong to it, how the tab
// is asked for, and that the script itself always exits 0 and returns at once.
// Nothing here pushes, and nothing talks to a real Hangar or GitHub.
const test = require("node:test");
const assert = require("node:assert/strict");
const { spawnSync } = require("node:child_process");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { isGitPush, simpleCommands, recentRuns, findRuns, openTab } = require("../hooks/lib.js");
const { KIT } = require("./helpers.js");

test("what is a git push", () => {
  const pushes = [
    "git push",
    "git push origin main",
    "git push -u origin hangar/branch",
    "git push --force-with-lease",
    "git -C ../repo push",
    "git -c core.editor=true push origin HEAD",
    "cd repo && git push",
    "git add . && git commit -m 'x' && git push origin main",
    "git status; git push",
    "FOO=1 git push",
    "git push origin main 2>&1 | tail -5",
    'git commit -m "fix" && git push',
    "echo done\ngit push",
    "(cd sub && git push)",
  ];
  for (const command of pushes) assert.equal(isGitPush(command), true, command);
});

test("what is not", () => {
  const others = [
    "git push --help",
    "git push -h",
    "git push --dry-run",
    "git push -n origin main",
    "git push origin --delete old-branch",
    "echo git push",
    "echo 'git push'",
    'echo "run git push when ready"',
    'git commit -m "git push later"',
    "git pull",
    "git status",
    "git log --oneline",
    "git pushd",
    "gh pr create",
    "npm run push",
    "cat notes-on-git-push.txt",
    "grep 'git push' README.md",
    "git",
    "",
    "   ",
  ];
  for (const command of others) assert.equal(isGitPush(command), false, JSON.stringify(command));
  for (const odd of [undefined, null, 42, {}]) assert.equal(isGitPush(odd), false);
});

test("the command line is split on operators outside quotes", () => {
  assert.deepEqual(simpleCommands('a b && c "d && e" ; f'), [["a", "b"], ["c", "d && e"], ["f"]]);
  assert.deepEqual(simpleCommands("a\\ b c"), [["a b", "c"]]);
});

test("only recent runs of the push count, newest first", () => {
  const since = Date.parse("2026-10-31T18:00:00Z");
  const list = [
    { databaseId: 1, createdAt: "2026-10-31T17:00:00Z" }, // an hour old: a run of an earlier push
    { databaseId: 2, createdAt: "2026-10-31T17:59:30Z" },
    { databaseId: 3, createdAt: "2026-10-31T17:58:00Z" },
    { databaseId: "x", createdAt: "2026-10-31T17:59:30Z" },
    { databaseId: 4, createdAt: "not a date" },
  ];
  assert.deepEqual(recentRuns(list, since), ["2", "3"]);
  assert.deepEqual(recentRuns(list, since, 1), ["2"]);
  assert.deepEqual(recentRuns("nope", since), []);
});

function io({ sha = "a".repeat(40), answers = [], gitFails = false, ghError } = {}) {
  const log = { sleeps: [], gh: [] };
  let t = Date.parse("2026-10-31T18:00:00Z");
  return {
    log,
    git: async (args) => {
      assert.deepEqual(args, ["rev-parse", "HEAD"]);
      if (gitFails) throw Object.assign(new Error("not a repo"), { code: 128 });
      return sha + "\n";
    },
    gh: async (args) => {
      log.gh.push(args);
      if (ghError) throw ghError;
      return JSON.stringify(answers.shift() || []);
    },
    sleep: async (ms) => {
      log.sleeps.push(ms);
      t += ms;
    },
    now: () => t,
  };
}
const run = (id, createdAt = "2026-10-31T18:00:03Z") => ({ databaseId: id, createdAt });

test("the run takes seconds to appear: the search retries, then stops at once", async () => {
  const fake = io({ answers: [[], [], [run(7), run(8)]] });
  const ids = await findRuns(fake, { attempts: 8, delayMs: 4000 });
  assert.deepEqual(ids, ["7", "8"]);
  assert.deepEqual(fake.log.sleeps, [4000, 4000]);
  assert.deepEqual(fake.log.gh[0], ["run", "list", "--commit", "a".repeat(40), "--limit", "10", "--json", "databaseId,createdAt"]);
});

test("no run ever appears: gives up after its attempts", async () => {
  const fake = io({});
  assert.deepEqual(await findRuns(fake, { attempts: 4, delayMs: 10 }), []);
  assert.equal(fake.log.gh.length, 4);
});

test("gh absent or signed out, or no git: does nothing, without waiting", async () => {
  const absent = io({ ghError: Object.assign(new Error("spawn gh ENOENT"), { code: "ENOENT" }) });
  assert.deepEqual(await findRuns(absent), []);
  assert.equal(absent.log.gh.length, 1);
  assert.deepEqual(absent.log.sleeps, []);

  const signedOut = io({ ghError: Object.assign(new Error("exit 4"), { stderr: "To get started with GitHub CLI, please run:  gh auth login" }) });
  assert.deepEqual(await findRuns(signedOut), []);
  assert.equal(signedOut.log.gh.length, 1);

  assert.deepEqual(await findRuns(io({ gitFails: true })), []);
  assert.deepEqual(await findRuns(io({ sha: "not a sha" })), []);
});

test("the tab is opened through the documented route, two seconds at most", async () => {
  const seen = [];
  const ok = await openTab({
    url: "http://127.0.0.1:5555", token: "T", pane: "Cora 1", id: "4567",
    fetchImpl: async (url, init) => {
      seen.push({ url, init });
      return { ok: true };
    },
  });
  assert.equal(ok, true);
  assert.equal(seen[0].url, "http://127.0.0.1:5555/api/kit/tab/open?pane=Cora%201");
  assert.equal(seen[0].init.method, "POST");
  assert.equal(seen[0].init.headers.authorization, "Bearer T");
  assert.deepEqual(JSON.parse(seen[0].init.body), { kit: "ci-status", tab: "ci", params: { id: "4567" }, reveal: true });
  assert.ok(seen[0].init.signal instanceof AbortSignal);
});

test("a refused or unreachable Hangar is no error", async () => {
  const base = { url: "http://127.0.0.1:5555", token: "T", pane: "p", id: "1" };
  assert.equal(await openTab({ ...base, fetchImpl: async () => { throw new Error("ECONNREFUSED"); } }), false);
  assert.equal(await openTab({ ...base, fetchImpl: async () => ({ ok: false }) }), false);
  assert.equal(await openTab({ ...base, token: undefined, fetchImpl: async () => assert.fail("no token, no call") }), false);
  assert.equal(await openTab({ ...base, id: "1; rm -rf", fetchImpl: async () => assert.fail("not an id") }), false);
});

// The script itself. It is given a dead address and a folder that is no repository,
// so even a push it recognises can reach nothing.
function runHook(input, env = {}) {
  const cwd = fs.mkdtempSync(path.join(os.tmpdir(), "ci-status-hook-"));
  try {
    const started = Date.now();
    const result = spawnSync(process.execPath, [path.join(KIT, "hooks", "open-ci.js")], {
      input: typeof input === "string" ? input : JSON.stringify({ cwd, ...input }),
      env: { PATH: process.env.PATH, SystemRoot: process.env.SystemRoot, ...env },
      encoding: "utf8",
      timeout: 15000,
    });
    return { ...result, ms: Date.now() - started };
  } finally {
    fs.rmSync(cwd, { recursive: true, force: true });
  }
}
const HANGAR = { HANGAR_API_URL: "http://127.0.0.1:1", HANGAR_API_TOKEN: "t", HANGAR_PANE_ID: "p" };

test("the script exits 0 and says nothing: any input, any environment", () => {
  const cases = [
    ["not json", HANGAR],
    ["", HANGAR],
    [{ tool_input: { command: "git push" } }, {}], // not in a Hangar pane
    [{ tool_input: { command: "git status" } }, HANGAR],
    [{ tool_input: {} }, HANGAR],
    [{}, HANGAR],
    [{ tool_input: { command: "git push --help" } }, HANGAR],
    [{ tool_input: { command: "git push" } }, HANGAR], // recognised: searches detached, in a folder with no repo
    [{ tool_input: { command: "git push" }, cwd: "/does/not/exist" }, HANGAR],
  ];
  for (const [input, env] of cases) {
    const result = runHook(input, env);
    assert.equal(result.status, 0, JSON.stringify(input));
    assert.equal(result.stdout, "");
    assert.equal(result.stderr, "");
  }
});

test("the agent does not wait for the search", () => {
  const result = runHook({ tool_input: { command: "git push origin main" } }, HANGAR);
  assert.equal(result.status, 0);
  assert.ok(result.ms < 3000, `${result.ms} ms`);
});

test("the plugin wires the hook on Bash after tool use", () => {
  const hooks = JSON.parse(fs.readFileSync(path.join(KIT, "hooks", "hooks.json"), "utf8")).hooks;
  assert.deepEqual(Object.keys(hooks), ["PostToolUse"]);
  assert.equal(hooks.PostToolUse[0].matcher, "Bash");
  const command = hooks.PostToolUse[0].hooks[0];
  assert.equal(command.type, "command");
  assert.match(command.command, /^node "\$\{CLAUDE_PLUGIN_ROOT\}\/hooks\/open-ci\.js"$/);
  assert.ok(fs.existsSync(path.join(KIT, "hooks", "open-ci.js")));
});

test("the hook scripts use Node's own modules only", () => {
  for (const name of ["lib.js", "open-ci.js", "find-run.js"]) {
    const source = fs.readFileSync(path.join(KIT, "hooks", name), "utf8");
    for (const [, target] of source.matchAll(/require\(["']([^"']+)["']\)/g)) {
      assert.ok(target.startsWith("node:") || target.startsWith("./"), `${name} requires ${target}`);
    }
  }
});
