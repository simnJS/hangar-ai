"use strict";
// The manifest, checked by hand against docs/kits.md: Hangar's validator is Rust
// (src-tauri/src/kits/manifest.rs) and cannot be reached from a Node test. A value's
// rule is written in what Rust and JavaScript read alike, as Hangar's window reads it.
const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { KIT, TAB } = require("./helpers.js");

const manifest = JSON.parse(fs.readFileSync(path.join(KIT, "hangar-kit.json"), "utf8"));
const exists = (relative) => fs.existsSync(path.join(KIT, relative));
const whole = (pattern) => new RegExp(`^(?:${pattern})$`, "u");
const PERMISSIONS = ["workspace.read", "panes.read", "panes.send", "files.read", "files.write", "data.read", "data.write", "preview.show", "notify"];

test("the manifest's own fields", () => {
  assert.equal(manifest.schema, 1);
  assert.equal(manifest.id, "checklist");
  assert.match(manifest.id, /^[a-z0-9-]{2,40}$/);
  assert.equal(manifest.name, "Recette");
  assert.match(manifest.version, /^\d+\.\d+\.\d+$/);
  assert.ok(manifest.description.trim());
  assert.equal(manifest.hangar, ">=1.9.0");
  assert.equal(manifest.homepage, "https://github.com/simnJS/hangar-ai/tree/main/kits/checklist");
  assert.ok(exists(manifest.icon) && manifest.icon.endsWith(".svg"));
  assert.ok(manifest.gallery.permissionsSummary.trim());
  assert.ok(manifest.gallery.tags.length && manifest.gallery.tags.every((tag) => typeof tag === "string" && tag.trim()));
});

test("the plugin is named like the kit, and holds a skill and nothing that runs by itself", () => {
  assert.equal(manifest.plugin, undefined);
  const plugin = JSON.parse(fs.readFileSync(path.join(KIT, ".claude-plugin", "plugin.json"), "utf8"));
  assert.equal(plugin.name, manifest.id);
  assert.equal(plugin.version, manifest.version);
  for (const part of ["hooks", ".mcp.json", ".lsp.json", "monitors", "bin", "settings.json"]) assert.ok(!exists(part), part);
  for (const key of ["hooks", "mcpServers", "lspServers", "monitors", "settings", "binaries", "dependencies"]) assert.equal(plugin[key], undefined, key);
  assert.equal(manifest.commands, undefined, "no command: the kit runs nothing");
  assert.equal(manifest.setup, undefined);
});

test("the tab: five permissions and nothing more, one tab per list", () => {
  assert.equal(manifest.tabs.length, 1);
  const tab = manifest.tabs[0];
  assert.match(tab.id, /^[a-z0-9-]{1,40}$/);
  assert.equal(tab.title, "Recette");
  assert.ok(exists(tab.entry) && tab.entry.endsWith(".html") && exists(tab.icon));
  assert.notEqual(tab.singleton, true, "one tab per path");
  assert.deepEqual([...tab.permissions].sort(), ["data.read", "data.write", "files.read", "notify", "panes.send"]);
  assert.ok(tab.permissions.every((permission) => PERMISSIONS.includes(permission)));
  assert.deepEqual(Object.keys(tab.params), ["path"]);
  const param = tab.params.path;
  assert.ok(param.description.trim());
  const rule = whole(param.pattern);
  assert.ok(rule.test("recette/login.json") && rule.test("a b/c.json"));
  assert.ok(!rule.test("") && !rule.test("C:/x.json") && !rule.test("a\nb"));
  assert.doesNotMatch(param.pattern, /\(\?[<!=]/, "Rust's regex has no look-around");
});

test("the page loads only the kit's own files and Hangar UI's three addresses", () => {
  const html = fs.readFileSync(path.join(TAB, "index.html"), "utf8");
  const sources = [...html.matchAll(/(?:src|href)="([^"]+)"/g)].map((match) => match[1]);
  const hangar = ["/_hangar/kit.css", "/_hangar/kit.js", "/_hangar/kit-ui.js"];
  for (const address of hangar) assert.ok(sources.includes(address), address);
  for (const source of sources.filter((source) => !hangar.includes(source))) {
    assert.doesNotMatch(source, /^([a-z]+:)?\/\//i, `${source} leaves the kit`);
    assert.doesNotMatch(source, /^\//, `${source} must be relative`);
    assert.ok(fs.existsSync(path.join(TAB, source)), source);
  }
  assert.ok(sources.indexOf("/_hangar/kit.js") < sources.indexOf("/_hangar/kit-ui.js"), "kit-ui.js after kit.js");
  for (const name of fs.readdirSync(TAB)) {
    const text = fs.readFileSync(path.join(TAB, name), "utf8");
    assert.doesNotMatch(text, /\bfetch\(|XMLHttpRequest|WebSocket|localStorage|sessionStorage|\beval\(|innerHTML|@import|url\(\s*["']?https?:/, `${name} reaches outside what a tab may`);
  }
});

test("the stylesheet writes no colour of its own", () => {
  const css = fs.readFileSync(path.join(TAB, "style.css"), "utf8");
  assert.doesNotMatch(css, /#[0-9a-fA-F]{3,8}\b|\brgba?\(|\bhsla?\(|(?<![-\w])(?:white|black|red|green|blue|gray|grey)(?![-\w])/i);
});

test("the skill is there, named like the kit, and asks for concrete steps", () => {
  const skill = fs.readFileSync(path.join(KIT, "skills", "checklist", "SKILL.md"), "utf8");
  assert.match(skill, /^---\nname: checklist\ndescription: .+\n---\n/);
  assert.match(skill, /kit_tab_open/);
  assert.match(skill, /"kit": "checklist"/);
  assert.match(skill, /\*\*Stop\.\*\*/);
  assert.match(skill, /check it works/);
});

test("nothing personal, nothing attributed to an assistant", () => {
  const files = [];
  const walk = (dir) => fs.readdirSync(dir, { withFileTypes: true }).forEach((entry) => (entry.isDirectory() ? walk(path.join(dir, entry.name)) : files.push(path.join(dir, entry.name))));
  walk(KIT);
  for (const file of files.filter((file) => path.basename(file) !== "manifest.test.js")) {
    const text = fs.readFileSync(file, "utf8");
    assert.doesNotMatch(text, /C:\\Users|\/Users\/|@[a-z0-9-]+\.(fr|com)\b|Co-Authored-By/i, path.relative(KIT, file));
  }
});

test("the permissions and methods used are in the docs, when the docs are around", () => {
  const docs = path.join(KIT, "..", "..", "docs", "kits.md");
  if (!fs.existsSync(docs)) return; // a kit installed on its own has no docs beside it
  const text = fs.readFileSync(docs, "utf8");
  for (const permission of manifest.tabs[0].permissions) assert.ok(text.includes(`\`${permission}\``), permission);
  for (const method of ["files.read", "data.read", "data.write", "panes.send", "tab.params", "tab.setTitle", "tab.setBadge", "notify"]) {
    assert.ok(text.includes(`\`${method}\``), method);
  }
});
