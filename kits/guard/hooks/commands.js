"use strict";
// Turns a command line into the flat list of what it would run, in order: chained
// commands, what `bash -c`, `eval`, `sudo`, `find -exec`, `pwsh -c` or `iex` hold, what
// substitutions and script blocks run. Each comes with its arguments as far as they can be
// read, following `cd` and simple variables along the way.
//
// Three languages come through here, and one can hold another: "sh", "cmd" and "powershell".
//
// An argument is { text, texts, exact, glob, raw, commands, blocks }:
//   texts     what it can stand for (`{a,b}`, `a,b`, a `for` variable), `text` the first;
//   exact     false when some of it cannot be known; UNKNOWN marks where;
//   glob      it holds an unquoted `*`, `?` or `[`;
//   commands  what its substitutions run;
//   blocks    the script blocks it holds, read once the command is known.
//
// A command in the block of a ForEach-Object or Where-Object has that command as `owner`:
// it reads the pipeline from there, and what comes after does not read from it.

const { parse, literalText } = require("./shell.js");
const { parsePowerShell } = require("./powershell.js");
const { VALUE, cmdletOf, bind } = require("./cmdlets.js");
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

// What `$x.member` can be told from `$x`: the path it holds, as far as the rules ask.
const PATH_MEMBERS = new Set(["path", "fullname", "providerpath"]);

/** PowerShell's names have no case, and only `$env:X` reaches the environment. */
function lookupPowerShell(part, scope, ctx) {
  const { name, env, member } = part;
  if (member && !PATH_MEMBERS.has(member)) return null;
  if (env) {
    const lower = name.toLowerCase();
    if (scope.vars.has(`env:${lower}`)) return scope.vars.get(`env:${lower}`);
    const found = Object.keys(ctx.env).find((key) => key.toLowerCase() === lower);
    return found && typeof ctx.env[found] === "string" && ctx.env[found] ? [ctx.env[found]] : null;
  }
  if (scope.vars.has(name)) return scope.vars.get(name);
  if (name === "pwd") return scope.cwd === null ? null : [scope.cwd];
  if (name === "home") return [ctx.home];
  return null;
}

function lookup(part, scope, ctx) {
  const { name } = part;
  if (!name) return null;
  if (part.ps) return lookupPowerShell(part, scope, ctx);
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

/** What a PowerShell substitution stands for, when the text alone says. */
function knownPowerShell(nodes, scope, state, ctx) {
  if (nodes.length !== 1 || nodes[0].kind !== "command" || !nodes[0].words.length) return null;
  const { words } = nodes[0];
  const head = literalText(words[0]);
  if (head === null) return null;
  // Read for their value only: what they run was counted when the substitution was walked.
  const quietly = (word) => {
    const mark = state.out.length;
    const value = resolve(word, scope, state, ctx, "powershell");
    state.out.length = mark;
    return value;
  };
  const plain = words.slice(1).filter((word) => !(literalText(word) || "").startsWith("-")).map(quietly);
  const joined = () => (plain.length >= 2 ? plain[0].texts.map((text) => `${text}/${plain[1].text}`) : null);
  if (head === VALUE) return words.length === 2 ? quietly(words[1]).texts : null;
  if (head === "[path]::gettemppath") return [ctx.tmp];
  if (head === "[path]::combine") return joined();
  const cmdlet = cmdletOf(programName(head));
  if (cmdlet === "get-location") return scope.cwd === null ? null : [scope.cwd];
  if (cmdlet === "join-path") return joined();
  if (cmdlet === "resolve-path" || cmdlet === "convert-path") return plain.length ? plain[0].texts : null;
  return null;
}

function resolve(word, scope, state, ctx, dialect) {
  const powershell = dialect === "powershell";
  let texts = [""];
  let exact = true;
  let glob = false;
  let braces = false;
  const commands = [];
  const blocks = [];
  word.parts.forEach((part, index) => {
    let values;
    if (part.type === "text") {
      let { value } = part;
      if (!part.quoted) {
        if (index === 0 && /^~(\/|\\|$)/.test(value)) value = ctx.home + value.slice(1);
        else if (index === 0 && value.startsWith("~") && !powershell) value = `${ctx.home}/../${value.slice(1)}`;
        if (/[*?[]/.test(value)) glob = true;
        if (!powershell && /\{[^{}]*,[^{}]*\}/.test(value)) braces = true;
      }
      values = [value];
    } else if (part.type === "block") {
      blocks.push(part.nodes);
      values = [UNKNOWN];
      exact = false;
    } else if (part.type === "list") {
      const items = part.words.map((item) => resolve(item, scope, state, ctx, dialect));
      values = items.length ? items.flatMap((item) => item.texts) : [""];
      if (items.some((item) => !item.exact)) exact = false;
      if (items.some((item) => item.glob)) glob = true;
      for (const item of items) {
        commands.push(...item.commands);
        blocks.push(...item.blocks);
      }
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
      walk(part.nodes, copyScope(scope), state, ctx, null, dialect);
      commands.push(...state.out.slice(before));
      values = powershell ? knownPowerShell(part.nodes, scope, state, ctx) : knownSubstitution(part.nodes, scope, ctx);
      if (part.member && !PATH_MEMBERS.has(part.member)) values = null;
      if (!values) {
        values = [UNKNOWN];
        exact = false;
      } else if (values.some((value) => value.includes(UNKNOWN))) {
        exact = false;
      }
    }
    const next = [];
    for (const start of texts) for (const value of values) if (next.length < 64) next.push(start + value);
    texts = next;
  });
  if (braces) texts = texts.flatMap(paths.expandBraces).slice(0, 64);
  return { text: texts[0], texts, exact, glob, raw: word.raw, commands, blocks };
}

function walk(nodes, scope, state, ctx, forced, dialect) {
  for (const node of nodes) {
    const pipeline = forced !== null ? forced : node.piped ? state.last : ++state.pipelines;
    state.last = pipeline;
    if (node.kind === "for") {
      const values = node.values.map((word) => resolve(word, scope, state, ctx, dialect));
      scope.vars.set(node.name, values.length ? values.flatMap((value) => value.texts) : [UNKNOWN]);
    } else if (node.kind === "subshell") {
      walk(node.body, copyScope(scope), state, ctx, pipeline, dialect);
    } else if (node.kind === "group") {
      walk(node.body, scope, state, ctx, pipeline, dialect);
    } else {
      command(node, scope, state, ctx, pipeline, dialect);
    }
    state.last = pipeline;
  }
}

function command(node, scope, state, ctx, pipeline, dialect) {
  const words = node.words.map((word) => resolve(word, scope, state, ctx, dialect));
  const redirects = node.redirects.map((item) => ({
    op: item.op,
    fd: item.fd,
    target: item.target ? resolve(item.target, scope, state, ctx, dialect) : null,
  }));
  let stdin = null;
  for (const doc of node.heredocs) {
    if (doc.word) resolve(doc.word, scope, state, ctx, dialect);
    stdin = (stdin === null ? "" : `${stdin}\n`) + doc.text;
  }
  for (const word of node.herestrings) {
    const value = resolve(word, scope, state, ctx, dialect);
    stdin = (stdin === null ? "" : `${stdin}\n`) + value.text;
  }
  for (const { name, value } of node.assigns) {
    const resolved = resolve(value, scope, state, ctx, dialect);
    if (!words.length) scope.vars.set(name, resolved.texts);
    runBlocks(resolved, null, scope, state, ctx, dialect);
  }
  const info = { pipeline, dialect, windows: dialect !== "sh", powershell: dialect === "powershell", stdin, redirects, xargs: false, fromFind: null };
  if (words.length) record(words, info, scope, state, ctx);
  else if (redirects.length) state.out.push(makeCommand("", "", [], info, scope, state));
}

// Their block runs on each item of the pipeline: what is in it reads from that pipeline.
const PIPELINE_BLOCKS = new Set(["foreach-object", "where-object"]);

/** A script block is read as if it ran: nothing on the line says it will not. */
function runBlocks(word, c, scope, state, ctx, dialect) {
  const inPipeline = c !== null && PIPELINE_BLOCKS.has(c.cmdlet);
  for (const nodes of word.blocks || []) {
    const outer = state.owner;
    state.owner = inPipeline ? c : null;
    walk(nodes, copyScope(scope), state, ctx, inPipeline ? c.pipeline : null, dialect);
    state.owner = outer;
  }
}

function makeCommand(program, path, args, info, scope, state) {
  return { program, path, args, cwd: scope.cwd, seq: state.out.length, owner: state.owner, ...info };
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
  // `rm.exe` is the program, whatever `rm` alone would be an alias of.
  c.cmdlet = info.powershell && !/\.(exe|cmd|bat|com)$/i.test(head.text) ? cmdletOf(program) : null;
  state.out.push(c);
  for (const word of words) runBlocks(word, c, scope, state, ctx, info.dialect);

  if (SHELLS.has(program)) {
    const script = shellScript(args);
    if (script.code) for (const text of script.code.texts.slice(0, 8)) reparse(text, scope, state, ctx, "sh", c.pipeline);
    if (script.stdin) readStdin(c, "sh", scope, state, ctx);
  } else if (program === "eval") {
    reparse(args.map((arg) => arg.text).join(" "), scope, state, ctx, info.dialect, c.pipeline, true);
  } else if (program === "cmd") {
    const flag = args.findIndex((arg) => /^\/+[ck]$/i.test(arg.text));
    if (flag >= 0) reparse(args.slice(flag + 1).map((arg) => arg.text).join(" "), scope, state, ctx, "cmd", c.pipeline);
  } else if (program === "powershell" || program === "pwsh") {
    const script = powershellScript(args);
    // Refused as unreadable by its own rule. Read all the same, for a project that turned it off.
    const code = script.encoded === undefined ? script.code : Buffer.from(script.encoded, "base64").toString("utf16le");
    if (code) reparse(code, scope, state, ctx, "powershell", c.pipeline);
    if (script.stdin) readStdin(c, "powershell", scope, state, ctx);
  } else if (c.cmdlet === "invoke-expression" || CODE_CALLS.has(program)) {
    const bound = bind(c);
    const code = bound.named("command")[0] || bound.plain[0];
    if (code) for (const text of code.texts.slice(0, 8)) reparse(text, scope, state, ctx, "powershell", c.pipeline, true);
  } else if (c.cmdlet === "start-process") {
    const bound = bind(c);
    const [file] = bound.files;
    const list = bound.named("argumentlist")[0] || bound.named("args")[0] || bound.plain[1];
    if (file) reparse([file.text, ...(list ? list.texts : [])].join(" "), scope, state, ctx, "powershell", c.pipeline);
  } else if (c.cmdlet === "set-location" || c.cmdlet === "push-location" || c.cmdlet === "pop-location") {
    const name = { "set-location": "cd", "push-location": "pushd", "pop-location": "popd" }[c.cmdlet];
    changeDirectory(name, bind(c).files.slice(0, 1), scope, ctx);
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

function readerFor(text, dialect) {
  return dialect === "powershell" ? parsePowerShell(text) : parse(text, { windows: dialect === "cmd" });
}

function reparse(text, scope, state, ctx, dialect, pipeline, sameScope = false) {
  if (state.depth >= MAX_REPARSE) throw new Error("command nested too deep to read");
  state.depth++;
  walk(readerFor(text, dialect), sameScope ? scope : copyScope(scope), state, ctx, pipeline, dialect);
  state.depth--;
  state.last = pipeline;
}

// Calls that run the text they are given.
const CODE_CALLS = new Set(["[scriptblock]::create", ".invokescript"]);

/** A shell reading its code from stdin: a here-document, or what is piped from a plain value. */
function readStdin(c, dialect, scope, state, ctx) {
  if (c.stdin !== null) reparse(c.stdin, scope, state, ctx, dialect, c.pipeline);
  const feed = state.out.filter((other) => other.pipeline === c.pipeline && other.seq < c.seq && (["echo", "printf", VALUE].includes(other.program) || other.cmdlet === "write-output"));
  for (const other of feed) reparse(other.args.map((arg) => arg.text).join(" "), scope, state, ctx, dialect, c.pipeline);
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

// powershell.exe and pwsh take any start of an option's name, after `-`, `--` or `/`.
const POWERSHELL_VALUE_OPTIONS = ["executionpolicy", "ep", "inputformat", "outputformat", "windowstyle", "configurationname", "configurationfile", "workingdirectory", "wd", "custompipename", "psconsolefile", "settingsfile", "version"];

/** What PowerShell was given to run: { code, file? }, { file }, { encoded }, or { stdin: true }. */
function powershellScript(args) {
  for (let i = 0; i < args.length; i++) {
    const { text } = args[i];
    const option = /^(?:--?|\/)([A-Za-z]+)(?::[\s\S]*)?$/.exec(text);
    if (option) {
      const name = option[1].toLowerCase();
      if (name === "ec" || "encodedcommand".startsWith(name)) return { encoded: args[i + 1] ? args[i + 1].text : "" };
      if ("command".startsWith(name)) {
        const rest = args.slice(i + 1);
        return !rest.length || rest[0].text === "-" ? { stdin: true } : { code: rest.map((arg) => arg.text).join(" ") };
      }
      if ("file".startsWith(name)) return !args[i + 1] || args[i + 1].text === "-" ? { stdin: true } : { file: args[i + 1] };
      if (POWERSHELL_VALUE_OPTIONS.some((known) => known === name || (name.length >= 2 && known.startsWith(name)))) i++;
      continue;
    }
    if (text === "-") return { stdin: true };
    if (text.startsWith("-")) continue;
    return { code: args.slice(i).map((arg) => arg.text).join(" "), file: args[i] };
  }
  return { stdin: true };
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

/** What the command line would run, in order. `dialect`: the language it is written in. */
function flatten(text, ctx, dialect = "sh") {
  const state = { out: [], pipelines: 0, last: 0, depth: 0, owner: null };
  const scope = { cwd: ctx.cwd, vars: new Map(), dirs: [] };
  walk(readerFor(text, dialect), scope, state, ctx, null, dialect);
  return state.out;
}

module.exports = { flatten, shellScript, powershellScript, programName, UNKNOWN, SHELLS };
