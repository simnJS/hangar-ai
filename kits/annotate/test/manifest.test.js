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
  assert.equal(manifest.id, "annotate");
  assert.equal(manifest.name, "Annoter");
  assert.match(manifest.version, /^\d+\.\d+\.\d+$/);
  assert.ok(manifest.description.trim());
  assert.equal(manifest.hangar, ">=1.9.0");
  assert.equal(manifest.homepage, "https://github.com/simnJS/hangar-ai/tree/main/kits/annotate");
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

test("the tab: three permissions and nothing more, one tab per image", () => {
  assert.equal(manifest.tabs.length, 1);
  const [tab] = manifest.tabs;
  assert.equal(tab.id, "annotate");
  assert.equal(tab.title, "Annoter");
  safePath(tab.entry, [".html"], "entry");
  safePath(tab.icon, [".svg"], "icon");
  assert.deepEqual([...tab.permissions].sort(), ["files.read", "files.write", "panes.send"]);
  assert.notEqual(tab.singleton, true, "one tab per image: another image is another tab");
});

test("the params: only a path, and what its rule lets through", () => {
  const { params } = manifest.tabs[0];
  assert.deepEqual(Object.keys(params), ["path"]);
  assert.ok(params.path.description.trim());
  const rule = whole(params.path.pattern);
  for (const good of [".hangar/capture.png", "shots/a b.jpg", "sub\\shot.webp"]) assert.ok(rule.test(good), good);
  for (const bad of ["C:/x.png", "a:b", "a\nb", ""]) assert.ok(!rule.test(bad), JSON.stringify(bad));
  assert.doesNotMatch(params.path.pattern, /\(\?[<!=]/, "Rust's regex has no look-around");
});

test("the skill says what the brief says", () => {
  const skill = fs.readFileSync(path.join(KIT, "skills", "annotate", "SKILL.md"), "utf8").replace(/\r\n/g, "\n");
  assert.match(skill, /^---\nname: annotate\ndescription: .+\n---\n/);
  assert.match(skill, /kit_tab_open/);
  assert.match(skill, /"kit": "annotate"/);
  assert.match(skill, /"tab": "annotate"/);
  assert.match(skill, /Take the capture/);
  assert.match(skill, /Wait/);
  assert.match(skill, /Read the PNG/);
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
  const allowed = new Set(["tab.params", "files.readBinary", "files.writeBinary", "files.write", "panes.send"]);
  const seen = new Set();
  for (const name of fs.readdirSync(TAB).filter((file) => file.endsWith(".js"))) {
    const text = fs.readFileSync(path.join(TAB, name), "utf8");
    for (const [, method] of text.matchAll(/hangar\.call\("([\w.]+)"/g)) seen.add(method);
  }
  for (const method of seen) assert.ok(allowed.has(method), method);
  for (const method of allowed) assert.ok(seen.has(method), `${method} is declared but unused`);
  // files.writeBinary needs the permission the tab declares; panes.read would be one more.
  assert.ok(!seen.has("panes.list") && !seen.has("workspace.info"));
});

test("the permissions and methods used are in the docs, when the docs are around", () => {
  const docs = path.join(KIT, "..", "..", "docs", "kits.md");
  if (!fs.existsSync(docs)) return; // a kit installed on its own has no docs beside it
  const text = fs.readFileSync(docs, "utf8");
  for (const permission of manifest.tabs[0].permissions) assert.ok(text.includes(`\`${permission}\``), permission);
  for (const method of ["tab.params", "files.readBinary", "files.writeBinary", "kit_tab_open"]) assert.ok(text.includes(`\`${method}\``), method);
});

test("no colour of its own: only Hangar UI's tokens, and the annotation palette", () => {
  const colour = /#[0-9a-fA-F]{3,8}\b|\b(?:rgb|rgba|hsl|hsla)\(/;
  for (const name of fs.readdirSync(TAB)) {
    const text = fs.readFileSync(path.join(TAB, name), "utf8");
    if (name === "model.js") {
      // The palette the person picks from, painted on the image: nothing else.
      const hexes = [...text.matchAll(/#[0-9a-fA-F]{3,8}\b/g)].map((match) => match[0]);
      assert.deepEqual(new Set(hexes), new Set(["#e5484d", "#f08c00", "#2f9e44", "#3b82f6", "#d6409f", "#000000", "#ffffff"]));
    } else {
      assert.doesNotMatch(text, colour, name);
    }
  }
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
