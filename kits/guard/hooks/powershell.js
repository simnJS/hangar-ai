"use strict";
// Reads a PowerShell command line into the tree shell.js makes of a shell's, running
// nothing. It has a reader of its own because little carries over: the backtick escapes,
// a backslash is a path's, quotes double themselves, `&` calls, `{…}` holds code, commas
// make lists, and a statement can start with a value. Malformed input never throws: an
// unterminated string simply runs to the end of the text.
//
// It adds to shell.js's tree:
//   a command whose first word is VALUE: a statement that is a value, its operands as words;
//   a command `[file]::readalltext` or `.downloadstring`: a .NET call, its arguments as words;
//   a part { type: "list", words }: `a,b`, one argument standing for each;
//   a part { type: "block", nodes }: `{ … }`, read when the command holding it is;
// and a part { type: "var", ps: true, env?, member? } lowers its name, as PowerShell's
// names have no case. `member` is what `$x.member` or `(…).member` asks of it.
//
// Every text part counts as unquoted, for what the rules ask of "unquoted": in PowerShell
// the cmdlet expands `*` and `~` itself, inside quotes as well.

const { VALUE } = require("./cmdlets.js");

const MAX_DEPTH = 64;
const SINGLE_QUOTES = "'\u2018\u2019\u201a\u201b";
const DOUBLE_QUOTES = '"\u201c\u201d\u201e';
const DASHES = "\u2013\u2014\u2015";
const ESCAPES = { 0: "\0", a: "\x07", b: "\b", e: "\x1b", f: "\f", n: "\n", r: "\r", t: "\t", v: "\v" };
const ASSIGNMENT = /^(?:\[[^\]\n]*\]\s*)*\$(?:\{([^}\n]+)\}|((?:[A-Za-z_]\w*:)?[A-Za-z_]\w*))\s*(=|\+=|-=|\*=|\/=|%=)(?!=)/;
const METHOD = /(?:\[([^\]]*)\]\s*::\s*|\.)([A-Za-z_]\w*)$/;
const SCOPES = new Set(["global", "script", "local", "private", "using"]);
const LITERALS = new Set(["true", "false", "null"]);

// Keywords opening a compound statement, with what must follow for it to be one: `foreach`
// and `where` without a `(` are the aliases of ForEach-Object and Where-Object.
const COMPOUND = new Map(Object.entries({
  if: "(", elseif: "(", while: "(", until: "(", for: "(", foreach: "(", switch: "(-", param: "(",
  else: "{", do: "{", try: "{", finally: "{", begin: "{", process: "{", end: "{", clean: "{", dynamicparam: "{", data: "{",
  catch: "[{", trap: "[{",
}));
const LEADING = new Set(["return", "throw", "exit", "break", "continue"]);

const isBlank = (c) => c !== undefined && /[ \t\r\f\v\u00a0\u2000-\u200a\u202f\u205f\u3000\ufeff]/.test(c);
const newCommand = (words = []) => ({ kind: "command", assigns: [], words, redirects: [], heredocs: [], herestrings: [] });
const textWord = (value, raw = value) => ({ parts: [{ type: "text", value, quoted: false }], raw });
const subPart = (nodes) => ({ type: "sub", nodes, quoted: false });
const isValue = (node) => node.kind === "command" && node.words.length > 0 && node.words[0].raw === "" && node.words[0].parts[0].value === VALUE;

function addText(parts, value) {
  const last = parts[parts.length - 1];
  if (last && last.type === "text") last.value += value;
  else parts.push({ type: "text", value, quoted: false });
}

function varPart(raw) {
  const scoped = /^([A-Za-z_]\w*):([\s\S]*)$/.exec(raw);
  if (!scoped) return { type: "var", name: raw.toLowerCase(), ps: true, quoted: false };
  const scope = scoped[1].toLowerCase();
  if (scope === "env") return { type: "var", name: scoped[2], ps: true, env: true, quoted: false };
  // `$script:x` is x; `${C:\x}` and `$function:x` are a provider's, whose value is not read.
  return { type: "var", name: SCOPES.has(scope) ? scoped[2].toLowerCase() : null, ps: true, quoted: false };
}

class Reader {
  constructor(text, depth = 0) {
    this.s = text;
    this.i = 0;
    this.depth = depth;
  }

  deeper(read) {
    if (this.depth >= MAX_DEPTH) throw new Error("command nested too deep to read");
    this.depth++;
    const value = read();
    this.depth--;
    return value;
  }

  nested(closer) {
    return this.deeper(() => this.statements(closer));
  }

  /** Spaces, comments, and a backtick ending a line. Stops at a newline. */
  skipBlanks() {
    const { s } = this;
    while (this.i < s.length) {
      const c = s[this.i];
      const next = s[this.i + 1];
      if (isBlank(c)) {
        this.i++;
      } else if (c === "`" && next === "\n") {
        this.i += 2;
      } else if (c === "`" && next === "\r" && s[this.i + 2] === "\n") {
        this.i += 3;
      } else if (c === "<" && next === "#") {
        const stop = s.indexOf("#>", this.i + 2);
        this.i = stop < 0 ? s.length : stop + 2;
      } else if (c === "#") {
        while (this.i < s.length && s[this.i] !== "\n") this.i++;
      } else {
        return;
      }
    }
  }

  skipLines() {
    for (;;) {
      this.skipBlanks();
      if (this.s[this.i] !== "\n") return;
      this.i++;
    }
  }

  /** Reads statements up to `closer` (`)`, `}`) or the end of the text. */
  statements(closer) {
    const { s } = this;
    const nodes = [];
    let piped = false;
    while (this.i < s.length) {
      this.skipBlanks();
      if (this.i >= s.length) break;
      const c = s[this.i];
      const next = s[this.i + 1];
      if (c === "\n") {
        this.i++; // a pipe may still follow, on the next line
      } else if (c === ";") {
        this.i++;
        piped = false;
      } else if (c === closer) {
        this.i++;
        return nodes;
      } else if (c === ")" || c === "}") {
        this.i++;
      } else if (c === "|") {
        piped = next !== "|";
        this.i += next === "|" ? 2 : 1;
      } else if (c === "&" && next === "&") {
        piped = false;
        this.i += 2;
      } else {
        const start = this.i;
        for (const node of this.statement(piped)) {
          node.piped = piped;
          piped = false;
          nodes.push(node);
        }
        if (this.i === start) this.i++; // never stuck
      }
    }
    return nodes;
  }

  atCommandEnd() {
    const c = this.s[this.i];
    return c === undefined || c === "\n" || c === ";" || c === "|" || c === ")" || c === "}" || c === "&";
  }

  /** One statement: the nodes it makes. */
  statement(piped = false) {
    const { s } = this;
    if (!piped) {
      const word = /^[A-Za-z]+/.exec(s.slice(this.i, this.i + 16));
      const keyword = word ? word[0].toLowerCase() : null;
      const after = keyword === null ? undefined : s[this.i + keyword.length];
      if (keyword && (after === undefined || !/[\w\-.\\/:]/.test(after))) {
        if (LEADING.has(keyword)) {
          this.i += keyword.length;
          this.skipBlanks();
          return this.atCommandEnd() ? [] : this.statement();
        }
        if (keyword === "function" || keyword === "filter") {
          this.i += keyword.length;
          this.skipBlanks();
          this.element();
          return this.compoundBody("function");
        }
        if (keyword === "using") {
          while (this.i < s.length && s[this.i] !== "\n") this.i++;
          return [];
        }
        const opener = COMPOUND.get(keyword);
        if (opener !== undefined) {
          let look = this.i + keyword.length;
          while (isBlank(s[look]) || s[look] === "\n") look++;
          if (s[look] !== undefined && opener.includes(s[look])) {
            this.i += keyword.length;
            return keyword === "foreach" ? this.foreachLoop() : this.compoundBody(keyword);
          }
        }
      }
    }
    const assigned = ASSIGNMENT.exec(s.slice(this.i, this.i + 400));
    if (assigned) return [this.assignment(assigned)];
    const node = this.startsValue() ? this.expression() : this.command();
    return node ? [node] : [];
  }

  /** What follows a compound keyword: conditions, then its block. */
  compoundBody(keyword) {
    const { s } = this;
    const nodes = [];
    for (;;) {
      this.skipLines();
      const c = s[this.i];
      if (c === "(") {
        this.i++;
        nodes.push({ kind: "subshell", body: this.nested(")") });
      } else if (c === "{") {
        this.i++;
        nodes.push({ kind: "group", body: this.nested("}") });
        return nodes;
      } else if (c === "[") {
        const stop = s.indexOf("]", this.i);
        this.i = stop < 0 ? s.length : stop + 1;
      } else if (c === ",") {
        this.i++;
      } else if (c === "-" && keyword === "switch") {
        this.element();
      } else {
        return nodes;
      }
    }
  }

  /** `foreach ($x in a, b) { … }`: the values, then the block. */
  foreachLoop() {
    const { s } = this;
    this.skipLines();
    this.i++; // the `(` looked at before coming here
    this.skipLines();
    const variable = /^\$([A-Za-z_]\w*)\s+in\b/i.exec(s.slice(this.i, this.i + 200));
    if (!variable) return [{ kind: "subshell", body: this.nested(")") }, ...this.compoundBody("foreach")];
    this.i += variable[0].length;
    const start = this.i;
    const inside = this.nested(")");
    const atoms = inside.length === 1 && isValue(inside[0]) ? inside[0].words.slice(1) : null;
    const values = atoms && atoms.length ? atoms : [{ parts: [subPart(inside)], raw: s.slice(start, this.i - 1) }];
    return [{ kind: "for", name: variable[1].toLowerCase(), values }, ...this.compoundBody("foreach")];
  }

  /** `$x = …`: a value, or what the commands on its right run. */
  assignment(match) {
    const { s } = this;
    this.i += match[0].length;
    const part = varPart(match[1] || match[2]);
    const name = part.env ? `env:${part.name.toLowerCase()}` : part.name === null ? "(provider)" : part.name;
    this.skipLines();
    const start = this.i;
    const nodes = this.deeper(() => this.rightHandSide());
    const raw = s.slice(start, this.i).trim();
    let value;
    if (nodes.length === 1 && isValue(nodes[0]) && nodes[0].words.length === 2 && !nodes[0].redirects.length) [, value] = nodes[0].words;
    else value = { parts: [subPart(nodes)], raw };
    // `$x += …` keeps a value the text does not say.
    if (match[3] !== "=") value = { parts: [{ type: "var", name: null, ps: true, quoted: false }, subPart(nodes)], raw };
    return { ...newCommand(), assigns: [{ name, value }] };
  }

  /** A pipeline, up to what ends a statement. */
  rightHandSide() {
    const { s } = this;
    const nodes = [];
    let piped = false;
    for (;;) {
      for (const node of this.statement(piped)) {
        node.piped = piped;
        piped = false;
        nodes.push(node);
      }
      this.skipBlanks();
      if (s[this.i] !== "|" || s[this.i + 1] === "|") return nodes;
      this.i++;
      this.skipLines();
      piped = true;
    }
  }

  /** Whether the statement here starts as a value rather than as a command's name. */
  startsValue() {
    const { s } = this;
    const c = s[this.i];
    const next = s[this.i + 1];
    if (c === "$" || c === "(" || c === "[" || c === "@" || c === "{" || c === "," || c === "!" || c === "+") return true;
    if (SINGLE_QUOTES.includes(c) || DOUBLE_QUOTES.includes(c) || DASHES.includes(c) || c === "-") return true;
    if (/\d/.test(c)) return true;
    return c === "." && next !== undefined && /\d/.test(next);
  }

  expression() {
    const node = newCommand([{ parts: [{ type: "text", value: VALUE, quoted: false }], raw: "" }]);
    this.arguments(node, false);
    return node;
  }

  command() {
    const { s } = this;
    const node = newCommand();
    const c = s[this.i];
    // The call operator, and dot-sourcing: the words after them are the command as ever.
    if (c === "&" || (c === "." && (s[this.i + 1] === undefined || /[\s"'$({]/.test(s[this.i + 1])))) this.i++;
    this.arguments(node, true);
    return node.words.length || node.redirects.length ? node : null;
  }

  arguments(node, isCommand) {
    const { s } = this;
    for (;;) {
      this.skipBlanks();
      if (this.atCommandEnd()) return;
      if (this.redirect(node)) continue;
      if (isCommand && node.words.length && s.startsWith("--%", this.i) && (this.i + 3 >= s.length || /\s/.test(s[this.i + 3]))) {
        this.stopParsing(node);
        return;
      }
      const start = this.i;
      const word = this.word();
      if (this.i === start) {
        this.i++; // never stuck
        continue;
      }
      if (word.parts.length) node.words.push(word);
    }
  }

  redirect(node) {
    const match = /^(\*|\d)?(>>|>)(&\d)?/.exec(this.s.slice(this.i, this.i + 6));
    if (!match) return false;
    this.i += match[0].length;
    const fd = match[1] && match[1] !== "*" ? match[1] : null;
    if (match[3]) {
      node.redirects.push({ op: ">&", fd, target: textWord(match[3].slice(1)) });
      return true;
    }
    this.skipBlanks();
    const target = this.atCommandEnd() ? null : this.element();
    const op = match[1] === "*" && match[2] === ">" ? "&>" : match[2];
    node.redirects.push({ op, fd, target });
    return true;
  }

  /** After `--%`, the line is handed over as it stands, `%X%` expanded. */
  stopParsing(node) {
    const { s } = this;
    this.i += 3;
    const start = this.i;
    while (this.i < s.length && s[this.i] !== "\n" && s[this.i] !== "|") this.i++;
    for (const piece of s.slice(start, this.i).split(/\s+/).filter(Boolean)) {
      const parts = [];
      for (const bit of piece.replace(/^"(.*)"$/, "$1").split(/(%[A-Za-z_]\w*%)/)) {
        if (/^%[A-Za-z_]\w*%$/.test(bit)) parts.push({ type: "var", name: bit.slice(1, -1), ps: true, env: true, quoted: false });
        else if (bit) addText(parts, bit);
      }
      node.words.push({ parts, raw: piece });
    }
  }

  /** One argument: an element, or a comma's list of them, spaces allowed around it. */
  word() {
    const { s } = this;
    const start = this.i;
    const first = this.element();
    let look = this.i;
    while (isBlank(s[look])) look++;
    if (s[look] !== ",") return first;
    const words = first.parts.length ? [first] : [];
    while (s[look] === ",") {
      this.i = look + 1;
      this.skipLines();
      if (this.atCommandEnd()) break;
      const element = this.element();
      if (element.parts.length) words.push(element);
      look = this.i;
      while (isBlank(s[look])) look++;
    }
    return { parts: [{ type: "list", words, quoted: false }], raw: s.slice(start, this.i) };
  }

  element() {
    const { s } = this;
    const start = this.i;
    const parts = [];
    while (this.i < s.length) {
      const c = s[this.i];
      const next = s[this.i + 1];
      if (isBlank(c) || "\n;|,)}&><".includes(c)) break;
      if (c === "`") {
        if (next === "\n" || (next === "\r" && s[this.i + 2] === "\n")) break;
        addText(parts, ESCAPES[next] === undefined ? next || "" : ESCAPES[next]);
        this.i = Math.min(this.i + 2, s.length);
      } else if (SINGLE_QUOTES.includes(c)) {
        this.single(parts);
      } else if (DOUBLE_QUOTES.includes(c)) {
        this.double(parts);
      } else if (c === "$") {
        this.dollar(parts, true);
      } else if (c === "@" && next !== undefined && (next === "(" || next === "{" || SINGLE_QUOTES.includes(next) || DOUBLE_QUOTES.includes(next))) {
        this.at(parts);
      } else if (c === "(") {
        const called = METHOD.exec(s.slice(start, this.i));
        if (called) {
          this.methodCall(parts, called);
        } else if (parts.length) {
          break; // `a(b)` is two arguments
        } else {
          this.i++;
          const part = subPart(this.nested(")"));
          parts.push(part);
          this.member(part);
        }
      } else if (c === "{" && this.i === start) {
        this.i++;
        parts.push({ type: "block", nodes: this.nested("}"), quoted: false });
      } else if (DASHES.includes(c) && this.i === start) {
        addText(parts, "-"); // PowerShell takes a typographic dash for a parameter's
        this.i++;
      } else {
        addText(parts, c);
        this.i++;
      }
    }
    return { parts, raw: s.slice(start, this.i) };
  }

  single(parts) {
    const { s } = this;
    this.i++;
    let value = "";
    while (this.i < s.length) {
      const c = s[this.i];
      if (!SINGLE_QUOTES.includes(c)) {
        value += c;
        this.i++;
      } else if (s[this.i + 1] !== undefined && SINGLE_QUOTES.includes(s[this.i + 1])) {
        value += "'";
        this.i += 2;
      } else {
        this.i++;
        break;
      }
    }
    addText(parts, value);
  }

  double(parts) {
    this.i++;
    this.expandable(parts, true);
  }

  /** The inside of double quotes, up to the closing one when `closes`. */
  expandable(parts, closes) {
    const { s } = this;
    addText(parts, "");
    while (this.i < s.length) {
      const c = s[this.i];
      const next = s[this.i + 1];
      if (closes && DOUBLE_QUOTES.includes(c)) {
        if (next !== undefined && DOUBLE_QUOTES.includes(next)) {
          addText(parts, '"');
          this.i += 2;
          continue;
        }
        this.i++;
        return;
      }
      if (c === "`") {
        addText(parts, ESCAPES[next] === undefined ? next || "" : ESCAPES[next]);
        this.i = Math.min(this.i + 2, s.length);
      } else if (c === "$") {
        this.dollar(parts, false);
      } else {
        addText(parts, c);
        this.i++;
      }
    }
  }

  /** `$…`. `bare`: outside quotes, where `.member` belongs to it. */
  dollar(parts, bare) {
    const { s } = this;
    const next = s[this.i + 1];
    let part;
    if (next === "(") {
      this.i += 2;
      part = subPart(this.nested(")"));
    } else if (next === "{") {
      const stop = s.indexOf("}", this.i + 2);
      part = varPart(s.slice(this.i + 2, stop < 0 ? s.length : stop));
      this.i = stop < 0 ? s.length : stop + 1;
    } else {
      const name = /^(?:[A-Za-z_]\w*:)?[A-Za-z_]\w*/.exec(s.slice(this.i + 1, this.i + 200));
      if (name) {
        this.i += 1 + name[0].length;
        if (LITERALS.has(name[0].toLowerCase())) {
          addText(parts, `$${name[0].toLowerCase()}`);
          return;
        }
        part = varPart(name[0]);
      } else if (next && "?$^".includes(next)) {
        this.i += 2;
        part = { type: "var", name: null, ps: true, quoted: false };
      } else {
        addText(parts, "$");
        this.i++;
        return;
      }
    }
    parts.push(part);
    if (bare) this.member(part);
  }

  /** `.a.b` after a value. The last name is left when a `(` makes it a call. */
  member(part) {
    const { s } = this;
    const match = /^(?:\.[A-Za-z_]\w*)+/.exec(s.slice(this.i, this.i + 200));
    if (!match) return;
    let chain = match[0];
    if (s[this.i + chain.length] === "(") chain = chain.slice(0, chain.lastIndexOf("."));
    if (!chain) return;
    part.member = chain.slice(1).toLowerCase();
    this.i += chain.length;
  }

  /** `@(…)`, `@{…}`, and here-strings. */
  at(parts) {
    const { s } = this;
    const next = s[this.i + 1];
    if (next === "(") {
      this.i += 2;
      const part = subPart(this.nested(")"));
      parts.push(part);
      this.member(part);
      return;
    }
    if (next === "{") {
      this.i += 2;
      parts.push(subPart(this.deeper(() => this.hashtable())));
      return;
    }
    const single = SINGLE_QUOTES.includes(next);
    const header = /^[ \t]*\r?\n/.exec(s.slice(this.i + 2, this.i + 200));
    if (!header) {
      addText(parts, "@");
      this.i++;
      return;
    }
    const from = this.i + 2 + header[0].length - 1; // its newline: the closing one may be this one
    const closing = (single ? /\n['\u2018\u2019\u201a\u201b]@/ : /\n["\u201c\u201d\u201e]@/).exec(s.slice(from));
    const stop = closing ? from + closing.index : s.length;
    const body = s.slice(from + 1, Math.max(stop, from + 1));
    if (single) addText(parts, body);
    else new Reader(body, this.depth + 1).expandable(parts, false);
    this.i = closing ? stop + closing[0].length : s.length;
  }

  /** `@{ key = value; … }`: keys are data, and each value is read like a right-hand side. */
  hashtable() {
    const { s } = this;
    const nodes = [];
    for (;;) {
      this.skipLines();
      const c = s[this.i];
      if (c === undefined) return nodes;
      if (c === "}") {
        this.i++;
        return nodes;
      }
      if (c === ";") {
        this.i++;
        continue;
      }
      const key = /^[^=;\n}]*=/.exec(s.slice(this.i, this.i + 400));
      if (!key) {
        this.i++;
        continue;
      }
      this.i += key[0].length;
      this.skipLines();
      nodes.push(...this.rightHandSide());
    }
  }

  /** `[Type]::Method(…)` or `.Method(…)`: a command named after it, its arguments as words. */
  methodCall(parts, called) {
    const program = called[1] === undefined ? `.${called[2]}` : `[${called[1].split(".").pop()}]::${called[2]}`;
    this.i++;
    const inside = this.nested(")");
    let args = [];
    if (inside.length === 1 && isValue(inside[0])) {
      args = inside[0].words.slice(1);
      if (args.length === 1 && args[0].parts.length === 1 && args[0].parts[0].type === "list") args = args[0].parts[0].words;
    } else if (inside.length) {
      args = [{ parts: [subPart(inside)], raw: "" }];
    }
    const before = parts.filter((part) => part.type !== "text");
    parts.length = 0;
    parts.push(...before, subPart([newCommand([textWord(program.toLowerCase()), ...args])]));
  }
}

function parsePowerShell(text) {
  return new Reader(String(text)).statements(null);
}

module.exports = { parsePowerShell };
