"use strict";
// The manifest, checked by hand against docs/kits.md: Hangar's validator is Rust
// (src-tauri/src/kits/manifest.rs) and cannot be reached from a Node test. Each
// rule below is one of that page's, so a manifest that passes here is one it reads.
const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { KIT } = require("./helpers.js");

const manifest = JSON.parse(fs.readFileSync(path.join(KIT, "hangar-kit.json"), "utf8"));
const exists = (relative) => fs.existsSync(path.join(KIT, relative));
const PERMISSIONS = ["workspace.read", "panes.read", "panes.send", "files.read", "data.read", "data.write", "preview.show", "notify"];
const SEMVER = /^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$/;

function safePath(relative, extensions, at) {
  assert.equal(typeof relative, "string", at);
  assert.doesNotMatch(relative, /^([/\\]|[A-Za-z]:)|:|(^|[/\\])\.\.([/\\]|$)/, `${at}: must stay inside the kit`);
  assert.ok(extensions.some((ext) => relative.toLowerCase().endsWith(ext)), `${at}: ${extensions.join("/")}`);
  assert.ok(exists(relative), `${at}: ${relative} must exist`);
}

test("the manifest's own fields", () => {
  assert.equal(manifest.schema, 1);
  assert.equal(manifest.id, "ci-status");
  assert.match(manifest.id, /^[a-z0-9-]{2,40}$/);
  assert.ok(manifest.name.trim());
  assert.match(manifest.version, SEMVER);
  assert.equal(typeof manifest.description, "string");
  assert.equal(manifest.hangar, ">=1.9.0");
  assert.match(manifest.homepage, /^https?:\/\//);
  safePath(manifest.icon, [".svg", ".png"], "icon");
});

test("the plugin is found at the root", () => {
  assert.equal(manifest.plugin, undefined, "default: the root, since .claude-plugin exists");
  const plugin = JSON.parse(fs.readFileSync(path.join(KIT, ".claude-plugin", "plugin.json"), "utf8"));
  assert.equal(plugin.name, manifest.id, "the plugin is named like the kit: it prefixes its skills");
  const skill = fs.readFileSync(path.join(KIT, "skills", "ci-status", "SKILL.md"), "utf8");
  assert.match(skill, /^---\nname: ci-status\n/);
});

test("the tab", () => {
  assert.equal(manifest.tabs.length, 1);
  const tab = manifest.tabs[0];
  assert.match(tab.id, /^[a-z0-9-]{1,40}$/);
  assert.equal(tab.title, "CI");
  safePath(tab.entry, [".html"], "tabs[0].entry");
  safePath(tab.icon, [".svg", ".png"], "tabs[0].icon");
  assert.notEqual(tab.singleton, true, "one tab per run");
  assert.deepEqual(Object.keys(tab.params), ["id"]);
  const { id } = tab.params;
  assert.ok(id.description.trim());
  const whole = new RegExp(`^(?:${id.pattern})$`);
  assert.ok(whole.test("41222364482") && !whole.test("") && !whole.test("12a") && !whole.test("-1") && !whole.test("1\n2"));
});

test("permissions are known, and every command one names is declared", () => {
  const tab = manifest.tabs[0];
  const named = new Set();
  for (const permission of tab.permissions) {
    if (permission.startsWith("command:")) {
      const name = permission.slice("command:".length);
      assert.ok(manifest.commands[name], `${permission} names a command the kit does not declare`);
      named.add(name);
    } else {
      assert.ok(PERMISSIONS.includes(permission), `unknown permission ${permission}`);
    }
  }
  assert.deepEqual([...named].sort(), Object.keys(manifest.commands).sort(), "no declared command left unused");
  assert.ok(tab.permissions.includes("notify"));
  assert.equal(new Set(tab.permissions).size, tab.permissions.length);
});

test("the commands: an argv through gh, placeholders declared with a rule", () => {
  const names = Object.keys(manifest.commands).sort();
  assert.deepEqual(names, ["ci-list", "ci-log", "ci-run"], "nothing else");
  for (const [name, command] of Object.entries(manifest.commands)) {
    assert.match(name, /^[a-z][a-z0-9-]*$/);
    assert.ok(command.description.trim());
    assert.ok(Array.isArray(command.run), `${name}: an argv, never a command line`);
    assert.ok(command.run.every((item) => typeof item === "string"));
    const [program, ...rest] = command.run;
    assert.equal(program, "gh", `${name}: gh only`);
    assert.doesNotMatch(program, /[{}/\\:]|[. ]$/);
    assert.equal(rest[0], "run");
    assert.ok(["view", "list"].includes(rest[1]));

    const used = new Set();
    for (const item of rest) for (const [, placeholder] of item.matchAll(/\{([A-Za-z_][A-Za-z0-9_-]*)\}/g)) used.add(placeholder);
    assert.deepEqual([...used].sort(), Object.keys(command.args || {}).sort(), `${name}: placeholders and args agree`);
    for (const [arg, rule] of Object.entries(command.args || {})) {
      assert.equal(typeof rule.pattern, "string", `${name}.${arg} needs a pattern`);
      new RegExp(rule.pattern);
      const whole = new RegExp(`^(?:${rule.pattern})$`);
      assert.ok(whole.test("123") && !whole.test("12; rm") && !whole.test("--x") && !whole.test(""), `${name}.${arg}`);
      assert.doesNotMatch(rule.pattern, /\(\?[<!=]/, "Rust's regex has no look-around");
    }
    if (command.timeoutSeconds !== undefined) {
      assert.ok(Number.isInteger(command.timeoutSeconds) && command.timeoutSeconds >= 1 && command.timeoutSeconds <= 120);
    }
    // Options are the kit's to write: none may be a placeholder.
    assert.ok(!rest.some((item) => /^-.*\{/.test(item) || item.startsWith("{") && item.slice(1).startsWith("-")));
  }
  assert.deepEqual(manifest.commands["ci-run"].run, [
    "gh", "run", "view", "{id}", "--json", "status,conclusion,displayTitle,headBranch,workflowName,url,jobs,startedAt,updatedAt",
  ]);
  assert.deepEqual(manifest.commands["ci-log"].run, ["gh", "run", "view", "{id}", "--log-failed"]);
  assert.deepEqual(manifest.commands["ci-list"].run.slice(0, 5), ["gh", "run", "list", "--limit", "15"]);
});

test("the tab's page loads the kit's own files, and Hangar's library", () => {
  const dir = path.join(KIT, "tabs", "ci");
  const html = fs.readFileSync(path.join(dir, "index.html"), "utf8");
  const sources = [...html.matchAll(/(?:src|href)="([^"]+)"/g)].map((match) => match[1]);
  const library = ["/_hangar/kit.css", "/_hangar/kit.js", "/_hangar/kit-ui.js"];
  assert.deepEqual(sources.filter((source) => source.startsWith("/")), library, "the sheet, then the SDK, then the behaviours");
  assert.ok(sources.indexOf("/_hangar/kit-ui.js") < sources.indexOf("app.js"));
  for (const source of sources.filter((source) => !library.includes(source))) {
    assert.doesNotMatch(source, /^([a-z]+:)?\/\//i, `${source} leaves the kit`);
    assert.doesNotMatch(source, /^\//, `${source} must be relative`);
    assert.ok(fs.existsSync(path.join(dir, source)), source);
  }
  for (const name of fs.readdirSync(dir)) {
    const text = fs.readFileSync(path.join(dir, name), "utf8");
    assert.doesNotMatch(text, /\bfetch\(|XMLHttpRequest|WebSocket|localStorage|sessionStorage|\beval\(|innerHTML|@import|url\(\s*["']?https?:/, `${name} reaches outside what a tab may`);
  }
});

test("the tab's sheet has no colour and no measure of its own", () => {
  const sheet = fs.readFileSync(path.join(KIT, "tabs", "ci", "style.css"), "utf8").replace(/\/\*[\s\S]*?\*\//g, "");
  assert.doesNotMatch(sheet, /#[0-9a-f]{3,8}\b|\brgba?\(|\bhsla?\(|\bcolor-mix\(|\b(?:Canvas|CanvasText)\b|prefers-color-scheme/i, "Hangar's theme decides");
  assert.doesNotMatch(sheet, /\b\d+(?:\.\d+)?(?:px|rem|em)\b/, "the library's measures follow the interface size");
  for (const [, name] of sheet.matchAll(/^\.(ci-[a-z-]+)/gm)) assert.match(name, /^ci-/);
});

test("the classes the tab names are the library's, when the library is around", () => {
  const sheet = path.join(KIT, "..", "..", "src-tauri", "kit-sdk", "kit.css");
  if (!fs.existsSync(sheet)) return; // a kit installed on its own has no library beside it
  const known = fs.readFileSync(sheet, "utf8");
  const dir = path.join(KIT, "tabs", "ci");
  for (const name of ["index.html", "view.js", "style.css"]) {
    const text = fs.readFileSync(path.join(dir, name), "utf8");
    // A name being put together (`kit-icon--${icon}`) is left to the captures.
    for (const [, used] of text.matchAll(/(?<![-\w/])(kit-[a-z0-9_-]*[a-z0-9])(?![a-z0-9_-]|\$\{)/g)) {
      if (used === "kit-ui") continue;
      assert.ok(known.includes(`.${used}`), `${name}: ${used} is not in kit.css`);
    }
    for (const [, token] of text.matchAll(/var\((--kit-[a-z0-9-]+)/g)) {
      assert.ok(known.includes(`${token}:`), `${name}: ${token} is not in kit.css`);
    }
  }
});

test("the permissions used are in the docs, when the docs are around", () => {
  const docs = path.join(KIT, "..", "..", "docs", "kits.md");
  if (!fs.existsSync(docs)) return; // a kit installed on its own has no docs beside it
  const text = fs.readFileSync(docs, "utf8");
  for (const permission of manifest.tabs[0].permissions.filter((p) => !p.startsWith("command:"))) {
    assert.ok(text.includes(`\`${permission}\``), permission);
  }
  for (const method of ["command.run", "tab.setTitle", "tab.setBadge", "tab.params", "notify"]) {
    assert.ok(text.includes(`\`${method}\``), method);
  }
  assert.ok(text.includes("/api/kit/tab/open"), "the hook's route");
});
