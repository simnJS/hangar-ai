"use strict";
// What is refused. `check(event, ctx)` answers null, or the refusal:
// { rule, kind, subject, why }, `why` being the English sentence told to the agent.
// Nothing here writes anything, and everything it reads of the machine comes from
// `ctx`, so a test can hold any machine in a table.

const { flatten, shellScript, powershellScript, UNKNOWN, SHELLS } = require("./commands.js");
const { VALUE, bind } = require("./cmdlets.js");
const paths = require("./paths.js");

const WHY = {
  "recursive-delete/outside": "it deletes {subject}, outside the project",
  "recursive-delete/project": "it deletes the whole project ({subject})",
  "recursive-delete/protected": "it deletes {subject}, the project's git history",
  "recursive-delete/unreadable": "it deletes {subject}, and where that points cannot be read from the command line",
  "force-push/default": "it force-pushes to {subject}, a default branch: that rewrites its history for everyone",
  "git-reset-hard/any": "git reset --hard throws away uncommitted work, which git cannot bring back",
  "git-clean/any": "git clean --force deletes untracked files, which git cannot bring back",
  "drop-database/any": "it drops a whole database",
  "download-to-shell/any": "it runs code straight from a download, without anyone reading it",
  "encoded-command/any": "it hands PowerShell a command in base64 (-EncodedCommand), which nobody can read before it runs",
  "secret-file/read": "it reads or prints {subject}, a secret file",
  "secret-file/copy": "it copies {subject}, a secret file, out to where it is no longer treated as one",
  "secret-file/write": "it overwrites {subject}, a secret file",
  "guard-files/any": "it changes {subject}, one of the guard's own files, which are the person's to change",
};

function refusal(rule, kind, subject) {
  const clean = String(subject === undefined ? "" : subject).split(UNKNOWN).join("…");
  return { rule, kind, subject: clean, why: WHY[`${rule}/${kind}`].replace("{subject}", clean) };
}

const on = (rule) => rule && rule.enabled !== false;

// ---------------------------------------------------------------------------
// Recursive deletes
// ---------------------------------------------------------------------------

const REMOVE_RECURSE = /^-r(e(c(u(r(s(e)?)?)?)?)?)?$/i;

/** { recursive, targets } when this command deletes, else null. */
function deleteSpec(c) {
  const { program, args } = c;
  if (c.windows && ["remove-item", "ri", "rm", "del", "erase", "rd", "rmdir"].includes(program)) {
    let recursive = false;
    const targets = [];
    for (const arg of args) {
      // `rm -rf` here is Git's rm found on the PATH; PowerShell's own `rm` takes -Recurse.
      if (REMOVE_RECURSE.test(arg.text) || /^\/s$/i.test(arg.text) || (program === "rm" && /^-[fvdiI]*[rR][rRfvdiI]*$/.test(arg.text))) recursive = true;
      else if (!arg.text.startsWith("-") && !/^\/[a-z]$/i.test(arg.text)) {
        for (const text of arg.text.split(",")) targets.push({ ...arg, text, texts: [text] });
      }
    }
    return { recursive, targets };
  }
  if (program === "rm") {
    let recursive = false;
    let ended = false;
    const targets = [];
    for (const arg of args) {
      const { text } = arg;
      if (!ended && text === "--") ended = true;
      else if (!ended && text.length > 1 && text.startsWith("-")) {
        if (text === "--recursive" || /^-[a-zA-Z]*[rR]/.test(text)) recursive = true;
      } else targets.push(arg);
    }
    return { recursive, targets };
  }
  if (program === "rimraf") {
    return { recursive: true, targets: args.filter((arg) => !arg.text.startsWith("-")) };
  }
  return null;
}

/** Where a delete lands: { where: "inside" | "project" | "outside" | "protected" | "unreadable", path }. */
function classifyTarget(text, glob, cwd, ctx) {
  if (text === "") return { where: "inside" }; // `rm -rf ""` deletes nothing
  if (text.startsWith(UNKNOWN)) return { where: "unreadable" };
  // `HKCU:\x`, `Env:\x`: a PowerShell provider's, which no project holds.
  if (ctx.platform === "win32" && /^[a-zA-Z]{2,}:/.test(text)) return { where: "outside", path: text };
  const slashed = ctx.platform === "win32" ? text.replace(/\\/g, "/") : text;
  const kept = [];
  for (const segment of slashed.split("/")) {
    if (segment === UNKNOWN) break; // it may be empty, which leaves the folder before it
    if (glob && /[*?[]/.test(segment)) {
      // `*` is all of the folder, as good as naming it; `*.log` is something in it.
      if (!/^(\*+|\.\*|\*\.\*|\.\[!\.\]\*|\.\?\*)$/.test(segment)) kept.push("x");
      break;
    }
    kept.push(segment.split(UNKNOWN).join("x"));
  }
  const candidate = kept.join("/") || (slashed.startsWith("/") ? "/" : ".");
  const path = paths.normalize(candidate === "" ? "." : candidate, cwd, ctx.platform);
  if (path === null) return { where: "unreadable" };
  for (const root of ctx.roots) {
    if (paths.same(path, root, ctx.platform)) return { where: "project", path };
  }
  const root = ctx.roots.find((candidateRoot) => paths.isUnder(path, candidateRoot, ctx.platform));
  if (root) {
    const held = ctx.rules.recursiveDelete.protectedInsideProject.some((name) =>
      paths.same(path, paths.normalize(name, root, ctx.platform), ctx.platform),
    );
    return { where: held ? "protected" : "inside", path };
  }
  if (ctx.allowedOutside.some((allowed) => paths.isUnder(path, allowed, ctx.platform))) return { where: "inside", path };
  return { where: "outside", path };
}

/** `narrowed`: it only takes what a filter picks, so naming the project's folder is no harm. */
function targetRefusal(arg, cwd, ctx, narrowed = false) {
  for (const text of arg.texts) {
    const { where, path } = classifyTarget(text, arg.glob, cwd, ctx);
    if (where === "inside" || (where === "project" && narrowed)) continue;
    return refusal("recursive-delete", where, where === "unreadable" ? arg.raw : path);
  }
  return null;
}

// A filter that picks some files: `-type f` alone still takes them all.
const FIND_NARROWS = new Set(["-name", "-iname", "-path", "-ipath", "-wholename", "-regex", "-iregex", "-mtime", "-mmin", "-newer", "-size", "-empty"]);

function findRefusal(find, ctx) {
  const starts = [];
  for (const arg of find.args) {
    if (arg.text.startsWith("-") || arg.text === "(" || arg.text === "!") break;
    starts.push(arg);
  }
  if (!starts.length) starts.push({ text: ".", texts: ["."], raw: ".", glob: false });
  const narrowed = find.args.some((arg) => FIND_NARROWS.has(arg.text));
  for (const arg of starts) {
    const refused = targetRefusal(arg, find.cwd, ctx, narrowed);
    if (refused) return refused;
  }
  return null;
}

// What leaves the pipeline's items as they were, or only some of them.
const PASS_ALONG = new Set(["foreach-object", "where-object", "select-object", "sort-object", "write-host", "tee-object"]);
const NARROWING = new Set(["where-object", "select-object"]);
const HERE = { text: ".", texts: ["."], raw: ".", glob: false, exact: true };

/** What stands before a command in its pipeline, nearest first, going out of the blocks it is in. */
function upstreamOf(c, all) {
  const found = [];
  for (let level = c; level; level = level.owner) {
    const peers = all.filter((other) => other.pipeline === level.pipeline && other.seq < level.seq && other.owner === level.owner);
    found.push(...peers.reverse());
    if (level.owner) found.push(level.owner);
  }
  return found;
}

/** What comes down the pipeline into a PowerShell command, or null when nothing does. */
function pipelineFeed(c, all) {
  let narrowed = false;
  for (const other of upstreamOf(c, all)) {
    if (PASS_ALONG.has(other.cmdlet)) {
      narrowed = narrowed || NARROWING.has(other.cmdlet);
    } else if (other.program === VALUE) {
      return { kind: "values", args: other.args, narrowed };
    } else if (other.cmdlet === "get-childitem" || other.cmdlet === "get-item") {
      const bound = bind(other);
      // Get-ChildItem's second plain word is its filter.
      const starts = [...bound.named("path"), ...bound.named("literalpath"), ...bound.named("pspath"), ...bound.named("lp"), ...bound.plain.slice(0, 1)];
      const filters = [...bound.named("filter"), ...bound.named("include"), ...bound.plain.slice(1, 2)];
      return { kind: "files", starts, filters, cwd: other.cwd, recurse: bound.on("recurse"), narrowed: narrowed || filters.length > 0 };
    } else {
      return { kind: "unknown" };
    }
  }
  return null;
}

const isPipedItem = (arg) => /^\$(_|psitem)\b/i.test(arg.raw);

function powershellDelete(c, all, ctx) {
  const bound = bind(c);
  if (bound.on("whatif")) return null; // it only says what it would do
  const flagged = c.args.some((arg) => /^\/s$/i.test(arg.text) || (c.program === "rm" && /^-[fvdiI]*[rR][rRfvdiI]*$/.test(arg.text)));
  const recursive = bound.on("recurse") || flagged;
  const splat = c.args.find((arg) => /^@[A-Za-z_]/.test(arg.raw));
  if (splat) return refusal("recursive-delete", "unreadable", splat.raw); // `@p` can hold -Recurse and any path
  const targets = bound.files.filter((arg) => !/^\/[a-z]$/i.test(arg.text));
  const named = targets.filter((arg) => !isPipedItem(arg));
  const narrowed = bound.has("filter") || bound.has("include");
  if (recursive) {
    for (const arg of named) {
      const refused = targetRefusal(arg, c.cwd, ctx, narrowed);
      if (refused) return refused;
    }
  }
  if (named.length && named.length === targets.length) return null;
  // The list comes from the pipeline: what feeds it says what it can reach.
  const feed = pipelineFeed(c, all);
  if (!feed) return recursive && targets.length ? refusal("recursive-delete", "unreadable", targets[0].raw) : null;
  if (feed.kind === "files") {
    if (!recursive && !feed.recurse) return null;
    for (const arg of feed.starts.length ? feed.starts : [HERE]) {
      const refused = targetRefusal(arg, feed.cwd, ctx, feed.narrowed || narrowed);
      if (refused) return refused;
    }
    return null;
  }
  if (!recursive) return null;
  if (feed.kind === "values") {
    for (const arg of feed.args) {
      const refused = targetRefusal(arg, c.cwd, ctx, feed.narrowed || narrowed);
      if (refused) return refused;
    }
    return null;
  }
  return refusal("recursive-delete", "unreadable", "what the pipeline feeds it");
}

function recursiveDelete(c, all, ctx) {
  if (c.cmdlet === "remove-item") return powershellDelete(c, all, ctx);
  if (c.program === "[directory]::delete") {
    const [target, flag] = c.args;
    return target && flag && flag.text !== "$false" ? targetRefusal(target, c.cwd, ctx) : null;
  }
  // `(Get-Item x).Delete($true)`: nothing on the line says what it is called on.
  if (c.program === ".delete" && c.powershell && c.args[0] && c.args[0].text === "$true") {
    return refusal("recursive-delete", "unreadable", "what .Delete($true) is called on");
  }
  if (c.program === "find" && c.args.some((arg) => arg.text === "-delete")) return findRefusal(c, ctx);
  const spec = deleteSpec(c);
  if (!spec || !spec.recursive) return null;
  for (const arg of spec.targets) {
    if (arg.found) continue; // named by `find`: looked at below
    const refused = targetRefusal(arg, c.cwd, ctx);
    if (refused) return refused;
  }
  if (c.fromFind) return findRefusal(c.fromFind, ctx);
  if (c.xargs) {
    // The list comes from stdin: what feeds it says what it can reach.
    const feed = all.filter((other) => other.pipeline === c.pipeline && other.seq < c.seq).pop();
    if (feed && feed.program === "find") return findRefusal(feed, ctx);
    if (feed && ["echo", "printf"].includes(feed.program)) {
      for (const arg of feed.args) {
        for (const text of arg.text.split(/\s+/).filter(Boolean)) {
          const refused = targetRefusal({ ...arg, texts: [text] }, c.cwd, ctx);
          if (refused) return refused;
        }
      }
      return null;
    }
    return refusal("recursive-delete", "unreadable", "what xargs reads from stdin");
  }
  return null;
}

// ---------------------------------------------------------------------------
// Git
// ---------------------------------------------------------------------------

const GIT_VALUE_OPTIONS = new Set(["-c", "--git-dir", "--work-tree", "--namespace", "--super-prefix", "--config-env"]);

/** { sub, args, cwd } for a `git` command. */
function gitCommand(c, ctx) {
  if (c.program !== "git") return null;
  let { cwd } = c;
  let i = 0;
  while (i < c.args.length) {
    const { text, exact } = c.args[i];
    if (text === "-C") {
      const dir = c.args[i + 1];
      cwd = dir && dir.exact && !/^\s*$/.test(dir.text) ? paths.normalize(dir.text, cwd, ctx.platform) : null;
      i += 2;
    } else if (GIT_VALUE_OPTIONS.has(text)) {
      i += 2;
    } else if (text.startsWith("-") && exact) {
      i++;
    } else {
      break;
    }
  }
  if (i >= c.args.length) return null;
  return { sub: c.args[i].text, args: c.args.slice(i + 1), cwd };
}

const PUSH_VALUE_OPTIONS = new Set(["-o", "--push-option", "--repo", "--receive-pack", "--exec"]);

function forcePush(c, ctx) {
  const git = gitCommand(c, ctx);
  if (!git || git.sub !== "push") return null;
  let force = false;
  let everything = false;
  const positional = [];
  for (let i = 0; i < git.args.length; i++) {
    const { text } = git.args[i];
    if (text === "--") {
      positional.push(...git.args.slice(i + 1).map((arg) => arg.text));
      break;
    }
    if (PUSH_VALUE_OPTIONS.has(text)) {
      i++;
    } else if (text === "--force" || text === "--force-with-lease" || text.startsWith("--force-with-lease=")) {
      force = true;
    } else if (text === "--no-force" || text === "--no-force-with-lease") {
      force = false;
    } else if (text === "--mirror" || text === "--all" || text === "--branches") {
      everything = true;
    } else if (/^-[a-zA-Z]+$/.test(text)) {
      if (text.includes("f")) force = true;
    } else if (!text.startsWith("-")) {
      positional.push(text);
    }
  }
  const refspecs = positional.slice(1);
  const defaults = new Set(ctx.rules.forcePush.defaultBranches);
  const head = git.cwd === null ? null : ctx.originHead(git.cwd);
  if (head) defaults.add(head);
  const current = () => (git.cwd === null ? null : ctx.currentBranch(git.cwd));

  if (everything && (force || refspecs.some((spec) => spec.startsWith("+")))) {
    return refusal("force-push", "default", [...defaults][0] || "every branch");
  }
  if (!refspecs.length) {
    if (!force) return null;
    const branch = current();
    // Not on a branch this can read: nothing says it is not the default one.
    if (branch === null) return refusal("force-push", "default", "the current branch");
    return defaults.has(branch) ? refusal("force-push", "default", branch) : null;
  }
  for (const spec of refspecs) {
    const forced = force || spec.startsWith("+");
    if (!forced) continue;
    const bare = spec.replace(/^\+/, "");
    const source = bare.includes(":") ? bare.slice(bare.lastIndexOf(":") + 1) : bare;
    let branch = source.replace(/^refs\/heads\//, "");
    if (branch === "HEAD" || branch === "@") {
      const name = current();
      if (name === null) return refusal("force-push", "default", "the current branch");
      branch = name;
    }
    if (branch.includes(UNKNOWN)) return refusal("force-push", "default", spec);
    if (defaults.has(branch)) return refusal("force-push", "default", branch);
  }
  return null;
}

function gitResetHard(c, ctx) {
  const git = gitCommand(c, ctx);
  if (!git || git.sub !== "reset") return null;
  const end = git.args.findIndex((arg) => arg.text === "--");
  const options = end >= 0 ? git.args.slice(0, end) : git.args;
  return options.some((arg) => arg.text === "--hard") ? refusal("git-reset-hard", "any") : null;
}

function gitClean(c, ctx) {
  const git = gitCommand(c, ctx);
  if (!git || git.sub !== "clean") return null;
  let force = false;
  let dry = false;
  for (const { text } of git.args) {
    if (text === "--") break;
    if (text === "--force") force = true;
    else if (text === "--dry-run") dry = true;
    else if (/^-[a-zA-Z]+$/.test(text)) {
      if (text.includes("f")) force = true;
      if (text.includes("n")) dry = true;
    }
  }
  return force && !dry ? refusal("git-clean", "any") : null;
}

// ---------------------------------------------------------------------------
// Dropping a database
// ---------------------------------------------------------------------------

const CONTAINER_RUNNERS = new Set(["docker", "podman", "kubectl", "oc", "ssh"]);

function dropDatabase(c, all, ctx) {
  const rule = ctx.rules.dropDatabase;
  const words = [c.program, ...c.args.filter((arg) => !arg.text.startsWith("-")).map((arg) => arg.text)];
  for (const sequence of rule.commands) {
    let at = 0;
    for (const word of words) if (at < sequence.length && word.toLowerCase() === sequence[at].toLowerCase()) at++;
    // The first word must be the program, or `grep dropdb notes` would count.
    if (at === sequence.length && words[0] === sequence[0].toLowerCase()) return refusal("drop-database", "any");
  }
  const client = rule.clients.includes(c.program) || (CONTAINER_RUNNERS.has(c.program) && c.args.some((arg) => rule.clients.includes(arg.text.split("/").pop())));
  if (!client) return null;
  const sql = ctx.sqlPattern;
  if (c.args.some((arg) => arg.texts.some((text) => sql.test(text)))) return refusal("drop-database", "any");
  if (c.stdin !== null && sql.test(c.stdin)) return refusal("drop-database", "any");
  const feeds = all.filter((other) => other.pipeline === c.pipeline && other.seq < c.seq && (["echo", "printf", "cat", VALUE].includes(other.program) || other.cmdlet === "write-output"));
  for (const feed of feeds) {
    if (feed.stdin !== null && sql.test(feed.stdin)) return refusal("drop-database", "any");
    if (feed.program !== "cat" && feed.args.some((arg) => sql.test(arg.text))) return refusal("drop-database", "any");
  }
  return null;
}

// ---------------------------------------------------------------------------
// Running a download
// ---------------------------------------------------------------------------

const INLINE_CODE = /^-(c|e|m|p|r|E)$|^--(eval|print)$/;

const POWERSHELLS = new Set(["powershell", "pwsh"]);
// What runs the text or file it is given, in PowerShell.
const POWERSHELL_RUNNERS = new Set(["iex", "invoke-expression", "[scriptblock]::create", ".invokescript"]);

function readsCodeFromStdin(c, rule) {
  if (SHELLS.has(c.program)) return Boolean(shellScript(c.args).stdin);
  if (POWERSHELLS.has(c.program)) return Boolean(powershellScript(c.args).stdin);
  if (c.program === "iex" || c.program === "invoke-expression") return true;
  if (!rule.interpreters.includes(c.program)) return false;
  for (const { text } of c.args) {
    if (text === "-") return true;
    if (INLINE_CODE.test(text)) return false;
    if (!text.startsWith("-")) return false;
  }
  return true;
}

function downloadedFiles(c, ctx) {
  const files = [];
  const add = (arg) => {
    if (arg && arg.exact) {
      const path = paths.normalize(arg.text, c.cwd, ctx.platform);
      if (path) files.push(path);
    }
  };
  c.args.forEach((arg, i) => {
    if (arg.text === "-o" || arg.text === "--output" || (c.program === "wget" && arg.text === "-O")) add(c.args[i + 1]);
    const long = /^--(output|output-document)=(.+)$/.exec(arg.text);
    if (long) add({ ...arg, text: long[2] });
    const saved = c.powershell ? /^-(?:o|ou|out|outf|outfi|outfil|outfile|d|de|des|dest|desti|destin|destina|destinat|destinati|destinatio|destination)(:.+)?$/i.exec(arg.text) : null;
    if (saved) add(saved[1] ? { ...arg, text: saved[1].slice(1) } : c.args[i + 1]);
  });
  if (c.program === ".downloadfile") add(c.args[1]);
  for (const redirect of c.redirects) if (redirect.op === ">" || redirect.op === ">>") add(redirect.target);
  return files;
}

function downloadToShell(c, all, downloads, ctx) {
  const rule = ctx.rules.downloadToShell;
  const isDownload = (other) => rule.downloaders.includes(other.program);
  const runsCode = SHELLS.has(c.program) || POWERSHELLS.has(c.program) || rule.interpreters.includes(c.program) || POWERSHELL_RUNNERS.has(c.program) || ["eval", "source", "."].includes(c.program);

  if (readsCodeFromStdin(c, rule) && all.some((other) => other.pipeline === c.pipeline && other.seq < c.seq && isDownload(other))) {
    return refusal("download-to-shell", "any");
  }
  if (runsCode && c.args.some((arg) => arg.commands.some(isDownload))) return refusal("download-to-shell", "any");
  // PowerShell's `iex (irm …)` or `$s = irm …; iex $s`: the download sits beside it, in the same
  // text. `iex '1+1'` says what it runs, and that was read on its own.
  if (c.windows && POWERSHELL_RUNNERS.has(c.program) && !c.args.some((arg) => arg.exact && !arg.text.startsWith("-")) && all.some(isDownload)) {
    return refusal("download-to-shell", "any");
  }

  // Saved, then run, in the same command line.
  const isSaved = (text) => {
    const path = paths.normalize(text, c.cwd, ctx.platform);
    return path !== null && downloads.some((file) => paths.same(file, path, ctx.platform));
  };
  if (c.path && /[/\\]/.test(c.path) && isSaved(c.path)) return refusal("download-to-shell", "any");
  if (SHELLS.has(c.program)) {
    const { file } = shellScript(c.args);
    if (file && isSaved(file.text)) return refusal("download-to-shell", "any");
  } else if (POWERSHELLS.has(c.program)) {
    const { file } = powershellScript(c.args);
    if (file && isSaved(file.text)) return refusal("download-to-shell", "any");
  } else if (runsCode && c.program !== "eval") {
    const first = c.args.find((arg) => !arg.text.startsWith("-"));
    if (first && isSaved(first.text)) return refusal("download-to-shell", "any");
  }
  return null;
}

// ---------------------------------------------------------------------------
// Secrets
// ---------------------------------------------------------------------------

/** The rule's pattern a path answers to, or null. */
function matchSecret(path, ctx) {
  const rule = ctx.rules.secrets;
  if (rule.except.some((pattern) => paths.matchesPattern(path, pattern, ctx.home))) return null;
  return rule.paths.find((pattern) => paths.matchesPattern(path, pattern, ctx.home)) || null;
}

const GLOB_FILLERS = ["", "local", "rsa"];

/** What a glob would name by itself: `.env*` names `.env`; `*.json` names nothing secret. */
function globCandidates(text) {
  return GLOB_FILLERS.map((filler) => text.replace(/\*+/g, filler).replace(/\?/g, "x")).filter((candidate) => candidate && !candidate.endsWith("/"));
}

function secretPath(text, glob, cwd, ctx) {
  const clean = text.split(UNKNOWN).join("x");
  for (const candidate of glob ? globCandidates(clean) : [clean]) {
    const path = paths.normalize(candidate, cwd, ctx.platform) || paths.normalize(candidate, "/", ctx.platform);
    if (path && matchSecret(path, ctx)) return text.split(UNKNOWN).join("…");
  }
  return null;
}

function secretArg(arg, cwd, ctx) {
  for (const text of arg.texts) {
    if (!text || text === UNKNOWN) continue;
    const found = secretPath(text, arg.glob, cwd, ctx);
    if (found) return found;
  }
  return null;
}

const withText = (arg, text) => ({ ...arg, text, texts: [text] });

// Those that open a file object piped to them. `gci .env | sort` only sorts the objects.
const READS_PIPED_FILES = new Set(["get-content", "select-string", "format-hex", "compress-archive"]);

/** What a PowerShell reader gets from its pipeline, as paths to look at. */
function pipedFiles(c, all) {
  if (!READS_PIPED_FILES.has(c.cmdlet)) return [];
  const feed = pipelineFeed(c, all);
  if (!feed) return [];
  // Only Get-Content takes a piped text for a path: to Select-String it is what to search.
  if (feed.kind === "values") return c.cmdlet === "get-content" ? feed.args : [];
  if (feed.kind !== "files") return [];
  const found = [];
  for (const start of feed.starts) {
    for (const filter of feed.filters.length ? feed.filters : [null]) {
      // A folder is read for what is in it: `gci ~/.ssh | gc` reads the keys there.
      found.push(withText(start, `${start.text.replace(/[\\/]+$/, "")}/${filter ? filter.text : "local"}`));
    }
    if (!feed.filters.length) found.push(start);
  }
  if (!feed.starts.length) found.push(...feed.filters);
  return found;
}

/** The files a PowerShell reader reads. */
function powershellReads(c, all) {
  const { files } = bind(c);
  const named = files.filter((arg) => !isPipedItem(arg));
  return named.length === files.length && named.length ? named : [...named, ...pipedFiles(c, all)];
}

/** The files a PowerShell writer overwrites: appending leaves what is there, as `>>` does. */
function powershellWrites(c) {
  if (!c.cmdlet) return c.args.slice(0, 1); // [IO.File]::WriteAllText(path, …)
  const bound = bind(c);
  if (bound.on("append")) return [];
  if (c.cmdlet === "new-item") return bound.on("force") ? [...bound.files, ...bound.named("name")] : [];
  return bound.files;
}

/** Where a PowerShell copy comes from and goes. */
function powershellCopy(c) {
  const bound = bind(c);
  const to = bound.named("destination")[0] || bound.named("newname")[0] || (bound.named("path").length || bound.named("literalpath").length ? bound.plain[0] : bound.plain[1]);
  return { from: bound.files, to };
}

// Programs whose first plain argument is a pattern or a script, not a file. And the
// options of each that take a value, so that value is not read as that argument.
const SCRIPT_FIRST = {
  grep: ["-A", "-B", "-C", "-m", "-d", "-D"],
  egrep: ["-A", "-B", "-C", "-m", "-d", "-D"],
  fgrep: ["-A", "-B", "-C", "-m", "-d", "-D"],
  rg: ["-A", "-B", "-C", "-m", "-t", "-T", "-j", "-M", "-r", "-E"],
  ag: ["-A", "-B", "-C", "-m", "-G"],
  ack: ["-A", "-B", "-C", "-m"],
  findstr: [],
  sed: ["-l"],
  awk: ["-F", "-v"],
  gawk: ["-F", "-v"],
  mawk: ["-F", "-v"],
  jq: ["--indent", "--tab"],
  yq: ["--indent"],
};
const SCRIPT_GIVEN = new Set(["-e", "--regexp", "--expression", "-f", "--file"]);

// What openssl prints a key with. Elsewhere it uses the key without showing it.
const OPENSSL_PRINTS = new Set(["rsa", "ec", "dsa", "pkey", "pkcs8", "pkcs12", "enc", "base64", "asn1parse"]);

/** The arguments of a reading program that name a file it reads. */
function readTargets(c) {
  if (c.program === "openssl" && !(c.args[0] && OPENSSL_PRINTS.has(c.args[0].text))) return [];
  const scriptFirst = SCRIPT_FIRST[c.program];
  const targets = [];
  let scriptDone = !scriptFirst;
  for (let i = 0; i < c.args.length; i++) {
    const arg = c.args[i];
    const { text } = arg;
    if (c.program === "dd" && text.startsWith("if=")) {
      targets.push(withText(arg, text.slice(3)));
    } else if (text === "--exclude" || text === "--exclude-dir" || (c.program === "zip" && text === "-x")) {
      i++;
    } else if (text.startsWith("--exclude")) {
      // left out on purpose: it names what not to read
    } else if (text.length > 1 && text.startsWith("-")) {
      const equal = text.indexOf("=");
      if (SCRIPT_GIVEN.has(text) || (equal > 0 && SCRIPT_GIVEN.has(text.slice(0, equal)))) scriptDone = true;
      if (text === "-f" || text === "--file") {
        if (c.args[i + 1]) targets.push(c.args[i + 1]);
        i++;
      } else if (text === "-e" || text === "--regexp" || text === "--expression") {
        i++;
      } else if (equal > 0) {
        targets.push(withText(arg, text.slice(equal + 1)));
      } else if (scriptFirst && scriptFirst.includes(text)) {
        i++;
      }
    } else if (!scriptDone) {
      scriptDone = true;
    } else if (!text.startsWith("!")) {
      targets.push(arg);
    }
  }
  return targets;
}

/** What curl or wget would send: `@file`, `-T file`, `--post-file=file`. */
function uploadTargets(c) {
  const targets = [];
  c.args.forEach((arg, i) => {
    const { text } = arg;
    if (text === "-T" || text === "--upload-file" || text === "--post-file" || text === "--body-file") {
      if (c.args[i + 1]) targets.push(c.args[i + 1]);
    }
    const long = /^--(upload-file|post-file|body-file)=(.+)$/.exec(text);
    if (long) targets.push(withText(arg, long[2]));
    const at = text.indexOf("@");
    if (at >= 0 && (at === 0 || /[=:]$/.test(text.slice(0, at)) || /^-/.test(text))) {
      targets.push(withText(arg, text.slice(at + 1).split(";")[0]));
    }
  });
  return targets;
}

const INLINE_PROGRAMS = {
  python: ["-c"],
  node: ["-e", "-p", "-pe", "--eval", "--print"],
  bun: ["-e", "--eval"],
  deno: ["eval"],
  perl: ["-e", "-E", "-ne", "-pe", "-lne", "-lpe"],
  ruby: ["-e"],
  php: ["-r"],
};

/** The quoted strings in the one-liners given to an interpreter. */
function inlineStrings(c) {
  const flags = INLINE_PROGRAMS[c.program];
  if (!flags) return [];
  const found = [];
  c.args.forEach((arg, i) => {
    if (!flags.includes(arg.text) || !c.args[i + 1]) return;
    for (const match of c.args[i + 1].text.matchAll(/'([^'\n]*)'|"([^"\n]*)"|`([^`\n]*)`/g)) {
      found.push(match[1] ?? match[2] ?? match[3]);
    }
  });
  return found;
}

const COPIER_VALUE_OPTIONS = new Set(["--exclude", "--include", "--exclude-from", "--include-from", "--filter", "-f", "-e", "--rsh", "-m", "-o", "-g"]);

function secretFile(c, all, ctx) {
  const rule = ctx.rules.secrets;
  for (const { op, target } of c.redirects) {
    if (!target) continue;
    if (op === "<" || op === "<>") {
      const found = secretArg(target, c.cwd, ctx);
      if (found) return refusal("secret-file", "read", found);
    } else if (op === ">" || op === ">|" || op === "&>") {
      const found = secretArg(target, c.cwd, ctx);
      if (found) return refusal("secret-file", "write", found);
    }
  }
  if (rule.readers.includes(c.program)) {
    for (const arg of c.cmdlet ? powershellReads(c, all) : readTargets(c)) {
      const found = secretArg(arg, c.cwd, ctx);
      if (found) return refusal("secret-file", "read", found);
    }
  }
  if (c.powershell && rule.writers.includes(c.program)) {
    for (const arg of powershellWrites(c)) {
      const found = secretArg(arg, c.cwd, ctx);
      if (found) return refusal("secret-file", "write", found);
    }
  }
  if (c.cmdlet && rule.copiers.includes(c.program)) {
    const { from, to } = powershellCopy(c);
    if (to && !secretArg(to, c.cwd, ctx)) {
      for (const arg of from) {
        const found = secretArg(arg, c.cwd, ctx);
        if (found) return refusal("secret-file", "copy", found);
      }
    }
  } else if (rule.copiers.includes(c.program)) {
    const plain = [];
    for (let i = 0; i < c.args.length; i++) {
      const arg = c.args[i];
      if (COPIER_VALUE_OPTIONS.has(arg.text)) i++;
      else if (!arg.text.startsWith("-")) plain.push(arg);
    }
    const destination = plain[plain.length - 1];
    if (plain.length >= 2 && !secretArg(destination, c.cwd, ctx)) {
      for (const arg of plain.slice(0, -1)) {
        const found = secretArg(arg, c.cwd, ctx);
        if (found) return refusal("secret-file", "copy", found);
      }
    }
  }
  if (c.program === "curl" || c.program === "wget") {
    for (const arg of uploadTargets(c)) {
      const found = secretArg(arg, c.cwd, ctx);
      if (found) return refusal("secret-file", "read", found);
    }
  }
  if (c.powershell && ctx.rules.downloadToShell.downloaders.includes(c.program)) {
    const inFile = c.args.findIndex((arg) => /^-inf(i(le?)?)?$/i.test(arg.text));
    const found = inFile >= 0 && c.args[inFile + 1] ? secretArg(c.args[inFile + 1], c.cwd, ctx) : null;
    if (found) return refusal("secret-file", "read", found);
  }
  const git = gitCommand(c, ctx);
  if (git && ["show", "diff", "blame", "cat-file"].includes(git.sub) || (git && git.sub === "log" && git.args.some((arg) => arg.text === "-p" || arg.text === "--patch" || arg.text === "-u"))) {
    for (const arg of git.args) {
      if (arg.text.startsWith("-")) continue;
      const found = secretArg(withText(arg, arg.text.slice(arg.text.lastIndexOf(":") + 1)), git.cwd, ctx);
      if (found) return refusal("secret-file", "read", found);
    }
  }
  for (const text of inlineStrings(c)) {
    const found = text ? secretPath(text, false, c.cwd, ctx) : null;
    if (found) return refusal("secret-file", "read", found);
  }
  return null;
}

// ---------------------------------------------------------------------------
// The guard's own files
// ---------------------------------------------------------------------------

function isGuarded(text, cwd, ctx) {
  const path = paths.normalize(text.split(UNKNOWN).join("x"), cwd, ctx.platform);
  if (!path) return false;
  return ctx.guardedFiles.some((file) => paths.same(path, file, ctx.platform)) || ctx.guardedFolders.some((folder) => paths.same(path, folder, ctx.platform) || paths.isUnder(path, folder, ctx.platform));
}

function guardFiles(c, ctx) {
  for (const { op, target } of c.redirects) {
    if (!target || op === "<" || op === "<&" || op === ">&") continue;
    if (target.texts.some((text) => isGuarded(text, c.cwd, ctx))) return refusal("guard-files", "any", target.raw);
  }
  for (const text of inlineStrings(c)) {
    if (text && isGuarded(text, c.cwd, ctx)) return refusal("guard-files", "any", text);
  }
  if (!c.program || ctx.rules.guardFiles.readers.includes(c.program)) return null;
  // A value is no change, and of .NET's calls only the file system's touch a file.
  if (c.powershell && /^[.[(]/.test(c.program) && !/^\[(file|directory)\]::/.test(c.program)) return null;
  const binds = c.powershell ? ":" : "=";
  for (const arg of c.args) {
    for (const text of arg.texts) {
      const value = text.startsWith("-") && text.includes(binds) ? text.slice(text.indexOf(binds) + 1) : text;
      if (value && !value.startsWith("-") && isGuarded(value, c.cwd, ctx)) return refusal("guard-files", "any", arg.raw);
    }
  }
  return null;
}

// ---------------------------------------------------------------------------
// Every tool
// ---------------------------------------------------------------------------

function encodedCommand(c) {
  return POWERSHELLS.has(c.program) && powershellScript(c.args).encoded !== undefined ? refusal("encoded-command", "any") : null;
}

/** `irm x | Out-File i.ps1`: saved by what the download is piped into. */
function savedFromPipe(c, all, ctx) {
  if (!["out-file", "set-content", "tee-object"].includes(c.cmdlet)) return [];
  const downloaded = all.some((other) => other.pipeline === c.pipeline && other.seq < c.seq && ctx.rules.downloadToShell.downloaders.includes(other.program));
  if (!downloaded) return [];
  return bind(c).files.filter((arg) => arg.exact).map((arg) => paths.normalize(arg.text, c.cwd, ctx.platform)).filter(Boolean);
}

function checkCommand(command, ctx, dialect) {
  const { rules } = ctx;
  const all = flatten(command, ctx, dialect);
  const downloads = [];
  for (const c of all) {
    const refused =
      (on(rules.encodedCommand) && encodedCommand(c)) ||
      (on(rules.recursiveDelete) && recursiveDelete(c, all, ctx)) ||
      (on(rules.forcePush) && forcePush(c, ctx)) ||
      (on(rules.gitResetHard) && gitResetHard(c, ctx)) ||
      (on(rules.gitClean) && gitClean(c, ctx)) ||
      (on(rules.dropDatabase) && dropDatabase(c, all, ctx)) ||
      (on(rules.downloadToShell) && downloadToShell(c, all, downloads, ctx)) ||
      (on(rules.secrets) && secretFile(c, all, ctx)) ||
      (on(rules.guardFiles) && guardFiles(c, ctx));
    if (refused) return refused;
    if (on(rules.downloadToShell) && rules.downloadToShell.downloaders.includes(c.program)) downloads.push(...downloadedFiles(c, ctx));
    if (on(rules.downloadToShell)) downloads.push(...savedFromPipe(c, all, ctx));
  }
  return null;
}

function checkFileTool(tool, file, ctx) {
  if (typeof file !== "string" || !file) return null;
  const { rules, cwd } = ctx;
  if (tool !== "Read" && on(rules.guardFiles) && isGuarded(file, cwd, ctx)) return refusal("guard-files", "any", file);
  if (on(rules.secrets) && secretPath(file, false, cwd, ctx)) {
    return refusal("secret-file", tool === "Write" ? "write" : "read", file);
  }
  return null;
}

function checkGrep(input, ctx) {
  if (!on(ctx.rules.secrets)) return null;
  const { path, glob } = input;
  if (typeof path === "string" && path) {
    // A folder of secrets is searched like the files in it.
    const asFile = secretPath(path, false, ctx.cwd, ctx);
    const asFolder = secretPath(`${path.replace(/[\\/]+$/, "")}/local`, false, ctx.cwd, ctx);
    if (asFile || asFolder) return refusal("secret-file", "read", path);
  }
  if (typeof glob === "string" && glob) {
    for (const option of paths.expandBraces(glob)) {
      if (option.startsWith("!")) continue;
      if (secretPath(option, true, ctx.cwd, ctx)) return refusal("secret-file", "read", glob);
    }
  }
  return null;
}

// The tools that send a local file somewhere, and the file each names.
const texts = (...values) => values.flat().filter((value) => typeof value === "string" && value);
const list = (value) => (Array.isArray(value) ? value : []);

function artifactSources(files) {
  if (Array.isArray(files)) return files.map((entry) => entry && entry.path);
  if (files === null || typeof files !== "object") return [];
  return Object.values(files).map((entry) => (typeof entry === "string" ? entry : entry && entry.from));
}

const SENDERS = {
  Artifact: (input) => texts(input.file_path, list(input.file_paths), artifactSources(input.files)),
  ArtifactData: (input) => texts(input.file_path, list(input.writes).map((write) => write && write.file_path)),
  DesignSync: (input) => texts(list(input.files).map((entry) => entry && entry.localPath)),
};

// Where each tool names the file it opens or changes.
const FILE_FIELDS = { Read: "file_path", Edit: "file_path", Write: "file_path", NotebookEdit: "notebook_path", LSP: "filePath" };

/** The local files this call names, for the tools working on files. */
function filesOfCall(tool, input) {
  if (FILE_FIELDS[tool]) return texts(input[FILE_FIELDS[tool]]);
  return SENDERS[tool] ? SENDERS[tool](input) : [];
}

/** The refusal for this PreToolUse event, or null. */
function check(event, ctx) {
  const input = event.tool_input || {};
  const tool = event.tool_name;
  switch (tool) {
    case "Bash":
    case "Monitor": // its command runs in the Bash tool's shell, Git Bash on Windows
      return typeof input.command === "string" ? checkCommand(input.command, ctx, "sh") : null;
    case "PowerShell":
      return typeof input.command === "string" ? checkCommand(input.command, ctx, "powershell") : null;
    case "Grep":
      return checkGrep(input, ctx);
    case "Read":
    case "LSP":
    case "Edit":
    case "Write":
    case "NotebookEdit":
    case "Artifact":
    case "ArtifactData":
    case "DesignSync": {
      // An LSP call or an upload reads the file, like Read. A notebook edit changes it, like Write.
      const as = tool === "NotebookEdit" ? "Write" : tool === "Edit" || tool === "Write" ? tool : "Read";
      for (const file of filesOfCall(tool, input)) {
        const refused = checkFileTool(as, file, ctx);
        if (refused) return refused;
      }
      return null;
    }
    default:
      return null;
  }
}

module.exports = { check, filesOfCall, WHY };
