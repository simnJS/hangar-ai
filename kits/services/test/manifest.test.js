"use strict";
// The manifest, checked by hand against docs/kits.md: Hangar's validator is Rust
// (src-tauri/src/kits/manifest.rs) and cannot be reached from a Node test.
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

test("the manifest's own fields, and its card for the library (index.json is left to the release)", () => {
  assert.equal(manifest.schema, 1);
  assert.equal(manifest.id, "services");
  assert.match(manifest.id, /^[a-z0-9-]{2,40}$/);
  assert.equal(manifest.name, "Services");
  assert.match(manifest.version, /^\d+\.\d+\.\d+$/);
  assert.ok(manifest.description.trim());
  assert.equal(manifest.hangar, ">=1.9.0");
  assert.equal(manifest.homepage, "https://github.com/simnJS/hangar-ai/tree/main/kits/services");
  safePath(manifest.icon, [".svg"], "icon");
  assert.ok(manifest.gallery.permissionsSummary.trim());
  assert.ok(manifest.gallery.tags.length && manifest.gallery.tags.every((tag) => typeof tag === "string" && tag.trim()));
  const index = JSON.parse(fs.readFileSync(path.join(KIT, "..", "index.json"), "utf8"));
  assert.ok(!index.kits.some((kit) => kit.id === "services"), "a kit never edits kits/index.json");
});

test("the plugin is named like the kit, and holds a skill and nothing that runs by itself", () => {
  const plugin = JSON.parse(fs.readFileSync(path.join(KIT, ".claude-plugin", "plugin.json"), "utf8"));
  assert.equal(plugin.name, manifest.id);
  assert.equal(plugin.version, manifest.version);
  for (const part of ["hooks", ".mcp.json", ".lsp.json", "monitors", "bin", "settings.json"]) assert.ok(!exists(part), part);
  for (const key of ["hooks", "mcpServers", "lspServers", "monitors", "settings", "binaries", "dependencies"]) assert.equal(plugin[key], undefined, key);
  assert.equal(manifest.setup, undefined);
});

test("three commands, all of them reads, each an argv with no shell", () => {
  assert.deepEqual(Object.keys(manifest.commands).sort(), ["docker-list", "docker-logs", "ports"]);
  for (const [name, command] of Object.entries(manifest.commands)) {
    assert.match(name, /^[a-z][a-z0-9-]*$/);
    assert.ok(command.description.trim());
    assert.ok(Array.isArray(command.run), "an argv, never a command line");
    assert.doesNotMatch(command.run[0], /[{}\\/\s]/, "a program found on the PATH");
    assert.ok(!["sh", "bash", "cmd", "powershell", "pwsh"].includes(command.run[0]));
    assert.ok(Number.isInteger(command.timeoutSeconds) && command.timeoutSeconds >= 1 && command.timeoutSeconds <= 120);
    assert.ok(!command.run.some((item) => /^(stop|kill|rm|restart|start|exec|run|prune)$/.test(item)), `${name} must not act`);
  }
  assert.deepEqual(manifest.commands.ports.run, ["node", "{kitDir}/tools/ports.js"]);
  assert.ok(exists("tools/ports.js"));
  assert.deepEqual(manifest.commands["docker-list"].run, ["docker", "ps", "-a", "--format", "json"]);
  assert.deepEqual(manifest.commands["docker-logs"].run, ["docker", "logs", "--tail", "200", "{id}"]);
});

test("the logs command takes one strict id, and nothing that is an option or a second argument", () => {
  const { args } = manifest.commands["docker-logs"];
  assert.deepEqual(Object.keys(args), ["id"]);
  const rule = whole(args.id.pattern);
  for (const good of ["3f1c9a7b2d44", "a".repeat(64), "0123456789abcdef0123"]) assert.ok(rule.test(good), good);
  for (const bad of ["", "abc", "--follow", "-f3f1c9a7b2d4", "shop-api", "3f1c9a7b2d44 --follow", "3f1c9a7b2d44;ls", "3F1C9A7B2D44", "a".repeat(65), "3f1c9a7b2d44\n"]) assert.ok(!rule.test(bad), JSON.stringify(bad));
  assert.doesNotMatch(args.id.pattern, /\(\?[<!=]/, "Rust's regex has no look-around");
  const placeholders = manifest.commands["docker-logs"].run.filter((item) => /\{\w+\}/.test(item));
  assert.deepEqual(placeholders, ["{id}"]);
});

test("the tab: the permissions the brief asks for and none more, every command it holds declared", () => {
  assert.equal(manifest.tabs.length, 1);
  const [tab] = manifest.tabs;
  assert.equal(tab.id, "services");
  assert.equal(tab.title, "Services");
  safePath(tab.entry, [".html"], "entry");
  safePath(tab.icon, [".svg"], "icon");
  assert.deepEqual([...tab.permissions].sort(), ["command:docker-list", "command:docker-logs", "command:ports", "panes.read", "panes.send"]);
  for (const permission of tab.permissions.filter((item) => item.startsWith("command:"))) assert.ok(manifest.commands[permission.slice(8)], permission);
});

test("the skill says what the brief says", () => {
  const skill = fs.readFileSync(path.join(KIT, "skills", "services", "SKILL.md"), "utf8").replace(/\r\n/g, "\n");
  assert.match(skill, /^---\nname: services\ndescription: .+\n---\n/);
  assert.match(skill, /kit_tab_open/);
  assert.match(skill, /"kit": "services"/);
  assert.match(skill, /"tab": "services"/);
  assert.match(skill, /HANGAR_PORT_BASE/);
  assert.match(skill, /Never kill/);
  assert.ok(skill.split("\n").length < 40, "short");
});

test("the tab's page loads only the kit's own files and Hangar's, and every script of it", () => {
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
  assert.deepEqual(own.filter((source) => source.endsWith(".js")).sort(), fs.readdirSync(TAB).filter((name) => name.endsWith(".js")).sort(), "every script is loaded");
});

test("the stylesheet has no colour and no pixel of its own", () => {
  const css = fs.readFileSync(path.join(TAB, "style.css"), "utf8").replace(/\/\*[\s\S]*?\*\//g, "");
  assert.doesNotMatch(css, /#[0-9a-f]{3,8}\b|rgba?\(|hsla?\(|oklch\(|\b\d+(\.\d+)?px\b/i, "colours and measures come from var(--kit-…)");
});

test("the permissions and methods used are in the docs, when the docs are around", () => {
  const docs = path.join(KIT, "..", "..", "docs", "kits.md");
  if (!fs.existsSync(docs)) return; // a kit installed on its own has no docs beside it
  const text = fs.readFileSync(docs, "utf8");
  for (const permission of ["panes.read", "panes.send"]) assert.ok(text.includes(`\`${permission}\``), permission);
  for (const method of ["command.run", "panes.list", "panes.send", "kit_tab_open"]) assert.ok(text.includes(`\`${method}\``), method);
  assert.ok(text.includes("{kitDir}"));
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
  for (const file of files.filter((other) => other !== __filename && !/\.(bin)$/.test(other))) {
    const text = fs.readFileSync(file, "utf8");
    assert.doesNotMatch(text, /[A-Za-z]:[\\/]Users[\\/]|\/home\/[a-z]|\/Users\/[a-z]|simnjs(?!\/hangar-ai)|tailscale|\b(?:100\.\d+\.\d+\.\d+|192\.168\.\d+\.\d+)\b/i, file);
  }
});
