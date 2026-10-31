"use strict";
// The manifest and the kit's shape, checked by hand against docs/kits.md: Hangar's
// validator is Rust (src-tauri/src/kits/manifest.rs) and cannot be reached from a Node
// test. Each rule below is one of that page's.
const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { KIT } = require("./helpers.js");

const manifest = JSON.parse(fs.readFileSync(path.join(KIT, "hangar-kit.json"), "utf8"));
const read = (...parts) => fs.readFileSync(path.join(KIT, ...parts), "utf8");

function allFiles(dir = KIT, found = []) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (entry.isDirectory()) allFiles(path.join(dir, entry.name), found);
    else found.push(path.join(dir, entry.name));
  }
  return found;
}

function safePath(relative, extensions, at) {
  assert.equal(typeof relative, "string", at);
  assert.doesNotMatch(relative, /^([/\\]|[A-Za-z]:)|:|(^|[/\\])\.\.([/\\]|$)/, `${at}: must stay inside the kit`);
  assert.ok(extensions.some((ext) => relative.toLowerCase().endsWith(ext)), `${at}: ${extensions.join("/")}`);
  assert.ok(fs.existsSync(path.join(KIT, relative)), `${at}: ${relative} must exist`);
}

test("the manifest's own fields", () => {
  assert.equal(manifest.schema, 1);
  assert.equal(manifest.id, "guard");
  assert.match(manifest.id, /^[a-z0-9-]{2,40}$/);
  assert.ok(manifest.name.trim());
  assert.match(manifest.version, /^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/);
  assert.ok(manifest.description.trim());
  assert.equal(manifest.homepage, "https://github.com/simnJS/hangar-ai/tree/main/kits/guard");
  safePath(manifest.icon, [".svg", ".png"], "icon");
  assert.equal(manifest.commands, undefined, "it runs no declared command");
  assert.equal(manifest.setup, undefined, "and asks for no setup");
});

test("the plugin is found at the root, named like the kit", () => {
  assert.equal(manifest.plugin, undefined);
  const plugin = JSON.parse(read(".claude-plugin", "plugin.json"));
  assert.equal(plugin.name, manifest.id);
  assert.equal(plugin.version, manifest.version);
  assert.match(read("skills", "guard", "SKILL.md").replace(/\r\n/g, "\n"), /^---\nname: guard\n/);
});

test("the tab: « Garde-fous », and the two permissions it needs", () => {
  assert.equal(manifest.tabs.length, 1);
  const [tab] = manifest.tabs;
  assert.equal(tab.id, "guard");
  assert.equal(tab.title, "Garde-fous");
  safePath(tab.entry, [".html"], "tabs[0].entry");
  safePath(tab.icon, [".svg", ".png"], "tabs[0].icon");
  assert.deepEqual(tab.permissions, ["data.read", "data.write"]);
  assert.equal(tab.params, undefined);
});

test("the hook asks for the kit and the tab the manifest declares", () => {
  const { KIT: id, TAB } = require("../hooks/open-tab.js");
  assert.equal(id, manifest.id);
  assert.equal(TAB, manifest.tabs[0].id);
});

test("the tab's page loads only the kit's own files, and reaches for nothing else", () => {
  const dir = path.join(KIT, "tabs", "guard");
  const html = fs.readFileSync(path.join(dir, "index.html"), "utf8");
  const sources = [...html.matchAll(/(?:src|href)="([^"]+)"/g)].map((match) => match[1]);
  const fromHangar = sources.filter((source) => source.startsWith("/_hangar/"));
  assert.ok(fromHangar.includes("/_hangar/kit.js"), "the SDK");
  for (const source of sources.filter((item) => !item.startsWith("/_hangar/"))) {
    assert.doesNotMatch(source, /^([a-z]+:)?\/\//i, `${source} leaves the kit`);
    assert.doesNotMatch(source, /^\//, `${source} must be relative`);
    assert.ok(fs.existsSync(path.join(dir, source)), source);
  }
  for (const name of fs.readdirSync(dir)) {
    const text = fs.readFileSync(path.join(dir, name), "utf8");
    assert.doesNotMatch(text, /\bfetch\(|XMLHttpRequest|WebSocket|localStorage|sessionStorage|\beval\(|innerHTML|@import|url\(\s*["']?https?:/, `${name} reaches outside what a tab may`);
  }
});

test("rules.json reads: each rule says what it does, and can be turned off", () => {
  const rules = JSON.parse(read("rules.json"));
  assert.deepEqual(Object.keys(rules).sort(), ["downloadToShell", "dropDatabase", "forcePush", "gitClean", "gitResetHard", "guardFiles", "recursiveDelete", "secrets"]);
  for (const [name, rule] of Object.entries(rules)) {
    assert.equal(rule.enabled, true, name);
    assert.ok(rule.what.trim(), name);
  }
  new RegExp(rules.dropDatabase.sql, "i");
});

test("the README says what it does, what it touches, what it does not protect against, and where to get it", () => {
  const readme = read("README.md");
  assert.match(readme, /## What it does\b/);
  assert.match(readme, /## What it can touch\b/);
  assert.match(readme, /## What it does not protect against\b/);
  assert.match(readme, /It reads the command line/);
  assert.match(readme, /It is no sandbox/);
  assert.match(readme, /A script can do anything once started/);
  assert.ok(readme.includes("https://github.com/simnJS/hangar-ai/tree/main/kits/guard"));
});

test("the skill tells an agent to explain, not to go around", () => {
  const skill = read("skills", "guard", "SKILL.md");
  assert.match(skill, /Stop trying/);
  assert.match(skill, /why you need it/);
  assert.match(skill, /character for character/);
  assert.ok(skill.split(/\s+/).length < 300, "a short one");
});

test("the permissions, the route and the methods it uses are in the docs, when the docs are around", () => {
  const docs = path.join(KIT, "..", "..", "docs", "kits.md");
  if (!fs.existsSync(docs)) return; // a kit installed on its own has no docs beside it
  const text = fs.readFileSync(docs, "utf8");
  for (const name of ["data.read", "data.write", "tab.setBadge", "tab.setTitle"]) assert.ok(text.includes(`\`${name}\``), name);
  assert.ok(text.includes("/api/kit/tab/open"));
  assert.ok(text.includes("HANGAR_KITS_DATA"));
});

test("nothing personal, and the files in English, anywhere", () => {
  const personal = new RegExp(["sim" + "njs@", "pro@" + "sim", "C:\\\\Users\\\\sim" + "nJS", "/home/sim" + "n", "Iabench".toLowerCase() + "-worktrees"].join("|"), "i");
  for (const file of allFiles()) {
    const text = fs.readFileSync(file, "utf8");
    assert.doesNotMatch(text, personal, file);
  }
});
