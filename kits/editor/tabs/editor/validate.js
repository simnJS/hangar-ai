"use strict";
// Is this file broken? JSON is checked by JSON.parse, so "valid" means valid. TOML
// is parsed for its syntax, and YAML is only scanned for what is certainly wrong;
// both answer `partial: true`, and the page says it is a basic check. An error
// gives a code the page translates, and a line when one can be told.
(function (root, factory) {
  const api = factory();
  if (typeof module === "object" && module.exports) module.exports = api;
  else root.EditorValidate = api;
})(typeof globalThis !== "undefined" ? globalThis : this, function () {
  const baseName = (path) => path.slice(path.lastIndexOf("/") + 1).toLowerCase();

  /** Which check applies: json, jsonc, yaml, toml, or null for none. */
  function checkerFor(path) {
    const name = baseName(path);
    if (/\.jsonc$|\.code-workspace$/.test(name)) return "jsonc";
    if (/\.json$/.test(name)) {
      // These are read with comments and trailing commas by what uses them.
      if (/^(tsconfig|jsconfig)([.-].*)?\.json$|^\.eslintrc\.json$|^devcontainer\.json$|^\.devcontainer\.json$/.test(name)) return "jsonc";
      if (/(^|\/)\.vscode\//i.test(path) || /(^|\/)\.devcontainer\//i.test(path)) return "jsonc";
      return "json";
    }
    if (/\.ya?ml$/.test(name)) return "yaml";
    if (/\.toml$/.test(name)) return "toml";
    return null;
  }

  function lineOf(text, at) {
    let line = 1;
    let start = 0;
    const limit = Math.min(at, text.length);
    for (let i = 0; i < limit; i++) {
      if (text[i] === "\n") {
        line++;
        start = i + 1;
      }
    }
    return { line, column: limit - start + 1 };
  }

  /* ── JSON ─────────────────────────────────────────────────────── */

  /** Comments and trailing commas blanked, every other character where it was. */
  function blankJsonc(text) {
    const out = text.split("");
    const blank = (from, to) => {
      for (let i = from; i < to; i++) if (out[i] !== "\n" && out[i] !== "\r") out[i] = " ";
    };
    let lastComma = -1;
    for (let i = 0; i < text.length; i++) {
      const char = text[i];
      if (char === '"') {
        for (i++; i < text.length && text[i] !== '"'; i++) if (text[i] === "\\") i++;
        lastComma = -1;
      } else if (char === "/" && text[i + 1] === "/") {
        let end = i;
        while (end < text.length && text[end] !== "\n") end++;
        blank(i, end);
        i = end - 1;
      } else if (char === "/" && text[i + 1] === "*") {
        const close = text.indexOf("*/", i + 2);
        const end = close < 0 ? text.length : close + 2;
        blank(i, end);
        i = end - 1;
      } else if (char === ",") {
        lastComma = i;
      } else if (char === "]" || char === "}") {
        if (lastComma >= 0) out[lastComma] = " ";
        lastComma = -1;
      } else if (!/\s/.test(char)) {
        lastComma = -1;
      }
    }
    return out.join("");
  }

  function checkJson(text, kind) {
    const source = kind === "jsonc" ? blankJsonc(text) : text;
    try {
      JSON.parse(source);
      return { kind, ok: true, partial: false };
    } catch (error) {
      const message = String((error && error.message) || "");
      const place = /\(line (\d+) column (\d+)\)/.exec(message);
      const position = /position (\d+)/.exec(message);
      let where = {};
      if (place) where = { line: Number(place[1]), column: Number(place[2]) };
      else if (position) where = lineOf(source, Number(position[1]));
      else if (/end of JSON/i.test(message)) where = lineOf(source, source.length);
      return { kind, ok: false, partial: false, code: "json", ...where };
    }
  }

  /* ── YAML: only what is certainly wrong ───────────────────────── */

  function checkYaml(text) {
    const lines = text.split(/\r\n?|\n/);
    const closes = { "[": "]", "{": "}" };
    const flow = [];
    let quote = null;
    let blockIndent = null;
    let plainIndent = null; // the indent of the line that ended in a plain scalar, which may go on below
    const fail = (code, line, column) => ({ kind: "yaml", ok: false, partial: true, code, line, column });

    for (let n = 0; n < lines.length; n++) {
      const line = lines[n];
      let at = 0;
      let start = true; // a token may start here: a quote or a bracket means something

      if (!quote && !flow.length) {
        const lead = /^[ \t]*/.exec(line)[0];
        const rest = line.slice(lead.length);
        const indent = lead.length;
        if (rest === "") continue;
        if (blockIndent !== null) {
          if (indent > blockIndent) continue;
          blockIndent = null;
        }
        if (rest[0] === "#") continue;
        if (plainIndent !== null && indent > plainIndent) continue;
        plainIndent = null;
        if (lead.includes("\t")) return fail("yaml-tab", n + 1, lead.indexOf("\t") + 1);
        if (line[0] === "%") continue;
        at = indent;
        if (/^(---|\.\.\.)([ \t]|$)/.test(rest)) at += 3;
      }

      let plain = false;
      const lineIndent = /^ */.exec(line)[0].length;
      for (; at < line.length; at++) {
        const char = line[at];
        if (quote) {
          if (quote.char === '"' && char === "\\") at++;
          else if (char === quote.char) {
            if (quote.char === "'" && line[at + 1] === "'") at++;
            else quote = null;
          }
          plain = false;
          start = false;
          continue;
        }
        const previous = line[at - 1];
        const next = line[at + 1];
        const blankNext = next === undefined || next === " " || next === "\t";
        if (char === " " || char === "\t") continue;
        if (char === "#" && (previous === undefined || previous === " " || previous === "\t")) break;
        if (start) {
          if (char === '"' || char === "'") {
            quote = { char, line: n + 1, column: at + 1 };
            start = false;
            continue;
          }
          if (char === "[" || char === "{") {
            flow.push({ char, line: n + 1, column: at + 1 });
            continue;
          }
          if ((char === "]" || char === "}") && !flow.length) return fail("yaml-bracket", n + 1, at + 1);
          if (!flow.length && (char === "|" || char === ">") && /^[|>][-+0-9]*[ \t]*(#.*)?$/.test(line.slice(at))) {
            blockIndent = lineIndent;
            plain = false;
            break;
          }
          if ((char === "&" || char === "!") && !blankNext) {
            while (at + 1 < line.length && line[at + 1] !== " " && line[at + 1] !== "\t") at++;
            continue;
          }
          if ((char === "-" || char === "?" || char === ":") && blankNext) continue;
          if (char === "@" || char === "`") return fail("yaml-reserved", n + 1, at + 1);
          start = false;
          plain = true;
        }
        if (char === ":" && (blankNext || (flow.length && ",[]{}".includes(next)))) {
          start = true;
          plain = false;
        } else if (flow.length) {
          if (char === ",") {
            start = true;
            plain = false;
          } else if (char === ":" && previous === '"') {
            start = true;
            plain = false;
          } else if (char === "]" || char === "}") {
            const open = flow.pop();
            if (closes[open.char] !== char) return fail("yaml-bracket", n + 1, at + 1);
            plain = false;
          }
        }
      }
      plainIndent = plain && !quote && !flow.length ? lineIndent : null;
    }
    if (quote) return fail("yaml-quote", quote.line, quote.column);
    if (flow.length) {
      const open = flow[flow.length - 1];
      return fail("yaml-bracket", open.line, open.column);
    }
    return { kind: "yaml", ok: true, partial: true };
  }

  /* ── TOML: its syntax, and keys or tables given twice ─────────── */

  const NUMBER_OR_DATE = new RegExp(
    [
      "[+-]?(?:inf|nan)",
      "0x[0-9A-Fa-f](?:_?[0-9A-Fa-f])*",
      "0o[0-7](?:_?[0-7])*",
      "0b[01](?:_?[01])*",
      "\\d{4}-\\d{2}-\\d{2}(?:[Tt ]\\d{2}:\\d{2}:\\d{2}(?:\\.\\d+)?(?:[Zz]|[+-]\\d{2}:\\d{2})?)?",
      "\\d{2}:\\d{2}:\\d{2}(?:\\.\\d+)?",
      "[+-]?(?:0|[1-9](?:_?\\d)*)(?:\\.\\d(?:_?\\d)*)?(?:[eE][+-]?\\d(?:_?\\d)*)?",
    ].join("|"),
    "y",
  );

  function checkToml(text) {
    let i = 0;
    const stop = (code, at = i) => {
      throw { toml: code, at };
    };
    const blanks = () => {
      while (text[i] === " " || text[i] === "\t") i++;
    };
    const comment = () => {
      if (text[i] === "#") while (i < text.length && text[i] !== "\n") i++;
    };
    const atEol = () => i >= text.length || text[i] === "\n" || (text[i] === "\r" && text[i + 1] === "\n");
    const endOfLine = () => {
      blanks();
      comment();
      if (!atEol()) stop("toml-trailing");
      i += text[i] === "\r" ? 2 : 1;
    };
    const skipBlankLines = () => {
      for (;;) {
        blanks();
        comment();
        if (i >= text.length || !atEol()) return;
        i += text[i] === "\r" ? 2 : 1;
      }
    };

    function basicString(multi) {
      const open = i;
      i += multi ? 3 : 1;
      for (;;) {
        if (i >= text.length) stop("toml-string", open);
        const char = text[i];
        if (multi && text.startsWith('"""', i)) {
          i += 3;
          while (text[i] === '"' && i - open < text.length) i++;
          return;
        }
        if (!multi && char === '"') {
          i++;
          return;
        }
        if (!multi && (char === "\n" || char === "\r")) stop("toml-string", open);
        if (char === "\\") {
          const next = text[i + 1];
          if ("btnfr\"\\".includes(next || "?")) i += 2;
          else if (next === "u" && /^[0-9A-Fa-f]{4}$/.test(text.slice(i + 2, i + 6))) i += 6;
          else if (next === "U" && /^[0-9A-Fa-f]{8}$/.test(text.slice(i + 2, i + 10))) i += 10;
          else if (multi && /^\\[ \t]*\r?\n/.test(text.slice(i, i + 200))) i += 2;
          else stop("toml-escape");
        } else {
          i++;
        }
      }
    }

    function literalString(multi) {
      const open = i;
      i += multi ? 3 : 1;
      for (;;) {
        if (i >= text.length) stop("toml-string", open);
        if (multi && text.startsWith("'''", i)) {
          i += 3;
          while (text[i] === "'") i++;
          return;
        }
        if (!multi && text[i] === "'") {
          i++;
          return;
        }
        if (!multi && (text[i] === "\n" || text[i] === "\r")) stop("toml-string", open);
        i++;
      }
    }

    /** One part of a key; returns it, for telling duplicates. */
    function keyPart() {
      const open = i;
      if (text[i] === '"') {
        basicString(false);
        return text.slice(open, i);
      }
      if (text[i] === "'") {
        literalString(false);
        return text.slice(open, i);
      }
      while (i < text.length && /[A-Za-z0-9_-]/.test(text[i])) i++;
      if (i === open) stop("toml-key");
      return text.slice(open, i);
    }

    function key() {
      const parts = [keyPart()];
      for (;;) {
        blanks();
        if (text[i] !== ".") return parts;
        i++;
        blanks();
        parts.push(keyPart());
      }
    }

    function arrayGap() {
      for (;;) {
        blanks();
        comment();
        if (i < text.length && atEol()) i += text[i] === "\r" ? 2 : 1;
        else return;
      }
    }

    function array() {
      i++;
      arrayGap();
      while (text[i] !== "]") {
        if (i >= text.length) stop("toml-value");
        value();
        arrayGap();
        if (text[i] === ",") {
          i++;
          arrayGap();
        } else if (text[i] !== "]") {
          stop("toml-value");
        }
      }
      i++;
    }

    function inlineTable() {
      i++;
      blanks();
      if (text[i] === "}") {
        i++;
        return;
      }
      for (;;) {
        key();
        if (text[i] !== "=") stop("toml-equals");
        i++;
        blanks();
        value();
        blanks();
        if (text[i] === ",") {
          i++;
          blanks();
        } else if (text[i] === "}") {
          i++;
          return;
        } else {
          stop("toml-value");
        }
      }
    }

    function value() {
      if (text.startsWith('"""', i)) return basicString(true);
      if (text.startsWith("'''", i)) return literalString(true);
      if (text[i] === '"') return basicString(false);
      if (text[i] === "'") return literalString(false);
      if (text[i] === "[") return array();
      if (text[i] === "{") return inlineTable();
      if (text.startsWith("true", i) || text.startsWith("false", i)) {
        i += text[i] === "t" ? 4 : 5;
      } else {
        NUMBER_OR_DATE.lastIndex = i;
        const match = NUMBER_OR_DATE.exec(text);
        if (!match || !match[0]) stop("toml-value");
        i += match[0].length;
      }
      if (i < text.length && !/[\s,\]}#]/.test(text[i])) stop("toml-value");
      return undefined;
    }

    const seen = new Set();
    let table = "";
    try {
      for (;;) {
        skipBlankLines();
        if (i >= text.length) break;
        const start = i;
        if (text[i] === "[") {
          const isArray = text[i + 1] === "[";
          i += isArray ? 2 : 1;
          blanks();
          const name = key().join("\u0000");
          if (text[i] !== "]" || (isArray && text[i + 1] !== "]")) stop("toml-header");
          i += isArray ? 2 : 1;
          endOfLine();
          if (isArray) {
            // Each [[item]] is a table of its own: its keys may repeat those of the last.
            for (const other of [...seen]) if (other.startsWith(`${name}\u0000`)) seen.delete(other);
          } else {
            if (seen.has(`[${name}]`)) stop("toml-duplicate", start);
            seen.add(`[${name}]`);
          }
          table = name;
        } else {
          const name = key().join("\u0000");
          if (text[i] !== "=") stop("toml-equals");
          i++;
          blanks();
          value();
          endOfLine();
          const whole = table ? `${table}\u0000${name}` : name;
          if (seen.has(whole)) stop("toml-duplicate", start);
          seen.add(whole);
        }
      }
    } catch (error) {
      if (!error || !error.toml) throw error;
      return { kind: "toml", ok: false, partial: true, code: error.toml, ...lineOf(text, error.at) };
    }
    return { kind: "toml", ok: true, partial: true };
  }

  /**
   * @returns {{ kind: string|null, ok: boolean, partial: boolean, code?: string, line?: number, column?: number }}
   */
  function validate(path, text) {
    const kind = checkerFor(path);
    if (kind === "json" || kind === "jsonc") return checkJson(text, kind);
    if (kind === "yaml") return checkYaml(text);
    if (kind === "toml") return checkToml(text);
    return { kind: null, ok: true, partial: false };
  }

  return { validate, checkerFor };
});
