"use strict";
// Turns a command line into the flat list of what it would run, in order: chained
// commands, what `bash -c`, `eval`, `sudo` or `find -exec` hold, what substitutions
// run. Each comes with its arguments as far as they can be read, following `cd` and
// simple variables along the way.
//
// An argument is { text, texts, exact, glob, raw, commands }:
//   texts     what it can stand for (`{a,b}`, a `for` variable), `text` the first;
//   exact     false when some of it cannot be known; UNKNOWN marks where;
//   glob      it holds an unquoted `*`, `?` or `[`;
//   commands  what its substitutions run.

const { parse, literalText } = require("./shell.js");
const paths = require("./paths.js");

const UNKNOWN = "\u0001";
const MAX_REPARSE = 16;

const SHELLS = new Set(["sh", "bash", "zsh", "dash", "ksh", "ash", "fish"]);

// Programs that run the rest of their arguments: their options, what takes a value,
// and how many arguments come before the program.
const WRAPPERS = {
  sudo: { value: ["-u", "-g", "-h", "-p", "-C", "-T", "-r", "-t", "-U", "-D", "--user", "--group", "--host", "--prompt", "--chdir"] },
  doas: { value: ["-u", "-C"] },
  env: { value: ["-u", "-C", "--unset", "--chdir"], assigns: true },
  command: { lookup: ["-v", "-V"] },
  builtin: {},
  exec: { value: ["-a"] },
  nohup: {},
  setsid: {},
  time: { value: ["-f", "-o"] },
  nice: { value: ["-n", "--adjustment"] },
  ionice: { value: ["-c", "-n"] },
  stdbuf: { value: ["-i", "-o", "-e"] },
  timeout: { value: ["-k", "-s", "--kill-after", "--signal"], positionals: 1 },
  busybox: {},
  xargs: { value: ["-I", "-n", "-P", "-L", "-d", "-E", "-s", "-a", "--max-args", "--max-procs", "--replace", "--delimiter", "--arg-file"] },
  npx: { value: ["-p", "--package"] },
  pnpx: { value: ["-p", "--package"] },
  bunx: { value: ["-p", "--package"] },
};
const DECLARES = new Set(["export", "declare", "local", "readonly", "typeset"]);
const RUNNERS = new Set(["pnpm", "yarn", "npm", "bun"]);
const RUNNER_VERBS = new Set(["dlx", "exec", "x"]);

function programName(text) {
  const base = String(text).replace(/\\/g, "/").split("/").pop().toLowerCase().replace(/\.(exe|cmd|bat|ps1)$/, "");
  if (/^python[\d.]*$/.test(base) || base === "py") return "python";
  return base;
}

const copyScope = (scope) => ({ cwd: scope.cwd, vars: new Map(scope.vars), dirs: [...scope.dirs] });

function lookup(part, scope, ctx) {
  const { name } = part;
  if (!name) return null;
  if (scope.vars.has(name)) return scope.vars.get(name);
  if (name === "PWD") return scope.cwd === null ? null : [scope.cwd];
  if (name === "HOME") return [ctx.home];
  const value = ctx.env[name];
  if (typeof value === "string" && value) return [value];
  if (part.fallback && !/[$`]/.test(part.fallback)) return [part.fallback];
  return null;
}

/** What `$(pwd)` and `$(mktemp …)` stand for, the two whose answer is known. */
function knownSubstitution(nodes, scope, ctx) {
  if (nodes.length !== 1 || nodes[0].kind !== "command" || !nodes[0].words.length) return null;
  const name = literalText(nodes[0].words[0]);
  if (name === "pwd" && scope.cwd !== null) return [scope.cwd];
  if (name === "mktemp") return [`${ctx.tmp}/mktemp.x`];
  return null;
}

function resolve(word, scope, state, ctx, windows) {
  let texts = [""];
  let exact = true;
  let glob = false;
  let braces = false;
  const commands = [];
  word.parts.forEach((part, index) => {
    let values;
    if (part.type === "text") {
      let { value } = part;
      if (!part.quoted) {
        if (index === 0 && /^~(\/|\\|$)/.test(value)) value = ctx.home + value.slice(1);
        else if (index === 0 && value.startsWith("~")) value = `${ctx.home}/../${value.slice(1)}`;
        if (/[*?[]/.test(value)) glob = true;
        if (/\{[^{}]*,[^{}]*\}/.test(value)) braces = true;
      }
      values = [value];
    } else if (part.type === "var") {
      values = lookup(part, scope, ctx);
      if (!values) {
        values = [UNKNOWN];
        exact = false;
      } else if (values.some((value) => value.includes(UNKNOWN))) {
        exact = false;
      }
    } else {
      const before = state.out.length;
      walk(part.nodes, copyScope(scope), state, ctx, null, windows);
      commands.push(...state.out.slice(before));
      values = knownSubstitution(part.nodes, scope, ctx);
      if (!values) {
        values = [UNKNOWN];
        exact = false;
      }
    }
    const next = [];
    for (const start of texts) for (const value of values) if (next.length < 64) next.push(start + value);
    texts = next;
  });
  if (braces) texts = texts.flatMap(paths.expandBraces).slice(0, 64);
  return { text: texts[0], texts, exact, glob, raw: word.raw, commands };
}

function walk(nodes, scope, state, ctx, forced, windows) {
  for (const node of nodes) {
    const pipeline = forced !== null ? forced : node.piped ? state.last : ++state.pipelines;
    state.last = pipeline;
    if (node.kind === "for") {
      const values = node.values.map((word) => resolve(word, scope, state, ctx, windows));
      scope.vars.set(node.name, values.length ? values.flatMap((value) => value.texts) : [UNKNOWN]);
    } else if (node.kind === "subshell") {
      walk(node.body, copyScope(scope), state, ctx, pipeline, windows);
    } else if (node.kind === "group") {
      walk(node.body, scope, state, ctx, pipeline, windows);
    } else {
      command(node, scope, state, ctx, pipeline, windows);
    }
    state.last = pipeline;
  }
}

function command(node, scope, state, ctx, pipeline, windows) {
  const words = node.words.map((word) => resolve(word, scope, state, ctx, windows));
  const redirects = node.redirects.map((item) => ({
    op: item.op,
    fd: item.fd,
    target: item.target ? resolve(item.target, scope, state, ctx, windows) : null,
  }));
  let stdin = null;
  for (const doc of node.heredocs) {
    if (doc.word) resolve(doc.word, scope, state, ctx, windows);
    stdin = (stdin === null ? "" : `${stdin}\n`) + doc.text;
  }
  for (const word of node.herestrings) {
    const value = resolve(word, scope, state, ctx, windows);
    stdin = (stdin === null ? "" : `${stdin}\n`) + value.text;
  }
  for (const { name, value } of node.assigns) {
    const resolved = resolve(value, scope, state, ctx, windows);
    if (!words.length) scope.vars.set(name, resolved.texts);
  }
  const info = { pipeline, windows, stdin, redirects, xargs: false, fromFind: null };
  if (words.length) record(words, info, scope, state, ctx);
  else if (redirects.length) state.out.push(makeCommand("", "", [], info, scope, state));
}

function makeCommand(program, path, args, info, scope, state) {
  return { program, path, args, cwd: scope.cwd, seq: state.out.length, ...info };
}

/** Skips a wrapper's own arguments. The index of the program it runs, or -1. */
function afterWrapper(words, index, wrapper) {
  let i = index + 1;
  let positionals = wrapper.positionals || 0;
  while (i < words.length) {
    const text = words[i].text;
    if (wrapper.lookup && wrapper.lookup.includes(text)) return -1;
    if (text === "--") {
      i++;
      continue;
    }
    if (wrapper.assigns && /^[A-Za-z_][A-Za-z0-9_]*=/.test(text)) {
      i++;
      continue;
    }
    if (text.startsWith("-") && text.length > 1) {
      i += wrapper.value && wrapper.value.includes(text) ? 2 : 1;
      continue;
    }
    if (positionals > 0) {
      positionals--;
      i++;
      continue;
    }
    return i;
  }
  return -1;
}

function record(words, info, scope, state, ctx) {
  let index = 0;
  let xargs = false;
  for (let hop = 0; hop < 8; hop++) {
    const name = programName(words[index].text);
    const wrapper = WRAPPERS[name];
    if (wrapper && Object.prototype.hasOwnProperty.call(WRAPPERS, name)) {
      const next = afterWrapper(words, index, wrapper);
      if (next < 0) break;
      if (name === "xargs") xargs = true;
      index = next;
    } else if (RUNNERS.has(name) && words[index + 1] && RUNNER_VERBS.has(words[index + 1].text)) {
      let next = index + 2;
      while (words[next] && words[next].text.startsWith("-")) next++;
      if (!words[next]) break;
      index = next;
    } else {
      break;
    }
  }
  const head = words[index];
  const args = words.slice(index + 1);
  const program = programName(head.text);
  const c = makeCommand(program, head.text, args, { ...info, xargs: info.xargs || xargs }, scope, state);
  state.out.push(c);

  if (SHELLS.has(program)) {
    const script = shellScript(args);
    if (script.code) for (const text of script.code.texts.slice(0, 8)) reparse(text, scope, state, ctx, false, c.pipeline);
    if (script.stdin) {
      if (c.stdin !== null) reparse(c.stdin, scope, state, ctx, false, c.pipeline);
      const feed = state.out.filter((other) => other.pipeline === c.pipeline && other.seq < c.seq && ["echo", "printf"].includes(other.program));
      for (const other of feed) reparse(other.args.map((arg) => arg.text).join(" "), scope, state, ctx, false, c.pipeline);
    }
  } else if (program === "eval") {
    reparse(args.map((arg) => arg.text).join(" "), scope, state, ctx, info.windows, c.pipeline, true);
  } else if (program === "cmd") {
    const flag = args.findIndex((arg) => /^\/+[ck]$/i.test(arg.text));
    if (flag >= 0) reparse(args.slice(flag + 1).map((arg) => arg.text).join(" "), scope, state, ctx, true, c.pipeline);
  } else if (program === "powershell" || program === "pwsh") {
    const code = powershellCode(args);
    if (code) reparse(code, scope, state, ctx, true, c.pipeline);
  } else if (program === "find") {
    findExec(c, info, scope, state, ctx);
  } else if (DECLARES.has(program)) {
    for (const arg of args) {
      const match = /^([A-Za-z_][A-Za-z0-9_]*)=([\s\S]*)$/.exec(arg.text);
      if (match) scope.vars.set(match[1], arg.texts.map((text) => text.slice(match[1].length + 1)));
    }
  } else if (program === "unset") {
    for (const arg of args) scope.vars.set(arg.text, [UNKNOWN]);
  } else if (["cd", "pushd", "popd", "chdir"].includes(program) || (info.windows && ["set-location", "sl"].includes(program))) {
    changeDirectory(program, args, scope, ctx);
  }
}

function reparse(text, scope, state, ctx, windows, pipeline, sameScope = false) {
  if (state.depth >= MAX_REPARSE) throw new Error("command nested too deep to read");
  state.depth++;
  walk(parse(text, { windows }), sameScope ? scope : copyScope(scope), state, ctx, pipeline, windows);
  state.depth--;
  state.last = pipeline;
}

/** What a shell was given to run: { code: argument }, { file: argument }, or { stdin: true }. */
function shellScript(args) {
  let takesCode = false;
  let stdin = false;
  for (let i = 0; i < args.length; i++) {
    const { text } = args[i];
    if (text === "--") continue;
    if (/^[-+][oO]$/.test(text)) {
      i++;
      continue;
    }
    if (/^-[a-zA-Z]+$/.test(text)) {
      if (text.includes("c")) takesCode = true;
      if (text.includes("s")) stdin = true;
      continue;
    }
    if (text.startsWith("-") || text.startsWith("+")) continue;
    if (takesCode) return { code: args[i] };
    if (stdin) return { stdin: true };
    return { file: args[i] };
  }
  return takesCode ? {} : { stdin: true };
}

function powershellCode(args) {
  for (let i = 0; i < args.length; i++) {
    const { text } = args[i];
    if (/^-(e|ec|en|enc|enco|encod|encode|encoded|encodedc|encodedcommand)$/i.test(text) && args[i + 1]) {
      return Buffer.from(args[i + 1].text, "base64").toString("utf16le");
    }
    if (/^-(c|co|com|comm|comma|comman|command)$/i.test(text)) return args.slice(i + 1).map((arg) => arg.text).join(" ");
    if (/^-(f|fi|fil|file)$/i.test(text)) return null;
    if (/^-(executionpolicy|ep|ex|exec|version|inputformat|outputformat|windowstyle|w|configurationname|workingdirectory|wd|custompipename)$/i.test(text)) {
      i++;
      continue;
    }
    if (text.startsWith("-")) continue;
    return args.slice(i).map((arg) => arg.text).join(" ");
  }
  return null;
}

function findExec(c, info, scope, state, ctx) {
  const { args } = c;
  const nameAt = args.findIndex((arg) => /^-i?name$/.test(arg.text));
  const name = nameAt >= 0 && args[nameAt + 1] ? args[nameAt + 1].text : null;
  for (let i = 0; i < args.length; i++) {
    if (!["-exec", "-execdir", "-ok", "-okdir"].includes(args[i].text)) continue;
    const inner = [];
    let j = i + 1;
    for (; j < args.length && args[j].text !== ";" && args[j].text !== "+"; j++) {
      const arg = args[j];
      if (!arg.text.includes("{}")) {
        inner.push(arg);
        continue;
      }
      const fill = (text) => (name === null ? text : text.replace("{}", name));
      inner.push({ ...arg, text: fill(arg.text), texts: arg.texts.map(fill), glob: arg.glob || (name !== null && /[*?[]/.test(name)), found: true });
    }
    if (inner.length) record(inner, { ...info, fromFind: c }, copyScope(scope), state, ctx);
    i = j;
  }
}

function changeDirectory(program, args, scope, ctx) {
  if (program === "popd") {
    if (scope.dirs.length) scope.cwd = scope.dirs.pop();
    return;
  }
  const target = args.find((arg) => !/^-[LPe@]+$/.test(arg.text) && arg.text !== "--" && !/^\/d$/i.test(arg.text) && !/^-(path|literalpath)$/i.test(arg.text));
  if (program === "pushd") scope.dirs.push(scope.cwd);
  if (!target) {
    scope.cwd = ctx.home;
  } else if (!target.exact || target.text === "-") {
    scope.cwd = null;
  } else {
    scope.cwd = paths.normalize(target.text, scope.cwd, ctx.platform);
  }
}

/** What the command line would run, in order. */
function flatten(text, ctx) {
  const state = { out: [], pipelines: 0, last: 0, depth: 0 };
  const scope = { cwd: ctx.cwd, vars: new Map(), dirs: [] };
  walk(parse(text), scope, state, ctx, null, false);
  return state.out;
}

module.exports = { flatten, shellScript, programName, UNKNOWN, SHELLS };
