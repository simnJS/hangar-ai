"use strict";
const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { spawnSync } = require("node:child_process");
const { KIT, tempFolder } = require("./helpers");
const { scan, blame, findMarkers, markerIn, parseIgnore, ignored, parseBlame, safeRelative, FAMILIES } = require("../tools/scan");

const SCRIPT = path.join(KIT, "tools", "scan.js");

/** Writes `files` ({ "a/b.js": "text" }) under `root`. */
function write(root, files) {
  for (const [name, content] of Object.entries(files)) {
    const full = path.join(root, name);
    fs.mkdirSync(path.dirname(full), { recursive: true });
    fs.writeFileSync(full, content);
  }
}

// The fixture tree: what must be found, what must not, and what is skipped. Built in a temp folder
// because a checked-in node_modules/ or dist/ would be ignored by the repository's own .gitignore.
const TREE = {
  "src/app.js": [
    "// TODO: wire the settings",
    "const a = 1; // FIXME(ana) breaks with an empty list",
    "const prose = 'a todo list, and a TODO inside a string';",
    'const trap = "// TODO not a comment";',
    "/* HACK: remove when v2 ships */",
    "/**",
    " * XXX the doc comment",
    " * the TODO word in the middle of a sentence is not a marker",
    " */",
    "// todo lowercase is prose",
    "const TODOS = [];",
  ].join("\n"),
  "src/lib.rs": ["fn f<'a>(x: &'a str) -> char { let c = '/'; todo!() } // TODO: handle the empty str", "// XXX: unsafe here"].join("\n"),
  "tools/run.py": ["x = 1  # FIXME: slow", '"""', "# TODO in a docstring", '"""', "#TODO no space", "s = '# TODO in a string'"].join("\n"),
  "scripts/build.sh": "#!/bin/sh\n# TODO: quote the path\necho $#TODO\n",
  "docs/page.md": "Some TODO in prose.\n<!-- TODO: document the flag -->\n",
  "style/site.css": "/* FIXME: contrast */\n.a { color: red }",
  "db/query.sql": "-- HACK: temporary index\nselect 1;",
  "Dockerfile": "# TODO: pin the digest\nFROM node",
  "data.json": '{ "note": "// TODO in json is data" }',
  "build.bin": "\u0000\u0001// TODO in a binary",
  "minified.js": `${"a=1;".repeat(600)}// TODO hidden in a long line`,
  "node_modules/dep/index.js": "// TODO: not ours",
  "target/debug/x.rs": "// TODO: build output",
  "dist/bundle.js": "// TODO: bundle",
  "deep/node_modules/x/y.js": "// TODO: nested modules",
  ".git/hooks/pre.sh": "# TODO: git internals",
  "ignored/a.js": "// TODO: in an ignored folder",
  "keep/b.js": "// TODO: negated back in",
  "a.gen.js": "// TODO: ignored by pattern",
  "keep.gen.js": "// TODO: kept by a negation",
  ".gitignore": "ignored/\n*.gen.js\n!keep.gen.js\n/keep/skip.js\n",
  "keep/skip.js": "// TODO: anchored ignore",
  "pkg/.gitignore": "generated.js\n",
  "pkg/generated.js": "// TODO: nested gitignore",
  "pkg/real.js": "// TODO: nested kept",
};

const expected = [
  ["Dockerfile", 1, "TODO", "pin the digest"],
  ["db/query.sql", 1, "HACK", "temporary index"],
  ["docs/page.md", 2, "TODO", "document the flag"],
  ["keep/b.js", 1, "TODO", "negated back in"],
  ["keep.gen.js", 1, "TODO", "kept by a negation"],
  ["pkg/real.js", 1, "TODO", "nested kept"],
  ["scripts/build.sh", 2, "TODO", "quote the path"],
  ["src/app.js", 1, "TODO", "wire the settings"],
  ["src/app.js", 2, "FIXME", "breaks with an empty list"],
  ["src/app.js", 5, "HACK", "remove when v2 ships"],
  ["src/app.js", 7, "XXX", "the doc comment"],
  ["src/lib.rs", 1, "TODO", "handle the empty str"],
  ["src/lib.rs", 2, "XXX", "unsafe here"],
  ["style/site.css", 1, "FIXME", "contrast"],
  ["tools/run.py", 1, "FIXME", "slow"],
  ["tools/run.py", 5, "TODO", "no space"],
];

function tree(t) {
  const root = tempFolder(t);
  write(root, TREE);
  return root;
}

test("finds comment markers, and only them, across languages", (t) => {
  const result = scan(tree(t));
  assert.deepEqual(
    result.items.map((item) => [item.file, item.line, item.tag, item.text]),
    expected,
  );
  assert.equal(result.schema, 1);
  assert.equal(result.total, expected.length);
  assert.deepEqual(result.counts, { TODO: 9, FIXME: 3, HACK: 2, XXX: 2 });
  assert.equal(result.truncated, false);
  assert.equal(result.incomplete, false);
});

test("skips .git, node_modules, target and dist at any depth, and what .gitignore lists", (t) => {
  const files = new Set(scan(tree(t)).items.map((item) => item.file));
  for (const skipped of ["node_modules/dep/index.js", "target/debug/x.rs", "dist/bundle.js", "deep/node_modules/x/y.js", ".git/hooks/pre.sh", "ignored/a.js", "a.gen.js", "keep/skip.js", "pkg/generated.js", "build.bin", "minified.js", "data.json"]) {
    assert.ok(!files.has(skipped), skipped);
  }
  assert.ok(files.has("keep.gen.js"), "a negation brings a file back");
  assert.ok(files.has("pkg/real.js"));
});

test("the word is not enough: prose, strings, names, lowercase and macros are not markers", () => {
  const js = FAMILIES.js;
  const none = [
    'const s = "TODO: nope";',
    "const t = `// TODO nope`;",
    "const TODOS = 1; // not TODO",
    "// the TODO is in the middle",
    "// todo: lowercase",
    "// TODOS are plural",
    "// MYTODO: glued",
    "// XXXL size",
    "todo!()",
  ];
  for (const line of none) assert.deepEqual(findMarkers(line, js), [], line);
  assert.deepEqual(findMarkers("// TODO", js).map((m) => [m.tag, m.text]), [["TODO", ""]]);
  assert.deepEqual(findMarkers("//// TODO: many slashes", js).map((m) => m.tag), ["TODO"]);
  assert.deepEqual(findMarkers("  * FIXME - dash", js).length, 0, "a line outside any comment");
  assert.equal(markerIn("TODO(ana): with an owner").text, "with an owner");
  assert.equal(markerIn(" TODO: closed */").text, "closed");
});

test("comments are followed over lines, and strings that run over lines hide theirs", () => {
  const js = FAMILIES.js;
  const block = findMarkers("/*\n TODO: first\n * FIXME second\n */ const a = 1; // HACK after", js);
  assert.deepEqual(block.map((m) => [m.line, m.tag]), [[2, "TODO"], [3, "FIXME"], [4, "HACK"]]);
  assert.deepEqual(findMarkers("const a = `\n// TODO inside\n`;\n// TODO outside", js).map((m) => m.line), [4]);
  assert.deepEqual(findMarkers("// TODO: crlf\r\n// FIXME: next", js).map((m) => m.line), [1, 2]);
});

test("the text is clipped, and lists are capped while the totals go on counting", (t) => {
  const root = tempFolder(t);
  write(root, {
    "a.js": `// TODO: ${"x".repeat(500)}\n${Array.from({ length: 30 }, (_, i) => `// FIXME: n${i}`).join("\n")}`,
    "b.js": "// HACK: in the next file",
  });
  const result = scan(root, { limit: 10 });
  assert.equal(result.items.length, 10);
  assert.equal(result.total, 32);
  assert.deepEqual(result.counts, { TODO: 1, FIXME: 30, HACK: 1, XXX: 0 });
  assert.equal(result.truncated, true);
  assert.ok(result.items[0].text.length <= 200 && result.items[0].text.endsWith("…"));
});

test("a scan that counted enough stops, and says so", (t) => {
  const root = tempFolder(t);
  write(root, { "a.js": "// TODO: 1\n// TODO: 2\n// TODO: 3", "b.js": "// TODO: 4" });
  const result = scan(root, { counted: 3 });
  assert.equal(result.incomplete, true);
  assert.equal(result.total, 3);
});

test("it never follows a link out of the folder, and an empty folder is an empty scan", (t) => {
  const root = tempFolder(t);
  const outside = tempFolder(t);
  write(outside, { "secret.js": "// TODO: elsewhere" });
  try {
    fs.symlinkSync(outside, path.join(root, "link"), "junction");
  } catch {
    /* no right to make links here: the rest still holds */
  }
  const result = scan(root);
  assert.equal(result.total, 0);
  assert.deepEqual(result.items, []);
});

test("run as the declared command: JSON on stdout, in the folder it is started in, nothing else", (t) => {
  const root = tree(t);
  const run = spawnSync(process.execPath, [SCRIPT, "--blame="], { cwd: root, encoding: "utf8" });
  assert.equal(run.status, 0);
  assert.equal(run.stderr, "");
  const result = JSON.parse(run.stdout);
  assert.equal(result.total, expected.length);
  assert.ok(!run.stdout.includes(root), "the output holds no absolute path");
  assert.ok(result.items.every((item) => !path.isAbsolute(item.file) && !item.file.includes("\\")));
});

test("gitignore patterns: anchoring, directories, wildcards, negation, nesting", () => {
  const rules = [...parseIgnore("*.tmp\n/build\ndocs/*.md\n!docs/keep.md\ncache/\n**/gen/**\nout?.txt\n# comment\n\n", "")];
  const check = (p, dir = false) => ignored(rules, p, dir);
  assert.ok(check("a.tmp") && check("deep/er/a.tmp"));
  assert.ok(check("build", true) && !check("src/build", true));
  assert.ok(check("docs/a.md") && !check("docs/keep.md") && !check("docs/sub/a.md"));
  assert.ok(check("cache", true) && check("x/cache", true) && !check("cache", false));
  assert.ok(check("a/gen/b/c.js") && check("gen/x"));
  assert.ok(check("out1.txt") && !check("out12.txt"));
  const nested = parseIgnore("*.js\n", "pkg/");
  assert.ok(ignored(nested, "pkg/a.js", false) && !ignored(nested, "other/a.js", false));
});

// ---- author and age ---------------------------------------------------------------------------

const GIT_ENV = { GIT_AUTHOR_NAME: "Ada Test", GIT_AUTHOR_EMAIL: "ada@example.test", GIT_COMMITTER_NAME: "Ada Test", GIT_COMMITTER_EMAIL: "ada@example.test" };
const git = (root, args, env = {}) => spawnSync("git", args, { cwd: root, encoding: "utf8", env: { ...process.env, ...GIT_ENV, ...env } });
const hasGit = git(process.cwd(), ["--version"]).status === 0;

test("parses git blame --line-porcelain", () => {
  const sha = "a".repeat(40);
  const text = [`${sha} 1 1 1`, "author Ada Test", "author-mail <ada@example.test>", "author-time 1700000000", "summary x", "filename f", "\tline one", `${"0".repeat(40)} 2 2 1`, "author Not Committed Yet", "author-time 1800000000", "filename f", "\tline two", ""].join("\n");
  const map = parseBlame(text);
  assert.deepEqual(map.get(1), { author: "Ada Test", at: 1700000000000 });
  assert.deepEqual(map.get(2), { author: null, at: null });
});

test("only a path inside the folder is read", () => {
  assert.equal(safeRelative("src/a.js"), "src/a.js");
  assert.equal(safeRelative("./src\\a.js"), "src/a.js");
  for (const bad of ["", "../x", "a/../b", "/etc/passwd", "C:/x", "C:\\x", "a//b", "a\0b", 3, null]) assert.equal(safeRelative(bad), null, String(bad));
});

test("blame: each marker of one file with its author and when, from a real repository", { skip: !hasGit && "git is not installed" }, (t) => {
  const root = tempFolder(t);
  assert.equal(git(root, ["init", "-q"]).status, 0);
  write(root, { "a.js": "// TODO: committed\nconst a = 1;\n", "u.js": "// TODO: never added\n" });
  git(root, ["add", "a.js"]);
  assert.equal(git(root, ["-c", "commit.gpgsign=false", "commit", "-q", "-m", "first"], { GIT_AUTHOR_DATE: "2024-01-02T03:04:05Z", GIT_COMMITTER_DATE: "2024-01-02T03:04:05Z" }).status, 0);
  fs.appendFileSync(path.join(root, "a.js"), "// FIXME: edited since\n");

  const result = blame(root, "a.js");
  assert.equal(result.status, "ok");
  assert.equal(result.file, "a.js");
  assert.deepEqual(result.items[0], { line: 1, tag: "TODO", text: "committed", author: "Ada Test", at: Date.parse("2024-01-02T03:04:05Z") });
  assert.deepEqual(result.items[1], { line: 3, tag: "FIXME", text: "edited since", author: null, at: null });

  assert.equal(blame(root, "u.js").status, "untracked");
  assert.equal(blame(root, "u.js").items[0].author, null);
  assert.equal(blame(root, "../a.js").status, "error");
  assert.equal(blame(root, "missing.js").status, "error");
});

test("blame outside a repository says so, and the command prints it as JSON", { skip: !hasGit && "git is not installed" }, (t) => {
  const root = tempFolder(t);
  write(root, { "a.js": "// TODO: loose\n" });
  const run = spawnSync(process.execPath, [SCRIPT, "--blame=a.js"], { cwd: root, encoding: "utf8", env: { ...process.env, GIT_CEILING_DIRECTORIES: path.dirname(root) } });
  assert.equal(run.status, 0);
  const result = JSON.parse(run.stdout);
  assert.equal(result.status, "no-git");
  assert.deepEqual(result.items.map((item) => [item.tag, item.text, item.author]), [["TODO", "loose", null]]);
});

test("blame without git installed is a state, not a crash", (t) => {
  const root = tempFolder(t);
  write(root, { "a.js": "// TODO: x\n" });
  const run = () => ({ error: Object.assign(new Error("spawn git ENOENT"), { code: "ENOENT" }) });
  assert.equal(blame(root, "a.js", { run }).status, "no-git");
});

// ---- read-only --------------------------------------------------------------------------------

test("the script cannot write: no write API, no network, no shell, one program run", () => {
  const source = fs.readFileSync(SCRIPT, "utf8");
  assert.doesNotMatch(source, /writeFile|appendFile|mkdir|unlink|rmSync|rmdir|rename|copyFile|createWriteStream|chmod|symlinkSync/);
  assert.doesNotMatch(source, /node:https?|node:net|node:dgram|node:dns|fetch\(|WebSocket|\beval\(|new Function|shell\s*:\s*true|(?<![.\w])exec(Sync|File)?\(|\bfork\(|\bspawn\(/);
  const run = source.match(/run\("([^"]+)", \[([^\]]*)\]/g);
  assert.equal(run.length, 1);
  assert.match(run[0], /run\("git", \["blame", "--line-porcelain", "--", file\]/);
  assert.doesNotMatch(source, /require\("(?!node:)/, "pure Node: nothing to install");
});
