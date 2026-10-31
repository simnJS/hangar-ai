"use strict";
const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { KIT, manifest } = require("./helpers");
const logic = require("../tabs/db/logic");
const core = require("../tools/core");

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
  assert.equal(manifest.id, "db");
  assert.match(manifest.version, /^\d+\.\d+\.\d+$/);
  assert.ok(manifest.name.trim() && manifest.description.trim());
  assert.equal(manifest.homepage, "https://github.com/simnJS/hangar-ai/tree/main/kits/db");
  assert.equal(manifest.hangar, ">=1.9.0");
  assert.ok(fs.existsSync(path.join(KIT, manifest.icon)));
  assert.equal(manifest.setup, undefined);
});

test("the gallery's card: a sentence that names what it runs, and its tags", () => {
  const { permissionsSummary, tags } = manifest.gallery;
  assert.match(permissionsSummary, /command/);
  assert.match(permissionsSummary, /read-only/);
  assert.match(permissionsSummary, /never its rows/);
  assert.ok(Array.isArray(tags) && tags.length > 0 && tags.every((tag) => typeof tag === "string" && tag.trim()));
});

test("the plugin is found at the root, named like the kit, with its skill", () => {
  const plugin = JSON.parse(read(".claude-plugin", "plugin.json"));
  assert.equal(plugin.name, manifest.id);
  assert.equal(plugin.version, manifest.version);
  assert.match(read("skills", "db", "SKILL.md").replace(/\r\n/g, "\n"), /^---\nname: db\ndescription: .+\n---\n/);
});

test("four declared commands, each node on the kit's own script with its verb fixed in the argv", () => {
  assert.deepEqual(Object.keys(manifest.commands).sort(), ["list", "rows", "schema", "tables"]);
  for (const [name, command] of Object.entries(manifest.commands)) {
    assert.deepEqual(command.run.slice(0, 3), ["node", "{kitDir}/tools/db.js", name], name);
    assert.ok(command.description.trim());
    assert.ok(Number.isInteger(command.timeoutSeconds) && command.timeoutSeconds >= 1 && command.timeoutSeconds <= 120);
    const placeholders = command.run.slice(3).map((item) => /^\{(\w+)\}$/.exec(item)[1]);
    assert.deepEqual(Object.keys(command.args || {}), placeholders, `${name}: every placeholder is a whole item with a rule, and the reverse`);
    for (const arg of Object.values(command.args || {})) assert.ok(arg.pattern && !/\.\*|\.\+|\\S|\\W/.test(arg.pattern), `${name}: a strict pattern`);
  }
  assert.ok(fs.existsSync(path.join(KIT, "tools", "db.js")));
  assert.equal(JSON.stringify(manifest.commands).match(/\bsql\b|query|statement/gi), null, "no free SQL");
});

test("the patterns of the manifest are the ones the script and the tab apply", () => {
  const commands = manifest.commands;
  assert.equal(commands.schema.args.path.pattern, commands.rows.args.path.pattern);
  assert.equal(commands.schema.args.table.pattern, commands.rows.args.table.pattern);
  assert.equal(commands.tables.args.path.pattern, commands.rows.args.path.pattern);
  assert.equal(`^${commands.rows.args.path.pattern}$`, core.PATH_PATTERN.source);
  assert.equal(`^${commands.rows.args.table.pattern}$`, core.TABLE_PATTERN.source);
  assert.equal(`^${commands.rows.args.path.pattern}$`, logic.PATH_RE.source);
  assert.equal(`^${commands.rows.args.table.pattern}$`, logic.TABLE_RE.source);
  assert.equal(manifest.tabs[0].params.path.pattern, commands.rows.args.path.pattern);
});

test("the tab: « Base de données », and exactly the permissions it needs", () => {
  assert.equal(manifest.tabs.length, 1);
  const [tab] = manifest.tabs;
  assert.match(tab.id, /^[a-z0-9-]{1,40}$/);
  assert.equal(tab.title, "Base de données");
  assert.deepEqual([...tab.permissions].sort(), ["command:list", "command:rows", "command:schema", "command:tables", "data.read", "data.write", "files.read", "panes.read", "panes.send"]);
  assert.ok(fs.existsSync(path.join(KIT, tab.entry)));
  assert.ok(fs.existsSync(path.join(KIT, tab.icon)));
  assert.equal(tab.singleton, true);
  assert.deepEqual(Object.keys(tab.params), ["path"]);
  assert.ok(tab.params.path.description.trim());
});

test("the page loads Hangar UI and its own files, and nothing from elsewhere", () => {
  const html = read("tabs", "db", "index.html");
  assert.match(html, /href="\/_hangar\/kit\.css"/);
  assert.match(html, /src="\/_hangar\/kit\.js"/);
  assert.match(html, /src="\/_hangar\/kit-ui\.js"/);
  for (const src of html.match(/(?:src|href)="[^"]+"/g)) assert.match(src, /="(\/_hangar\/|[a-z-]+\.(js|css)")/, src);
  assert.doesNotMatch(html, /https?:\/\//);
  assert.doesNotMatch(html, /<style/);
});

test("no colour of its own, no markup built from data, no network of its own", () => {
  for (const name of ["logic.js", "controller.js", "view.js", "app.js", "index.html", "style.css"]) {
    const source = read("tabs", "db", name);
    assert.doesNotMatch(source, /#[0-9a-fA-F]{3,8}\b|rgba?\(|hsla?\(/, name);
    assert.doesNotMatch(source, /innerHTML|insertAdjacentHTML|document\.write|eval\(/, name);
    assert.doesNotMatch(source, /fetch\(|XMLHttpRequest|WebSocket/, `${name} has no network of its own`);
  }
  const css = read("tabs", "db", "style.css");
  for (const [, value] of css.matchAll(/:\s*([^;{}]*var\(--[^;]*);/g)) assert.match(value, /--kit-/, value);
  assert.doesNotMatch(css, /var\(--(?!kit-)/);
});

test("the script cannot write: no write API, no SQL from outside, no shell", () => {
  for (const name of ["core.js", "sqlite.js", "db.js"]) {
    const source = read("tools", name).replace(/^\s*\/\/.*$/gm, "");
    assert.doesNotMatch(source, /writeFile|appendFile|unlink|rmSync|renameSync|mkdirSync|copyFile|truncateSync|ftruncate|createWriteStream|chmod/, name);
    assert.doesNotMatch(source, /shell\s*:\s*true|(?<![.\w])exec\(|execSync|execFile/, name);
    assert.doesNotMatch(source, /\b(INSERT|UPDATE|DELETE|DROP|ALTER|CREATE|REPLACE|VACUUM|ATTACH|DETACH)\b/, `${name} has no write statement`);
  }
  assert.match(read("tools", "sqlite.js"), /readOnly: true/);
  assert.match(read("tools", "sqlite.js"), /"-readonly"/);
});

test("English and French hold the same keys, and French has its accents and its glossary", () => {
  const { en, fr } = logic.STRINGS;
  const keys = (table) => new Set(Object.keys(table).map((key) => key.replace(/_(one|other)$/, "")));
  assert.deepEqual([...keys(fr)].sort(), [...keys(en)].sort());
  assert.equal(fr.tab, "Base de données");
  for (const key of Object.keys(en)) {
    const placeholders = (table) => (table[key].match(/\{\w+\}/g) || []).sort().join();
    assert.equal(placeholders(fr), placeholders(en), key);
  }
  assert.match(fr.deniedText, /Réglages › Kits/);
  assert.match(en.deniedText, /Settings › Kits/);
  assert.equal(fr.handOver, "Confier à un agent");
  assert.match(fr.secretNote, /« Confier à un agent »/);
  assert.match(fr.noDbText, /espace de travail/);
  assert.doesNotMatch(Object.values(fr).join(" "), /\b(Reglages|echec|Base de donnees|a un agent|donnees|verrouille)\b/);
});

test("nothing personal anywhere in the kit", () => {
  const forbidden = /C:\\Users|\/Users\/|\/home\/\w|@[\w-]+\.(com|fr)|simnjs\.fr|[Cc]o-[Aa]uthored/;
  for (const file of allFiles()) assert.doesNotMatch(fs.readFileSync(file, "utf8"), forbidden, path.relative(KIT, file));
});

test("the README says what it does and what it runs", () => {
  const readme = read("README.md");
  assert.match(readme, /## What it does/);
  assert.match(readme, /## What it runs/);
  for (const command of ["node {kitDir}/tools/db.js", "node:sqlite", "sqlite3", "read-only"]) assert.ok(readme.includes(command), command);
});
