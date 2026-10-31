"use strict";
const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { KIT } = require("./helpers");
const logic = require("../tabs/deps/logic");

const manifest = JSON.parse(fs.readFileSync(path.join(KIT, "hangar-kit.json"), "utf8"));
const read = (...parts) => fs.readFileSync(path.join(KIT, ...parts), "utf8");

function allFiles(dir = KIT, found = []) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (entry.isDirectory()) allFiles(path.join(dir, entry.name), found);
    else found.push(path.join(dir, entry.name));
  }
  return found;
}

test("the manifest's own fields", () => {
  assert.equal(manifest.schema, 1);
  assert.equal(manifest.id, "deps");
  assert.match(manifest.version, /^\d+\.\d+\.\d+$/);
  assert.ok(manifest.name.trim() && manifest.description.trim());
  assert.equal(manifest.homepage, "https://github.com/simnJS/hangar-ai/tree/main/kits/deps");
  assert.equal(manifest.hangar, ">=1.9.0");
  assert.ok(fs.existsSync(path.join(KIT, manifest.icon)));
  assert.equal(manifest.setup, undefined);
});

test("the gallery's card: one sentence, and its tags", () => {
  const { permissionsSummary, tags } = manifest.gallery;
  assert.ok(permissionsSummary.trim());
  assert.match(permissionsSummary, /command/);
  assert.ok(Array.isArray(tags) && tags.length > 0 && tags.every((tag) => typeof tag === "string" && tag.trim()));
});

test("the plugin is found at the root, named like the kit, with its skill", () => {
  assert.equal(manifest.plugin, undefined);
  const plugin = JSON.parse(read(".claude-plugin", "plugin.json"));
  assert.equal(plugin.name, manifest.id);
  assert.equal(plugin.version, manifest.version);
  assert.match(read("skills", "deps", "SKILL.md").replace(/\r\n/g, "\n"), /^---\nname: deps\ndescription: .+\n---\n/);
});

test("one declared command: node on the kit's own script, no placeholder but {kitDir}", () => {
  assert.deepEqual(Object.keys(manifest.commands), ["audit"]);
  const audit = manifest.commands.audit;
  assert.deepEqual(audit.run, ["node", "{kitDir}/tools/audit.js"]);
  assert.equal(audit.args, undefined);
  assert.ok(audit.description.trim());
  assert.ok(Number.isInteger(audit.timeoutSeconds) && audit.timeoutSeconds >= 1 && audit.timeoutSeconds <= 120);
  assert.ok(fs.existsSync(path.join(KIT, "tools", "audit.js")));
});

test("the tab: « Dépendances », and exactly the five permissions it needs", () => {
  assert.equal(manifest.tabs.length, 1);
  const [tab] = manifest.tabs;
  assert.match(tab.id, /^[a-z0-9-]{1,40}$/);
  assert.equal(tab.title, "Dépendances");
  assert.deepEqual([...tab.permissions].sort(), ["command:audit", "data.read", "data.write", "panes.read", "panes.send"]);
  assert.ok(fs.existsSync(path.join(KIT, tab.entry)));
  assert.ok(fs.existsSync(path.join(KIT, tab.icon)));
});

test("the page loads Hangar UI and its own scripts, and nothing from elsewhere", () => {
  const html = read("tabs", "deps", "index.html");
  assert.match(html, /href="\/_hangar\/kit\.css"/);
  assert.match(html, /src="\/_hangar\/kit\.js"/);
  assert.match(html, /src="\/_hangar\/kit-ui\.js"/);
  for (const src of html.match(/(?:src|href)="[^"]+"/g)) assert.match(src, /="(\/_hangar\/|[a-z-]+\.js")/, src);
  assert.doesNotMatch(html, /https?:\/\//);
  assert.doesNotMatch(html, /<style/);
});

test("no colour of its own in the tab, and no markup built from data", () => {
  for (const name of ["logic.js", "controller.js", "view.js", "app.js", "index.html"]) {
    const source = read("tabs", "deps", name);
    assert.doesNotMatch(source, /#[0-9a-fA-F]{3,8}\b|rgba?\(|hsla?\(/, name);
    assert.doesNotMatch(source, /innerHTML|insertAdjacentHTML|document\.write|eval\(/, name);
    assert.doesNotMatch(source, /fetch\(|XMLHttpRequest|WebSocket/, `${name} has no network of its own`);
  }
});

test("English and French hold the same keys, and French has its accents", () => {
  const { en, fr } = logic.STRINGS;
  const keys = (table) => new Set(Object.keys(table).map((key) => key.replace(/_(one|other)$/, "")));
  assert.deepEqual([...keys(fr)].sort(), [...keys(en)].sort());
  assert.equal(fr.tab, "Dépendances");
  for (const key of Object.keys(en)) {
    const placeholders = (table) => (table[key].match(/\{\w+\}/g) || []).sort().join();
    assert.equal(placeholders(fr), placeholders(en), key);
  }
  assert.match(fr.deniedText, /Réglages › Kits/);
  assert.match(fr.handOver, /^Confier à un agent$/);
  assert.doesNotMatch(Object.values(fr).join(" "), /\b(Reglages|echec|Dependances|a un agent)\b/);
});

test("nothing personal anywhere in the kit", () => {
  const forbidden = /C:\\Users|\/Users\/|\/home\/\w|@[\w-]+\.(com|fr)|simnjs\.fr|[Cc]o-[Aa]uthored/;
  for (const file of allFiles()) {
    if (file.includes(`${path.sep}fixtures${path.sep}`)) continue;
    assert.doesNotMatch(fs.readFileSync(file, "utf8"), forbidden, path.relative(KIT, file));
  }
});

test("the README says what it does and what it runs", () => {
  const readme = read("README.md");
  assert.match(readme, /## What it does/);
  assert.match(readme, /## What it runs/);
  for (const command of ["pnpm audit --json", "npm outdated --json", "cargo audit --json", "pip-audit", "pip list --outdated"]) assert.ok(readme.includes(command), command);
});
