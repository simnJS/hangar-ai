"use strict";
// The manifest, checked by hand against docs/kits.md: Hangar's validator is Rust
// (src-tauri/src/kits/manifest.rs) and cannot be reached from a Node test. And the
// promise of the kit: gh only, read-only, nothing that writes to GitHub.
const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { KIT } = require("./helpers.js");

const manifest = JSON.parse(fs.readFileSync(path.join(KIT, "hangar-kit.json"), "utf8"));
const exists = (relative) => fs.existsSync(path.join(KIT, relative));
const PERMISSIONS = ["workspace.read", "panes.read", "panes.send", "files.read", "files.write", "data.read", "data.write", "preview.show", "notify"];
const SEMVER = /^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$/;

test("the manifest's own fields", () => {
  assert.equal(manifest.schema, 1);
  assert.equal(manifest.id, "github");
  assert.match(manifest.version, SEMVER);
  assert.equal(manifest.hangar, ">=1.9.0");
  assert.equal(manifest.homepage, "https://github.com/simnJS/hangar-ai/tree/main/kits/github");
  assert.ok(exists(manifest.icon));
  assert.ok(manifest.gallery.permissionsSummary.trim());
  assert.ok(Array.isArray(manifest.gallery.tags) && manifest.gallery.tags.length);
  assert.match(manifest.gallery.permissionsSummary, /Nothing is written to GitHub/);
});

test("two tabs, each with the permissions it uses and no more", () => {
  assert.deepEqual(manifest.tabs.map((tab) => tab.title), ["Pull requests", "Issues"]);
  for (const tab of manifest.tabs) {
    assert.match(tab.id, /^[a-z0-9-]{1,40}$/);
    assert.ok(exists(tab.entry) && tab.entry.endsWith(".html"));
    assert.ok(exists(tab.icon));
    assert.equal(new Set(tab.permissions).size, tab.permissions.length);
    for (const permission of tab.permissions) {
      if (permission.startsWith("command:")) assert.ok(manifest.commands[permission.slice(8)], `${permission} is not declared`);
      else assert.ok(PERMISSIONS.includes(permission), `unknown ${permission}`);
    }
    for (const needed of ["panes.read", "panes.send", "data.read", "data.write"]) assert.ok(tab.permissions.includes(needed), needed);
    for (const unwanted of ["notify", "files.read", "files.write", "workspace.read", "preview.show"]) assert.ok(!tab.permissions.includes(unwanted), `${unwanted} is not needed`);
  }
  const [pulls, issues] = manifest.tabs.map((tab) => tab.permissions.filter((p) => p.startsWith("command:")).sort());
  assert.deepEqual(pulls, ["command:pr-list", "command:pr-threads", "command:pr-view", "command:repo-info"]);
  assert.deepEqual(issues, ["command:issue-list", "command:issue-view", "command:repo-info"]);
  const used = new Set([...pulls, ...issues].map((p) => p.slice(8)));
  assert.deepEqual([...used].sort(), Object.keys(manifest.commands).sort(), "no declared command left unused");
});

test("every command is a gh argv, read-only, and its placeholders are declared with a rule", () => {
  assert.deepEqual(Object.keys(manifest.commands).sort(), ["issue-list", "issue-view", "pr-list", "pr-threads", "pr-view", "repo-info"]);
  const READ = new Set(["view", "list"]);
  for (const [name, command] of Object.entries(manifest.commands)) {
    assert.match(name, /^[a-z][a-z0-9-]*$/);
    assert.ok(command.description.trim());
    assert.ok(Array.isArray(command.run), "an argv, never a command line");
    assert.equal(command.run[0], "gh");
    assert.ok(command.timeoutSeconds >= 1 && command.timeoutSeconds <= 120);
    const [, group, verb] = command.run;
    if (group === "api") {
      // the one place gh can write: it is held to a fixed GraphQL query, a GET-like read
      assert.equal(verb, "graphql");
      const query = command.run.find((item) => item.startsWith("query="));
      assert.ok(query, "the query is fixed in the manifest");
      assert.match(query, /^query=query\(/, "a query");
      assert.doesNotMatch(query, /mutation|subscription/i);
      assert.ok(!command.run.some((item) => /^(--method|-X|--input|--field|--raw-field)$/.test(item)), "no other way to change the request");
      assert.ok(!command.run.includes("graphql") || command.run.indexOf("graphql") === 2);
    } else {
      assert.ok(["pr", "issue", "repo"].includes(group), group);
      assert.ok(READ.has(verb), `${group} ${verb} is not a read`);
    }
    assert.ok(!command.run.some((item) => /^(merge|close|comment|edit|create|review|reopen|ready|delete|lock|transfer|develop|checkout)$/.test(item)));
    const used = new Set();
    for (const item of command.run.slice(1)) for (const match of item.matchAll(/\{([A-Za-z_][A-Za-z0-9_-]*)\}/g)) used.add(match[1]);
    assert.deepEqual([...used].sort(), Object.keys(command.args || {}).sort(), `${name}: placeholders and args match`);
    for (const [arg, rule] of Object.entries(command.args || {})) {
      assert.ok(rule.pattern, `${name}.${arg} needs a pattern`);
      new RegExp(`^(?:${rule.pattern})$`);
    }
  }
});

test("each id is digits and nothing else; owner and name cannot start an option", () => {
  for (const command of Object.values(manifest.commands)) {
    if (!command.args) continue;
    if (command.args.id) {
      const rule = new RegExp(command.args.id.pattern);
      assert.equal(command.args.id.pattern, "^[0-9]+$");
      for (const bad of ["", "12a", "-1", "1 2", "1\n2", "../1", "$(x)", "12;ls"]) assert.ok(!rule.test(bad), JSON.stringify(bad));
      assert.ok(rule.test("42"));
    }
    for (const arg of ["owner", "name"]) {
      if (!command.args[arg]) continue;
      const rule = new RegExp(`^(?:${command.args[arg].pattern})$`);
      assert.ok(rule.test("example") && rule.test(arg === "name" ? "my-repo.js" : "my-org"));
      for (const bad of ["", "-x", "--repo", "a b", "a/b", "a;b", "$(x)", "a\nb"]) assert.ok(!rule.test(bad), JSON.stringify(bad));
    }
  }
});

test("the fixed query reads a pull request's review threads and nothing else", () => {
  const query = manifest.commands["pr-threads"].run.find((item) => item.startsWith("query="));
  assert.match(query, /reviewThreads/);
  assert.match(query, /isResolved/);
  assert.doesNotMatch(query, /\{[A-Za-z_][A-Za-z0-9_-]*\}/, "a brace group that Hangar would read as a placeholder");
  const run = manifest.commands["pr-threads"].run;
  assert.equal(run.filter((item) => item === "-F").length, 1, "only the number is typed (an Int)");
  assert.ok(run.includes("owner={owner}") && run.includes("name={name}") && run.includes("number={id}"));
});

test("the kit holds no hook, no setup step and no MCP server", () => {
  for (const key of ["hooks", "setup", "mcpServers", "plugin"]) assert.equal(manifest[key], undefined, key);
  assert.equal(exists("hooks"), false);
});
