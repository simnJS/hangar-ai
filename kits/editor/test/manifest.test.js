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

function safePath(relative, extensions, at) {
  assert.equal(typeof relative, "string", at);
  assert.doesNotMatch(relative, /^([/\\]|[A-Za-z]:)|:|(^|[/\\])\.\.([/\\]|$)/, `${at}: must stay inside the kit`);
  assert.ok(extensions.some((ext) => relative.toLowerCase().endsWith(ext)), `${at}: ${extensions.join("/")}`);
  assert.ok(exists(relative), `${at}: ${relative} must exist`);
}

test("the manifest's own fields", () => {
  assert.equal(manifest.schema, 1);
  assert.equal(manifest.id, "editor");
  assert.equal(manifest.name, "Éditeur");
  assert.match(manifest.version, /^\d+\.\d+\.\d+$/);
  assert.ok(manifest.description.trim());
  assert.equal(manifest.hangar, ">=1.9.0");
  assert.equal(manifest.homepage, "https://github.com/simnJS/hangar-ai/tree/main/kits/editor");
  safePath(manifest.icon, [".svg"], "icon");
});

test("it carries its card for the library, and index.json is left to the release", () => {
  assert.ok(manifest.gallery.permissionsSummary.trim());
  assert.ok(manifest.gallery.tags.length && manifest.gallery.tags.every((tag) => typeof tag === "string" && tag.trim()));
});

test("the plugin is named like the kit, and holds a skill and nothing that runs by itself", () => {
  assert.equal(manifest.plugin, undefined, "default: the root, since .claude-plugin exists");
  const plugin = JSON.parse(fs.readFileSync(path.join(KIT, ".claude-plugin", "plugin.json"), "utf8"));
  assert.equal(plugin.name, manifest.id);
  assert.equal(plugin.version, manifest.version);
  for (const part of ["hooks", ".mcp.json", ".lsp.json", "monitors", "bin", "settings.json"]) assert.ok(!exists(part), part);
  for (const key of ["hooks", "mcpServers", "lspServers", "monitors", "settings", "binaries", "dependencies"]) assert.equal(plugin[key], undefined, key);
  assert.equal(manifest.commands, undefined, "no command either");
  assert.equal(manifest.setup, undefined);
});

test("the tab: three permissions and nothing more, one tab per set of params", () => {
  assert.equal(manifest.tabs.length, 1);
  const [tab] = manifest.tabs;
  assert.equal(tab.id, "editor");
  assert.equal(tab.title, "Éditeur");
  safePath(tab.entry, [".html"], "entry");
  safePath(tab.icon, [".svg"], "icon");
  assert.deepEqual([...tab.permissions].sort(), ["files.read", "files.write", "panes.send"]);
  assert.notEqual(tab.singleton, true, "one tab per file: another file is another tab");
});

test("the params: what is declared, what each rule lets through", () => {
  const { params } = manifest.tabs[0];
  assert.deepEqual(Object.keys(params).sort(), ["keys", "line", "path", "why"]);
  for (const param of Object.values(params)) assert.ok(param.description.trim());

  const pathRule = whole(params.path.pattern);
  for (const good of [".env", "config/app.yaml", "a b/c.json", "sub\\.env", ".env.local"]) assert.ok(pathRule.test(good), good);
  for (const bad of ["C:/x", "a:b", "a\nb", ""]) assert.ok(!pathRule.test(bad), JSON.stringify(bad));

  const keysRule = whole(params.keys.pattern);
  for (const good of ["A", "API_KEY", "A,B", "A, B ,C", "a.b-c,_d"]) assert.ok(keysRule.test(good), good);
  for (const bad of ["", "A,", ",A", "1A", "A=1", "A B", "A\nB"]) assert.ok(!keysRule.test(bad), JSON.stringify(bad));

  const lineRule = whole(params.line.pattern);
  for (const good of ["1", "42"]) assert.ok(lineRule.test(good), good);
  for (const bad of ["0", "-1", "1.5", "", "abc"]) assert.ok(!lineRule.test(bad), JSON.stringify(bad));

  assert.equal(params.why.pattern, undefined, "any sentence");
  for (const { pattern } of Object.values(params)) {
    if (pattern) assert.doesNotMatch(pattern, /\(\?[<!=]/, "Rust's regex has no look-around");
  }
});

test("the skill says what the brief says", () => {
  const skill = fs.readFileSync(path.join(KIT, "skills", "editor", "SKILL.md"), "utf8").replace(/\r\n/g, "\n");
  assert.match(skill, /^---\nname: editor\ndescription: .+\n---\n/);
  assert.match(skill, /kit_tab_open/);
  assert.match(skill, /"kit": "editor"/);
  assert.match(skill, /"tab": "editor"/);
  assert.match(skill, /Do not ask for a secret in chat/);
  assert.match(skill, /placeholder/);
  assert.match(skill, /Never read the values back/);
  assert.match(skill, /Wait/);
});

test("the tab's page loads only the kit's own files and Hangar's", () => {
  const html = fs.readFileSync(path.join(TAB, "index.html"), "utf8");
  const sources = [...html.matchAll(/(?:src|href)="([^"]+)"/g)].map((match) => match[1]);
  const own = sources.filter((source) => !source.startsWith("/_hangar/"));
  assert.deepEqual(sources.filter((source) => source.startsWith("/_hangar/")).sort(), ["/_hangar/kit-ui.js", "/_hangar/kit.css", "/_hangar/kit.js"]);
  for (const source of own) {
    assert.doesNotMatch(source, /^([a-z]+:)?\/\//i, `${source} leaves the kit`);
    assert.doesNotMatch(source, /^\//, `${source} must be relative`);
    assert.ok(fs.existsSync(path.join(TAB, source)), source);
  }
  for (const name of fs.readdirSync(TAB)) {
    const text = fs.readFileSync(path.join(TAB, name), "utf8");
    assert.doesNotMatch(text, /\bfetch\(|XMLHttpRequest|WebSocket|localStorage|sessionStorage|\beval\(|innerHTML|outerHTML|insertAdjacentHTML|document\.write|@import|url\(\s*["']?https?:|console\.log/, `${name} reaches outside what a tab may`);
  }
  const everyScript = fs.readdirSync(TAB).filter((name) => name.endsWith(".js"));
  assert.deepEqual(own.filter((source) => source.endsWith(".js")).sort(), everyScript.sort(), "every script is loaded");
});

test("the page calls nothing it did not ask for", () => {
  const allowed = new Set(["tab.params", "tab.setTitle", "tab.setBadge", "files.read", "files.write", "files.list", "panes.send"]);
  const seen = new Set();
  for (const name of fs.readdirSync(TAB).filter((file) => file.endsWith(".js"))) {
    const text = fs.readFileSync(path.join(TAB, name), "utf8");
    for (const [, method] of text.matchAll(/hangar\.call\("([\w.]+)"/g)) seen.add(method);
  }
  for (const method of seen) assert.ok(allowed.has(method), method);
  for (const method of allowed) assert.ok(seen.has(method), `${method} is declared but unused`);
});

test("the permissions and methods used are in the docs, when the docs are around", () => {
  const docs = path.join(KIT, "..", "..", "docs", "kits.md");
  if (!fs.existsSync(docs)) return; // a kit installed on its own has no docs beside it
  const text = fs.readFileSync(docs, "utf8");
  for (const permission of manifest.tabs[0].permissions) assert.ok(text.includes(`\`${permission}\``), permission);
  for (const method of ["tab.params", "tab.setTitle", "tab.setBadge", "files.list", "kit_tab_open"]) assert.ok(text.includes(`\`${method}\``), method);
});

test("nothing personal, nothing outside its folder is named", () => {
  const files = [];
  const walk = (dir) => {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      if (entry.isDirectory()) walk(path.join(dir, entry.name));
      else files.push(path.join(dir, entry.name));
    }
  };
  walk(KIT);
  const self = __filename;
  for (const file of files.filter((other) => other !== self)) {
    const text = fs.readFileSync(file, "utf8");
    assert.doesNotMatch(text, /[A-Za-z]:[\\/]Users[\\/]|\/home\/[a-z]|simnjs\.fr|@simnjs/i, file);
  }
});
