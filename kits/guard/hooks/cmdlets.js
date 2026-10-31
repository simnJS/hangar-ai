"use strict";
// What PowerShell's own commands take: their aliases, which parameters name a file and
// which hold another value, and how an argument binds to them. `-Rec`, `-Path:x` and a
// plain word mean what PowerShell would make of them, so `-Encoding .env` is no file and
// `Select-String .env notes.md` searches notes.md.

// The word a statement that is a value starts with, `'dist' | Remove-Item` for one: it
// stands where a program's name would, for the commands that read what comes down a pipe.
const VALUE = "(value)";

const ALIASES = new Map(Object.entries({
  ri: "remove-item", rm: "remove-item", del: "remove-item", erase: "remove-item", rd: "remove-item", rmdir: "remove-item",
  gc: "get-content", cat: "get-content", type: "get-content",
  sls: "select-string",
  gci: "get-childitem", ls: "get-childitem", dir: "get-childitem",
  gi: "get-item",
  cp: "copy-item", copy: "copy-item", cpi: "copy-item",
  mv: "move-item", move: "move-item", mi: "move-item",
  ren: "rename-item", rni: "rename-item",
  sc: "set-content", ac: "add-content", clc: "clear-content", ni: "new-item",
  tee: "tee-object",
  iex: "invoke-expression", icm: "invoke-command", saps: "start-process", start: "start-process",
  cd: "set-location", chdir: "set-location", sl: "set-location", pushd: "push-location", popd: "pop-location",
  pwd: "get-location", gl: "get-location", rvpa: "resolve-path", cvpa: "convert-path",
  "%": "foreach-object", foreach: "foreach-object", "?": "where-object", where: "where-object",
  select: "select-object", sort: "sort-object",
  echo: "write-output", write: "write-output",
  fhx: "format-hex", ipcsv: "import-csv",
}));

const COMMON_VALUES = ["erroraction", "ea", "warningaction", "wa", "informationaction", "infa", "errorvariable", "ev", "warningvariable", "wv", "informationvariable", "iv", "outvariable", "ov", "outbuffer", "ob", "pipelinevariable", "pv"];
const COMMON_SWITCHES = ["verbose", "vb", "debug", "db", "whatif", "wi", "confirm", "cf"];

const PATHS = ["path", "literalpath", "pspath", "lp"];
const FILE_PATHS = ["filepath", "literalpath", "pspath", "lp"];
const CONTENT_VALUES = ["value", "encoding", "filter", "include", "exclude", "credential", "stream"];
const CONTENT_SWITCHES = ["force", "passthru", "nonewline", "asbytestream"];
const COPY_VALUES = ["destination", "filter", "include", "exclude", "credential", "tosession", "fromsession"];
const COPY_SWITCHES = ["recurse", "force", "container", "passthru"];

// `files`: parameters naming a file. `values`: the others that take a value. `switches`:
// those that take none, named so that a short form like `-f` can be told ambiguous.
// `pattern`: the first plain word is a pattern. `single`: only the first is a file.
const CMDLETS = new Map(Object.entries({
  "remove-item": { files: PATHS, values: ["filter", "include", "exclude", "credential", "stream"], switches: ["recurse", "force"] },
  "get-childitem": { files: PATHS, values: ["filter", "include", "exclude", "depth", "attributes"], switches: ["recurse", "force", "name", "directory", "file", "hidden", "readonly", "system", "followsymlink"] },
  "get-item": { files: PATHS, values: ["filter", "include", "exclude", "credential", "stream"], switches: ["force"] },
  "get-content": { files: PATHS, values: ["totalcount", "first", "head", "tail", "last", "readcount", "delimiter", "encoding", "filter", "include", "exclude", "credential", "stream"], switches: ["raw", "wait", "force", "asbytestream"] },
  "select-string": { files: PATHS, values: ["pattern", "encoding", "context", "include", "exclude", "inputobject", "culture"], switches: ["allmatches", "casesensitive", "list", "noemphasis", "notmatch", "quiet", "raw", "simplematch"], pattern: true },
  "format-hex": { files: PATHS, values: ["encoding", "count", "offset", "inputobject"], switches: ["raw"], single: true },
  "import-csv": { files: PATHS, values: ["delimiter", "encoding", "header"], switches: ["useculture"], single: true },
  "compress-archive": { files: PATHS, values: ["destinationpath", "compressionlevel"], switches: ["force", "update", "passthru"], single: true },
  "set-content": { files: PATHS, values: CONTENT_VALUES, switches: CONTENT_SWITCHES, single: true },
  "add-content": { files: PATHS, values: CONTENT_VALUES, switches: CONTENT_SWITCHES, single: true },
  "clear-content": { files: PATHS, values: ["filter", "include", "exclude", "credential", "stream"], switches: ["force"] },
  "new-item": { files: PATHS, values: ["itemtype", "type", "name", "value", "credential"], switches: ["force"], single: true },
  "out-file": { files: FILE_PATHS, values: ["encoding", "width", "inputobject"], switches: ["append", "force", "noclobber", "nonewline"], single: true },
  "tee-object": { files: FILE_PATHS, values: ["variable", "inputobject", "encoding"], switches: ["append"], single: true },
  "copy-item": { files: PATHS, values: COPY_VALUES, switches: COPY_SWITCHES, single: true },
  "move-item": { files: PATHS, values: COPY_VALUES, switches: COPY_SWITCHES, single: true },
  "rename-item": { files: PATHS, values: ["newname", "credential"], switches: ["force", "passthru"], single: true },
  "set-location": { files: PATHS, values: ["stackname"], switches: ["passthru"], single: true },
  "push-location": { files: PATHS, values: ["stackname"], switches: ["passthru"], single: true },
  "invoke-expression": { files: [], values: ["command"], switches: [] },
  "start-process": { files: ["filepath"], values: ["argumentlist", "args", "workingdirectory", "wd", "verb", "windowstyle", "credential", "redirectstandardinput", "redirectstandardoutput", "redirectstandarderror", "environment"], switches: ["wait", "passthru", "nonewwindow", "usenewenvironment", "loaduserprofile"], single: true },
}));

/** The cmdlet a program's name stands for in PowerShell, or null. */
function cmdletOf(program) {
  return ALIASES.get(program) || (String(program).includes("-") ? program : null);
}

/** What a command's arguments bind to. */
function bind(c) {
  const spec = CMDLETS.get(c.cmdlet) || { files: [], values: [], switches: [] };
  const known = [...spec.files, ...spec.values, ...spec.switches, ...COMMON_VALUES, ...COMMON_SWITCHES];
  const flags = new Set([...spec.switches, ...COMMON_SWITCHES]);
  const named = new Map();
  const switches = new Map();
  const plain = [];
  const add = (name, arg) => named.set(name, [...(named.get(name) || []), arg]);

  for (let i = 0; i < c.args.length; i++) {
    const arg = c.args[i];
    const match = /^-([A-Za-z?][\w-]*)(:[\s\S]*)?$/.exec(arg.text);
    if (!match) {
      plain.push(arg);
      continue;
    }
    const typed = match[1].toLowerCase();
    const candidates = known.includes(typed) ? [typed] : known.filter((name) => name.startsWith(typed));
    // One PowerShell would call ambiguous or unknown takes no value: what follows is still read.
    const name = candidates.length === 1 ? candidates[0] : null;
    if (name === null) {
      switches.set(typed, true);
    } else if (flags.has(name)) {
      switches.set(name, !/^:\$false$/i.test(match[2] || ""));
    } else if (match[2] !== undefined) {
      const prefix = arg.text.slice(0, arg.text.length - match[2].length + 1);
      const texts = arg.texts.map((text) => (text.startsWith(prefix) ? text.slice(prefix.length) : text));
      add(name, { ...arg, text: texts[0], texts });
    } else if (c.args[i + 1]) {
      add(name, c.args[++i]);
    }
  }

  let positional = plain;
  if (spec.pattern && !named.has("pattern")) positional = positional.slice(1);
  if (spec.single) positional = positional.slice(0, 1);
  return {
    plain,
    named: (name) => named.get(name) || [],
    has: (name) => named.has(name),
    on: (name) => switches.get(name) === true,
    /** The arguments naming the files this command works on. */
    files: [...spec.files.flatMap((name) => named.get(name) || []), ...positional],
  };
}

module.exports = { VALUE, cmdletOf, bind };
