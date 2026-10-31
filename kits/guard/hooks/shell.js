"use strict";
// Reads a shell command line into a tree of commands, running nothing. It knows
// quotes, escapes, chaining, pipes, subshells, groups, here-documents, redirects
// and substitutions: enough to tell `rm -rf /` from `echo "rm -rf /"`, and to find
// `rm -rf /` inside `echo "$(rm -rf /)"`. Malformed input never throws: an
// unterminated quote simply runs to the end of the text.
//
// A node is one of:
//   { kind: "command", assigns, words, redirects, heredocs, herestrings, piped }
//   { kind: "subshell" | "group", body: [nodes], piped }
//   { kind: "for", name, values: [words], piped }
// A word is { parts, raw }; a part is { type: "text", value, quoted },
// { type: "var", name, fallback?, quoted } or { type: "sub", nodes, quoted }.

const MAX_DEPTH = 64;
const WORD_ENDS = " \t\r\n;&|()";
const SKIPPED = new Set(["if", "then", "elif", "else", "fi", "do", "done", "while", "until", "!", "esac"]);
const REDIRECTS = ["&>>", "&>", "<<<", "<<-", "<<", "<>", "<&", ">>", ">&", ">|", "<", ">"];

function addText(parts, value, quoted) {
  const last = parts[parts.length - 1];
  if (last && last.type === "text" && last.quoted === quoted) last.value += value;
  else parts.push({ type: "text", value, quoted });
}

/** The word's text when it holds nothing but unquoted text, else null. */
function plainText(word) {
  if (word.parts.length !== 1) return null;
  const [part] = word.parts;
  return part.type === "text" && !part.quoted ? part.value : null;
}

/** The word's text when it holds no expansion, quoted or not, else null. */
function literalText(word) {
  return word.parts.every((part) => part.type === "text") ? word.parts.map((part) => part.value).join("") : null;
}

class Reader {
  /** `windows`: the text is for cmd or PowerShell, where a backslash separates paths. */
  constructor(text, { windows = false, depth = 0 } = {}) {
    this.s = text;
    this.i = 0;
    this.windows = windows;
    this.depth = depth;
    this.pending = []; // here-documents whose body starts at the next newline
  }

  nested(closer) {
    if (this.depth >= MAX_DEPTH) throw new Error("command nested too deep to read");
    this.depth++;
    const nodes = this.list(closer);
    this.depth--;
    return nodes;
  }

  /** Reads commands up to `closer` (`)`, `}`) or the end of the text. */
  list(closer) {
    const { s } = this;
    const nodes = [];
    let cmd = null;
    let pipeNext = false;
    const push = (node) => {
      node.piped = pipeNext;
      pipeNext = false;
      nodes.push(node);
    };
    const end = () => {
      if (cmd) push(cmd);
      cmd = null;
    };
    const current = () => {
      if (!cmd) cmd = { kind: "command", assigns: [], words: [], redirects: [], heredocs: [], herestrings: [] };
      return cmd;
    };

    while (this.i < s.length) {
      const c = s[this.i];
      const next = s[this.i + 1];
      if (c === " " || c === "\t" || c === "\r") {
        this.i++;
      } else if (c === "\\" && next === "\n" && !this.windows) {
        this.i += 2;
      } else if (c === "\\" && next === "\r" && s[this.i + 2] === "\n" && !this.windows) {
        this.i += 3;
      } else if (c === "\n") {
        end();
        this.i++;
        this.readHeredocs();
      } else if (c === "#") {
        while (this.i < s.length && s[this.i] !== "\n") this.i++;
      } else if (c === ";") {
        end();
        pipeNext = false;
        this.i += next === ";" || next === "&" ? 2 : 1;
        if (s[this.i] === "&") this.i++; // ;;&
      } else if (c === "&" && next === ">") {
        this.redirect(current(), null);
      } else if (c === "&") {
        end();
        pipeNext = false;
        this.i += next === "&" ? 2 : 1;
      } else if (c === "|") {
        end();
        if (next === "|") {
          pipeNext = false;
          this.i += 2;
        } else {
          pipeNext = true;
          this.i += next === "&" ? 2 : 1;
        }
      } else if (c === "(") {
        end();
        this.i++;
        push({ kind: "subshell", body: this.nested(")") });
      } else if (c === ")") {
        end();
        this.i++;
        if (closer === ")") return nodes;
        // Otherwise a `case` pattern, or a stray one: it only ends the command.
      } else if ((c === "<" || c === ">") && next !== "(") {
        this.redirect(current(), null);
      } else {
        const start = this.i;
        const word = this.word();
        if (this.i === start) {
          this.i++; // never stuck
          continue;
        }
        const after = s[this.i];
        if ((after === "<" || after === ">") && /^\d+$/.test(word.raw)) {
          this.redirect(current(), word.raw);
          continue;
        }
        if (!cmd || !cmd.words.length) {
          const plain = plainText(word);
          if (plain !== null) {
            if (SKIPPED.has(plain)) {
              end();
              continue;
            }
            if (plain === "{") {
              end();
              push({ kind: "group", body: this.nested("}") });
              continue;
            }
            if (plain === "}") {
              end();
              if (closer === "}") return nodes;
              continue;
            }
            if (plain === "for" || plain === "select") {
              end();
              const loop = this.forLoop();
              if (loop) push(loop);
              continue;
            }
            if (plain === "function") {
              end();
              this.skipBlanks();
              this.word();
              continue;
            }
          }
          const assign = this.assignment(word);
          if (assign) {
            current().assigns.push(assign);
            if (s[this.i] === "(") {
              // An array: its elements are data, not a subshell to run.
              this.i++;
              this.nested(")");
            }
            continue;
          }
        }
        current().words.push(word);
      }
    }
    end();
    return nodes;
  }

  skipBlanks() {
    while (this.s[this.i] === " " || this.s[this.i] === "\t") this.i++;
  }

  assignment(word) {
    const [first] = word.parts;
    if (!first || first.type !== "text" || first.quoted) return null;
    const match = /^([A-Za-z_][A-Za-z0-9_]*)\+?=/.exec(first.value);
    if (!match) return null;
    const rest = first.value.slice(match[0].length);
    const parts = [...(rest ? [{ type: "text", value: rest, quoted: false }] : []), ...word.parts.slice(1)];
    return { name: match[1], value: { parts, raw: word.raw.slice(match[0].length) } };
  }

  forLoop() {
    this.skipBlanks();
    if (this.s[this.i] === "(") return null; // for ((…)): read on as ordinary text
    const name = literalText(this.word());
    this.skipBlanks();
    const values = [];
    const start = this.i;
    const keyword = this.word();
    if (plainText(keyword) !== "in") {
      this.i = start;
      return null;
    }
    for (;;) {
      this.skipBlanks();
      const c = this.s[this.i];
      if (c === undefined || ";\n&|()".includes(c) || c === "\r") break;
      const before = this.i;
      values.push(this.word());
      if (this.i === before) break;
    }
    return name ? { kind: "for", name, values } : null;
  }

  redirect(cmd, fd) {
    const op = REDIRECTS.find((candidate) => this.s.startsWith(candidate, this.i));
    this.i += op.length;
    this.skipBlanks();
    const c = this.s[this.i];
    const target = c === undefined || WORD_ENDS.includes(c) ? null : this.word();
    if (op === "<<" || op === "<<-") {
      const quoted = !target || target.parts.some((part) => part.type !== "text" || part.quoted);
      const delimiter = target ? target.parts.map((part) => part.value || "").join("") : "";
      const doc = { delimiter, quoted, stripTabs: op === "<<-", text: "", word: null };
      cmd.heredocs.push(doc);
      this.pending.push(doc);
    } else if (op === "<<<") {
      if (target) cmd.herestrings.push(target);
    } else {
      cmd.redirects.push({ op, fd, target });
    }
  }

  readHeredocs() {
    const { s } = this;
    for (const doc of this.pending) {
      const lines = [];
      while (this.i < s.length) {
        let stop = s.indexOf("\n", this.i);
        if (stop < 0) stop = s.length;
        const line = s.slice(this.i, stop).replace(/\r$/, "");
        this.i = Math.min(stop + 1, s.length);
        const seen = doc.stripTabs ? line.replace(/^\t+/, "") : line;
        if (seen === doc.delimiter) break;
        lines.push(seen);
      }
      doc.text = lines.join("\n");
      // An unquoted delimiter leaves substitutions in the body running.
      if (!doc.quoted) doc.word = new Reader(doc.text, { windows: this.windows, depth: this.depth + 1 }).expandable();
    }
    this.pending = [];
  }

  /** The whole text, read as the inside of double quotes. */
  expandable() {
    const parts = [];
    const { s } = this;
    while (this.i < s.length) {
      const c = s[this.i];
      if (c === "\\" && "$`\\".includes(s[this.i + 1] || " ")) {
        addText(parts, s[this.i + 1], true);
        this.i += 2;
      } else if (c === "$") {
        this.dollar(parts, true);
      } else if (c === "`") {
        this.backtick(parts, true);
      } else {
        addText(parts, c, true);
        this.i++;
      }
    }
    return { parts, raw: s };
  }

  word() {
    const { s } = this;
    const start = this.i;
    const parts = [];
    while (this.i < s.length) {
      const c = s[this.i];
      if (WORD_ENDS.includes(c)) break;
      const next = s[this.i + 1];
      if (c === "<" || c === ">") {
        if (next !== "(") break;
        this.i += 2;
        parts.push({ type: "sub", nodes: this.nested(")"), quoted: false });
      } else if (c === "\\" && !this.windows) {
        if (next === "\n") {
          this.i += 2;
        } else {
          addText(parts, next === undefined ? "" : next, true);
          this.i += 2;
        }
      } else if (c === "'") {
        let stop = s.indexOf("'", this.i + 1);
        if (stop < 0) stop = s.length;
        addText(parts, s.slice(this.i + 1, stop), true);
        this.i = stop + 1;
      } else if (c === '"') {
        this.double(parts);
      } else if (c === "$") {
        this.dollar(parts, false);
      } else if (c === "`") {
        this.backtick(parts, false);
      } else if (c === "%" && this.windows && /^%[A-Za-z_][A-Za-z0-9_]*%/.test(s.slice(this.i, this.i + 66))) {
        const stop = s.indexOf("%", this.i + 1);
        parts.push({ type: "var", name: s.slice(this.i + 1, stop), quoted: false });
        this.i = stop + 1;
      } else {
        addText(parts, c, false);
        this.i++;
      }
    }
    return { parts, raw: s.slice(start, this.i) };
  }

  double(parts) {
    const { s } = this;
    addText(parts, "", true);
    this.i++;
    while (this.i < s.length) {
      const c = s[this.i];
      if (c === '"') {
        this.i++;
        return;
      }
      if (c === "\\" && !this.windows) {
        const next = s[this.i + 1];
        if (next === "\n") {
          this.i += 2;
        } else if (next === "$" || next === "`" || next === '"' || next === "\\") {
          addText(parts, next, true);
          this.i += 2;
        } else {
          addText(parts, "\\", true);
          this.i++;
        }
      } else if (c === "$") {
        this.dollar(parts, true);
      } else if (c === "`") {
        this.backtick(parts, true);
      } else {
        addText(parts, c, true);
        this.i++;
      }
    }
  }

  dollar(parts, quoted) {
    const { s } = this;
    const next = s[this.i + 1];
    if (next === "(") {
      if (s[this.i + 2] === "(") {
        const stop = this.arithmeticEnd();
        if (stop > 0) {
          parts.push({ type: "var", name: null, quoted });
          this.i = stop;
          return;
        }
      }
      this.i += 2;
      parts.push({ type: "sub", nodes: this.nested(")"), quoted });
      return;
    }
    if (next === "{") {
      let stop = this.i + 2;
      for (let level = 1; stop < s.length && level; stop++) {
        if (s[stop] === "{") level++;
        else if (s[stop] === "}") level--;
      }
      const inner = s.slice(this.i + 2, stop - 1);
      this.i = stop;
      const fallback = /^([A-Za-z_][A-Za-z0-9_]*):?[-=]([\s\S]*)$/.exec(inner);
      if (fallback) {
        parts.push({ type: "var", name: fallback[1], fallback: fallback[2], quoted });
        // `${X:-$(a)}` runs `a` when X is unset.
        const inside = new Reader(fallback[2], { windows: this.windows, depth: this.depth + 1 }).expandable();
        parts.push(...inside.parts.filter((part) => part.type === "sub"));
      } else parts.push({ type: "var", name: /^[A-Za-z_][A-Za-z0-9_]*$/.test(inner) ? inner : null, quoted });
      return;
    }
    if (next === "'" && !quoted) {
      this.ansiC(parts);
      return;
    }
    if (next === '"' && !quoted) {
      this.i++;
      this.double(parts);
      return;
    }
    const tail = s.slice(this.i + 1, this.i + 130);
    const env = this.windows && /^env:([A-Za-z_][A-Za-z0-9_]*)/i.exec(tail);
    if (env) {
      parts.push({ type: "var", name: env[1], quoted });
      this.i += 1 + env[0].length;
      return;
    }
    const name = /^[A-Za-z_][A-Za-z0-9_]*/.exec(tail);
    if (name) {
      parts.push({ type: "var", name: name[0], quoted });
      this.i += 1 + name[0].length;
      return;
    }
    if (next && "@*#?$!-0123456789".includes(next)) {
      parts.push({ type: "var", name: null, quoted });
      this.i += 2;
      return;
    }
    addText(parts, "$", quoted);
    this.i++;
  }

  /** Where `$((…))` ends, or -1 when this is `$( (…) … )` after all. */
  arithmeticEnd() {
    const { s } = this;
    let level = 0;
    for (let j = this.i + 1; j < s.length; j++) {
      if (s[j] === "(") level++;
      else if (s[j] === ")" && --level === 0) return s[j - 1] === ")" ? j + 1 : -1;
    }
    return -1;
  }

  ansiC(parts) {
    const { s } = this;
    const escapes = { n: "\n", t: "\t", r: "\r", a: "\x07", b: "\b", e: "\x1b", f: "\f", v: "\v", "\\": "\\", "'": "'", '"': '"' };
    this.i += 2;
    let value = "";
    while (this.i < s.length && s[this.i] !== "'") {
      if (s[this.i] !== "\\") {
        value += s[this.i++];
        continue;
      }
      const rest = s.slice(this.i + 1, this.i + 5);
      const hex = /^x([0-9a-fA-F]{1,2})/.exec(rest);
      const octal = /^([0-7]{1,3})/.exec(rest);
      if (hex) {
        value += String.fromCharCode(parseInt(hex[1], 16));
        this.i += 1 + hex[0].length;
      } else if (octal) {
        value += String.fromCharCode(parseInt(octal[1], 8));
        this.i += 1 + octal[0].length;
      } else {
        value += escapes[rest[0]] === undefined ? `\\${rest[0] || ""}` : escapes[rest[0]];
        this.i += 2;
      }
    }
    this.i++;
    addText(parts, value, true);
  }

  backtick(parts, quoted) {
    const { s } = this;
    let j = this.i + 1;
    let body = "";
    while (j < s.length && s[j] !== "`") {
      if (s[j] === "\\" && "`$\\".includes(s[j + 1] || " ")) {
        body += s[j + 1];
        j += 2;
      } else {
        body += s[j++];
      }
    }
    this.i = j + 1;
    if (this.depth >= MAX_DEPTH) throw new Error("command nested too deep to read");
    const reader = new Reader(body, { windows: this.windows, depth: this.depth + 1 });
    const nodes = reader.list(null);
    reader.readHeredocs();
    parts.push({ type: "sub", nodes, quoted });
  }
}

function parse(text, options) {
  const reader = new Reader(String(text), options);
  const nodes = reader.list(null);
  reader.readHeredocs();
  return nodes;
}

module.exports = { parse, plainText, literalText };
