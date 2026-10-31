"use strict";
// Git's rules, and git itself as the judge: the table below is run through
// `git check-ignore` in a temporary repository when git is on the machine.
const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { spawnSync } = require("node:child_process");
const { gitignore } = require("./helpers.js");

const root = (text) => [{ dir: "", text }];

// [.gitignore, path, ignored?]
const TABLE = [
  ["", ".env", false],
  [".env\n", ".env", true],
  [".env\n", "sub/.env", true],
  ["/.env\n", ".env", true],
  ["/.env\n", "sub/.env", false],
  ["sub/.env\n", "sub/.env", true],
  ["sub/.env\n", "other/sub/.env", false],
  [".env*\n", ".env.local", true],
  [".env*\n", "sub/.env", true],
  ["*.env\n", "app.env", true],
  ["*.env\n", ".env", true],
  ["*.env\n", "sub/app.env", true],
  [".env\n!.env\n", ".env", false],
  [".env*\n!.env.example\n", ".env.example", false],
  [".env*\n!.env.example\n", ".env.local", true],
  ["!.env\n.env\n", ".env", true],
  ["# .env\n", ".env", false],
  ["\\#x\n", "#x", true],
  ["\\!x\n", "!x", true],
  ["config/\n", "config/.env", true],
  ["config/\n", "a/config/.env", true],
  ["config/\n", "config", false],
  ["/config/\n", "a/config/.env", false],
  ["config\n", "config", true],
  ["config\n", "config/.env", true],
  ["config/*\n!config/.env\n", "config/.env", false],
  ["config/\n!config/.env\n", "config/.env", true],
  ["**/.env\n", "a/b/.env", true],
  ["**/.env\n", ".env", true],
  ["a/**/.env\n", "a/.env", true],
  ["a/**/.env\n", "a/b/c/.env", true],
  ["a/**\n", "a/b/.env", true],
  ["a/**\n", "a", false],
  ["*\n", "a/b", true],
  ["*\n!a/\n!a/b\n", "a/b", false],
  [".en?\n", ".env", true],
  [".en?\n", ".env/x", true],
  [".e[nm]v\n", ".env", true],
  [".e[!n]v\n", ".env", false],
  [".e[a-z]v\n", ".env", true],
  ["  .env\n", ".env", false],
  [".env  \n", ".env", true],
  [".env\r\n", ".env", true],
  ["a b\n", "a b", true],
  ["foo*bar\n", "foo/bar", false],
  ["a/*/b\n", "a/x/b", true],
  ["a/*/b\n", "a/x/y/b", false],
  ["[\n", "[", true],
];

test("git's rules", () => {
  for (const [text, file, expected] of TABLE) {
    assert.equal(gitignore.isIgnored(root(text), file), expected, `${JSON.stringify(text)} on ${file}`);
  }
});

test("deeper .gitignore files have the last word", () => {
  const sources = [{ dir: "", text: ".env\n" }, { dir: "sub", text: "!.env\n" }];
  assert.equal(gitignore.isIgnored(sources, "sub/.env"), false);
  assert.equal(gitignore.isIgnored(sources, ".env"), true);
  assert.equal(gitignore.isIgnored(sources, "other/.env"), true);
  assert.equal(gitignore.isIgnored([{ dir: "sub", text: "/x\n" }], "sub/x"), true);
  assert.equal(gitignore.isIgnored([{ dir: "sub", text: "/x\n" }], "x"), false);
  assert.equal(gitignore.isIgnored([{ dir: "sub", text: "/x\n" }], "sub/y/x"), false);
});

test("the .gitignore files that have a say", () => {
  assert.deepEqual(gitignore.sourcesFor(".env"), [{ dir: "", path: ".gitignore" }]);
  assert.deepEqual(gitignore.sourcesFor("a/b/.env").map((source) => source.path), [".gitignore", "a/.gitignore", "a/b/.gitignore"]);
});

test("the line a click adds ignores that very file, whatever its name", () => {
  for (const file of [".env", "sub/.env", "a b/.env", "we[ird]/.env", "st*r/.env", "#x/.env", "!x/.env", "back\\slash.env"]) {
    const pattern = gitignore.patternFor(file);
    assert.ok(gitignore.isIgnored(root(`${pattern}\n`), file), `${file} by ${pattern}`);
  }
  assert.equal(gitignore.patternFor(".env"), ".env");
  assert.equal(gitignore.withPattern("", ".env"), ".env\n");
  assert.equal(gitignore.withPattern("a\n", ".env"), "a\n.env\n");
  assert.equal(gitignore.withPattern("a", ".env"), "a\n.env\n");
  assert.equal(gitignore.withPattern("a\r\nb\r\n", ".env"), "a\r\nb\r\n.env\r\n");
});

const git = (cwd, ...args) => spawnSync("git", args, { cwd, encoding: "utf8", windowsHide: true });

test("git agrees with the table", { skip: git(os.tmpdir(), "--version").status !== 0 && "git is not installed" }, () => {
  for (const [text, file, expected] of TABLE) {
    // Names Windows cannot create are for the logic above, not for git here.
    if (process.platform === "win32" && /[*?[\]]/.test(file)) continue;
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "editor-kit-"));
    try {
      git(dir, "init", "-q");
      fs.writeFileSync(path.join(dir, ".gitignore"), text);
      const answer = git(dir, "-c", "core.excludesFile=", "-c", "core.ignorecase=false", "check-ignore", "--no-index", "-q", file);
      assert.equal(answer.status === 0, expected, `git says otherwise of ${JSON.stringify(text)} on ${file}: ${answer.status} ${answer.stderr}`);
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  }
});
