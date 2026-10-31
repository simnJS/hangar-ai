"use strict";
// The hook as the process Claude Code starts: what it prints, that it exits 0 whatever
// happens, that its own failures let the call through and are logged, its overrides,
// and the route to the tab. Each command here is only read: the hook runs nothing.
const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const http = require("node:http");
const path = require("node:path");
const { execFileSync } = require("node:child_process");
const { KIT, tempFolder, runHook, bashEvent, readLog } = require("./helpers.js");
const { openTab } = require("../hooks/open-tab.js");
const { createContext, mergeRules } = require("../hooks/context.js");

function setup(t) {
  const root = tempFolder(t);
  const project = path.join(root, "project");
  const data = path.join(root, "kit-data");
  fs.mkdirSync(project, { recursive: true });
  return { project, data, dir: path.join(data, "guard"), env: { HANGAR_KITS_DATA: data, CLAUDE_PROJECT_DIR: project, HANGAR_PANE_ID: "pane-1", HANGAR_PANE_NAME: "Ava" } };
}

test("a refusal: Claude Code's deny, on stdout, exit 0, and written down", async (t) => {
  const { project, dir, env } = setup(t);
  const { code, stdout, stderr } = await runHook(bashEvent("git reset --hard", project), env);
  assert.equal(code, 0);
  assert.equal(stderr, "");
  const answer = JSON.parse(stdout);
  assert.deepEqual(Object.keys(answer), ["hookSpecificOutput"]);
  assert.equal(answer.hookSpecificOutput.hookEventName, "PreToolUse");
  assert.equal(answer.hookSpecificOutput.permissionDecision, "deny");
  assert.match(answer.hookSpecificOutput.permissionDecisionReason, /git reset --hard throws away uncommitted work/);
  assert.match(answer.hookSpecificOutput.permissionDecisionReason, /Do not look for another way/);
  assert.match(answer.hookSpecificOutput.permissionDecisionReason, /Garde-fous/);

  const [event] = readLog(dir);
  assert.equal(event.type, "refusal");
  assert.equal(event.agent, "Ava");
  assert.equal(event.agentId, "pane-1");
  assert.equal(event.tool, "Bash");
  assert.equal(event.command, "git reset --hard");
  assert.equal(event.rule, "git-reset-hard");
  assert.match(event.key, /^[0-9a-f]{64}$/);
  assert.ok(Number.isFinite(event.at));
});

test("an allowed call: nothing on stdout, exit 0, nothing written", async (t) => {
  const { project, dir, env } = setup(t);
  for (const command of ["npm test", "rm -rf node_modules", "rm -rf dist"]) {
    const { code, stdout } = await runHook(bashEvent(command, project), env);
    assert.equal(code, 0, command);
    assert.equal(stdout, "", command);
  }
  assert.equal(fs.existsSync(dir), false);
});

test("the other tools, by their real input", async (t) => {
  const { project, dir, env } = setup(t);
  const event = (tool_name, tool_input) => JSON.stringify({ session_id: "s", cwd: project, hook_event_name: "PreToolUse", tool_name, tool_input });
  const denied = async (tool, input) => JSON.parse((await runHook(event(tool, input), env)).stdout).hookSpecificOutput.permissionDecision;
  assert.equal(await denied("Read", { file_path: path.join(project, ".env") }), "deny");
  assert.equal(await denied("Edit", { file_path: path.join(project, ".env"), old_string: "a", new_string: "b" }), "deny");
  assert.equal(await denied("Write", { file_path: path.join(project, ".env"), content: "TOKEN=very-secret" }), "deny");
  assert.equal(await denied("Grep", { pattern: "KEY", path: path.join(project, ".env") }), "deny");
  assert.equal((await runHook(event("Read", { file_path: path.join(project, "a.ts") }), env)).stdout, "");
  // What a Write or an Edit would have held stays out of the log.
  assert.equal(fs.readFileSync(path.join(dir, "refusals.jsonl"), "utf8").includes("very-secret"), false);
});

test("its own failures let the call through, exit 0, and are logged", async (t) => {
  const { project, dir, env } = setup(t);
  for (const garbage of ["", "not json", "{", "null", "[]", "42"]) {
    const { code, stdout } = await runHook(garbage, env);
    assert.equal(code, 0, garbage);
    assert.equal(stdout, "", garbage);
  }
  const logged = readLog(dir, "errors.jsonl");
  assert.ok(logged.some((entry) => /JSON/.test(entry.message)), "a failure to read the event says so");
  assert.equal(readLog(dir).length, 0, "no refusal among them");

  // Nesting past what it reads: through, and logged.
  let deep = "git reset --hard";
  for (let i = 0; i < 80; i++) deep = `echo $(${deep})`;
  const { code, stdout } = await runHook(bashEvent(deep, project), env);
  assert.equal(code, 0);
  assert.equal(stdout, "");
  assert.ok(readLog(dir, "errors.jsonl").some((entry) => /too deep/.test(entry.message)));
});

test("broken rules let the call through, exit 0, and are logged", async (t) => {
  const { project, data, dir, env } = setup(t);
  const copy = path.join(data, "kit-copy");
  fs.cpSync(path.join(KIT, "hooks"), path.join(copy, "hooks"), { recursive: true });
  fs.writeFileSync(path.join(copy, "rules.json"), "{ broken");
  const { code, stdout } = await runHook(bashEvent("git reset --hard", project), env, path.join(copy, "hooks", "guard.js"));
  assert.equal(code, 0);
  assert.equal(stdout, "");
  assert.ok(readLog(dir, "errors.jsonl").length >= 1);
});

test("a hook that cannot write still refuses, and an allowed call still passes", async (t) => {
  const { project, data, env } = setup(t);
  fs.mkdirSync(data, { recursive: true });
  const notAFolder = path.join(data, "file");
  fs.writeFileSync(notAFolder, "x");
  const blocked = { ...env, HANGAR_KITS_DATA: notAFolder };
  const refused = await runHook(bashEvent("git reset --hard", project), blocked);
  assert.equal(refused.code, 0);
  assert.equal(JSON.parse(refused.stdout).hookSpecificOutput.permissionDecision, "deny");
  const allowed = await runHook(bashEvent("npm test", project), blocked);
  assert.equal(allowed.code, 0);
  assert.equal(allowed.stdout, "");
});

test("the project's overrides: a list adds, a value replaces, a rule goes off", async (t) => {
  const { project, dir, env } = setup(t);
  const hangar = path.join(project, ".hangar");
  fs.mkdirSync(hangar);
  fs.writeFileSync(path.join(hangar, "guard.json"), JSON.stringify({
    gitResetHard: { enabled: false },
    secrets: { except: [".env.test"], paths: ["my-secret.txt"] },
    forcePush: { defaultBranches: ["develop"] },
  }));
  const denied = async (command) => Boolean((await runHook(bashEvent(command, project), env)).stdout);
  assert.equal(await denied("git reset --hard"), false, "off");
  assert.equal(await denied("cat .env.test"), false, "an exception added");
  assert.equal(await denied("cat .env"), true, "the kit's own still hold");
  assert.equal(await denied("cat my-secret.txt"), true, "a path added");
  assert.equal(await denied("git push -f origin develop"), true, "a branch added");
  assert.equal(await denied("git push -f origin main"), true, "the kit's branch still counts");
  assert.equal(readLog(dir, "errors.jsonl").length, 0);
});

test("broken overrides leave the kit's rules, and are reported", async (t) => {
  const { project, dir, env } = setup(t);
  const hangar = path.join(project, ".hangar");
  fs.mkdirSync(hangar);
  fs.writeFileSync(path.join(hangar, "guard.json"), "{ not json");
  assert.ok((await runHook(bashEvent("git reset --hard", project), env)).stdout, "still refused");
  assert.match(readLog(dir, "errors.jsonl").pop().message, /guard\.json is not valid JSON/);

  fs.writeFileSync(path.join(hangar, "guard.json"), JSON.stringify({ gitResetHrd: { enabled: false } }));
  assert.ok((await runHook(bashEvent("git reset --hard", project), env)).stdout, "a mistyped rule turns nothing off");
  assert.match(readLog(dir, "errors.jsonl").pop().message, /unknown rule "gitResetHrd"/);
});

test("merging overrides keeps out what does not fit", () => {
  const defaults = { a: { enabled: true, list: ["x"], words: [["y"]], text: "t" } };
  const notes = [];
  const merged = mergeRules(defaults, {
    a: { enabled: "no", list: ["z", 1, ""], words: [["w"], "bad", [1]], text: "u", nope: 1 },
    b: {},
  }, (note) => notes.push(note));
  assert.deepEqual(merged.a, { enabled: true, list: ["x", "z"], words: [["y"], ["w"]], text: "u" });
  assert.equal(notes.length, 5);
  assert.deepEqual(defaults.a.list, ["x"], "the defaults stay as they are");
  assert.deepEqual(mergeRules(defaults, "nope", () => undefined), defaults);
  assert.deepEqual(mergeRules(defaults, null, () => undefined), defaults);
});

test("a bad regular expression in the rules falls back, and is reported", () => {
  const rules = JSON.parse(fs.readFileSync(path.join(KIT, "rules.json"), "utf8"));
  rules.dropDatabase.sql = "(";
  const notes = [];
  const ctx = createContext({ cwd: "/p" }, { env: {}, platform: "linux", home: "/h", tmp: "/tmp", rules, dataDir: "/d", pluginRoot: null, findTop: () => null, report: (note) => notes.push(note) });
  assert.match(notes[0], /not a regular expression/);
  assert.ok(ctx.sqlPattern.test("DROP DATABASE a"));
});

test("force pushes read the real branch, the worktree's too, and origin/HEAD", async (t) => {
  const { project, env } = setup(t);
  const gitEnv = { ...process.env, GIT_CONFIG_GLOBAL: path.join(project, "no-config"), GIT_CONFIG_NOSYSTEM: "1" };
  for (const name of ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_COMMON_DIR"]) delete gitEnv[name];
  const git = (...args) => execFileSync("git", args, { cwd: project, stdio: "pipe", env: gitEnv });
  git("init", "-q", "-b", "main");
  git("-c", "user.name=t", "-c", "user.email=t@example.com", "commit", "-q", "--allow-empty", "-m", "first");
  const denied = async (command, cwd = project) => Boolean((await runHook(bashEvent(command, cwd), { ...env, CLAUDE_PROJECT_DIR: cwd })).stdout);
  assert.equal(await denied("git push -f"), true, "on main");
  git("checkout", "-q", "-b", "feature");
  assert.equal(await denied("git push -f"), false, "on a feature branch");
  assert.equal(await denied("git push -f origin HEAD"), false);

  const tree = path.join(project, "..", "tree");
  git("worktree", "add", "-q", "-b", "other", tree);
  assert.equal(await denied("git push -f", tree), false, "in a worktree, its own branch");
  assert.equal(await denied("rm -rf dist", tree), false, "and its own folders");
  git("update-ref", "refs/remotes/origin/develop", "HEAD");
  git("symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/develop");
  assert.equal(await denied("git push -f origin develop"), true, "what origin/HEAD names");
  assert.equal(await denied("git push -f origin develop", tree), true, "seen from the worktree too");
});

test("deleting a worktree's own folders passes when the session's project is the main one", async (t) => {
  const { project, data, env } = setup(t);
  const tree = path.join(data, "tree");
  fs.mkdirSync(path.join(tree, ".git"), { recursive: true });
  const denied = async (command) => Boolean((await runHook(bashEvent(command, tree), { ...env, CLAUDE_PROJECT_DIR: project })).stdout);
  assert.equal(await denied("rm -rf dist"), false);
  assert.equal(await denied("rm -rf ."), true, "the worktree itself is a project");
  assert.equal(await denied("rm -rf .git"), true);
  assert.equal(await denied(`rm -rf "${path.join(project, "dist").replace(/\\/g, "/")}"`), false, "the main project is still the project");
});

test("asking for the tab: the route, the token, and no focus taken", async () => {
  const seen = [];
  const fetchImpl = async (url, init) => {
    seen.push({ url, init });
    return { ok: true };
  };
  assert.equal(await openTab({ url: "http://127.0.0.1:1", token: "tok", pane: "pane/1", fetchImpl }), true);
  assert.equal(seen[0].url, "http://127.0.0.1:1/api/kit/tab/open?pane=pane%2F1");
  assert.equal(seen[0].init.method, "POST");
  assert.equal(seen[0].init.headers.authorization, "Bearer tok");
  assert.deepEqual(JSON.parse(seen[0].init.body), { kit: "guard", tab: "guard", reveal: false });
  assert.ok(seen[0].init.signal, "a call with a time limit");

  assert.equal(await openTab({ url: "", token: "t", pane: "p", fetchImpl }), false);
  assert.equal(await openTab({ url: "http://x", token: "", pane: "p", fetchImpl }), false);
  assert.equal(await openTab({ url: "http://x", token: "t", pane: "", fetchImpl }), false);
  const failing = async () => {
    throw new Error("connection refused");
  };
  assert.equal(await openTab({ url: "http://x", token: "t", pane: "p", fetchImpl: failing }), false);
  assert.equal(await openTab({ url: "http://x", token: "t", pane: "p", fetchImpl: async () => ({ ok: false }) }), false);
});

const testPort = () => (process.env.HANGAR_PORT_BASE ? Number(process.env.HANGAR_PORT_BASE) + 7 : 0);

test("a refusal asks Hangar for the tab, and does not wait for its answer", async (t) => {
  const { project, env } = setup(t);
  let got;
  const asked = new Promise((resolve) => (got = resolve));
  // A Hangar that takes the request and never answers it.
  const server = http.createServer((request) => {
    let body = "";
    request.on("data", (chunk) => (body += chunk));
    request.on("end", () => got({ url: request.url, auth: request.headers.authorization, body }));
  });
  await new Promise((resolve) => server.listen(testPort(), "127.0.0.1", resolve));
  t.after(() => {
    server.closeAllConnections();
    server.close();
  });
  const { port } = server.address();

  // What a call that asks nothing takes on this machine: the refusal may not add the
  // two seconds the route is given.
  const baseline = (await runHook(bashEvent("npm test", project), env)).ms;
  const { code, stdout, ms } = await runHook(bashEvent("git reset --hard", project), {
    ...env,
    HANGAR_API_URL: `http://127.0.0.1:${port}`,
    HANGAR_API_TOKEN: "tok",
  });
  assert.equal(code, 0);
  assert.equal(JSON.parse(stdout).hookSpecificOutput.permissionDecision, "deny");
  assert.ok(ms < baseline + 1500, `the refusal took ${ms} ms against ${baseline} ms: it must not wait for the route`);

  const request = await asked;
  assert.equal(request.url, "/api/kit/tab/open?pane=pane-1");
  assert.equal(request.auth, "Bearer tok");
  assert.deepEqual(JSON.parse(request.body), { kit: "guard", tab: "guard", reveal: false });
});

test("with no Hangar to ask, the refusal stands all the same", async (t) => {
  const { project, env } = setup(t);
  const closed = { ...env, HANGAR_API_URL: "http://127.0.0.1:9", HANGAR_API_TOKEN: "tok" };
  assert.ok((await runHook(bashEvent("git reset --hard", project), closed)).stdout);
  const { HANGAR_PANE_ID: _, HANGAR_PANE_NAME: __, ...bare } = env;
  const answer = await runHook(bashEvent("git reset --hard", project), bare);
  assert.equal(answer.code, 0);
  assert.ok(answer.stdout);
});

// Each tool of Claude Code 2.1.316 that runs a command or touches a file, read from its own
// tool list (the init event of `claude -p`, and a session's tool definitions). MultiEdit is
// not one of them. The README lists those left out, and why.
const WATCHED = ["Artifact", "ArtifactData", "Bash", "DesignSync", "Edit", "Grep", "LSP", "Monitor", "NotebookEdit", "PowerShell", "Read", "Write"];

test("hooks.json: the tools by name, with a time limit, that fails open", () => {
  const { hooks } = JSON.parse(fs.readFileSync(path.join(KIT, "hooks", "hooks.json"), "utf8"));
  assert.deepEqual(Object.keys(hooks), ["PreToolUse"]);
  const [entry] = hooks.PreToolUse;
  assert.deepEqual(entry.matcher.split("|").sort(), WATCHED);
  assert.match(entry.matcher, /^[A-Za-z|]+$/, "explicit names, no pattern");
  assert.equal(entry.hooks[0].type, "command");
  assert.match(entry.hooks[0].command, /^node "\$\{CLAUDE_PLUGIN_ROOT\}\/hooks\/guard\.js"$/);
  assert.ok(entry.hooks[0].timeout > 0 && entry.hooks[0].timeout <= 10);
  assert.equal(entry.hooks[0].onFailure, undefined, "a hook that times out lets the call through");
});

test("the hook answers for every tool hooks.json sends it, and the README names each", async (t) => {
  const { project, dir, env } = setup(t);
  const inputs = {
    Bash: { command: "git reset --hard" },
    PowerShell: { command: "git reset --hard" },
    Monitor: { command: "git reset --hard", description: "x", timeout_ms: 1000 },
    Read: { file_path: path.join(project, ".env") },
    Edit: { file_path: path.join(project, ".env"), old_string: "a", new_string: "b" },
    Write: { file_path: path.join(project, ".env"), content: "x" },
    NotebookEdit: { notebook_path: path.join(project, ".env"), new_source: "x" },
    Grep: { pattern: "x", path: path.join(project, ".env") },
    LSP: { filePath: path.join(project, ".env"), operation: "hover", line: 1, character: 1 },
    Artifact: { file_path: path.join(project, ".env") },
    ArtifactData: { action: "set", file_path: path.join(project, "secrets.json") },
    DesignSync: { method: "write_files", files: [{ path: "a", localPath: ".env" }] },
  };
  assert.deepEqual(Object.keys(inputs).sort(), WATCHED);
  const readme = fs.readFileSync(path.join(KIT, "README.md"), "utf8");
  for (const [tool_name, tool_input] of Object.entries(inputs)) {
    const event = JSON.stringify({ session_id: "s", cwd: project, hook_event_name: "PreToolUse", tool_name, tool_input });
    const { code, stdout } = await runHook(event, env);
    assert.equal(code, 0, tool_name);
    assert.equal(JSON.parse(stdout).hookSpecificOutput.permissionDecision, "deny", tool_name);
    assert.ok(readme.includes(`\`${tool_name}\``), `the README names ${tool_name}`);
  }
  const logged = readLog(dir);
  assert.deepEqual(logged.map((event) => event.tool).sort(), WATCHED, "each written down under its tool");
  assert.equal(logged.find((event) => event.tool === "PowerShell").command, "PowerShell: git reset --hard", "the tab says which tool");
  assert.equal(logged.find((event) => event.tool === "Monitor").command, "Monitor: git reset --hard");
  assert.equal(logged.find((event) => event.tool === "Bash").command, "git reset --hard");
});

test("a tool it does not watch stays out, whatever it holds", async (t) => {
  const { project, dir, env } = setup(t);
  for (const tool_name of ["Glob", "Task", "WebFetch", "mcp__x__y"]) {
    const event = JSON.stringify({ session_id: "s", cwd: project, hook_event_name: "PreToolUse", tool_name, tool_input: { command: "git reset --hard", file_path: ".env", pattern: ".env" } });
    const { code, stdout } = await runHook(event, env);
    assert.equal(code, 0, tool_name);
    assert.equal(stdout, "", tool_name);
  }
  assert.equal(fs.existsSync(dir), false);
});

test("the reason tells the agent to retry with the same tool", async (t) => {
  const { project, env } = setup(t);
  const { stdout } = await runHook(bashEvent("git reset --hard", project), env);
  assert.match(JSON.parse(stdout).hookSpecificOutput.permissionDecisionReason, /same tool/);
});

test("nothing in the hook needs a dependency", () => {
  for (const name of fs.readdirSync(path.join(KIT, "hooks"))) {
    if (!name.endsWith(".js")) continue;
    const text = fs.readFileSync(path.join(KIT, "hooks", name), "utf8");
    for (const [, module] of text.matchAll(/require\("([^"]+)"\)/g)) {
      assert.match(module, /^(node:|\.\/)/, `${name} requires ${module}`);
    }
  }
});
