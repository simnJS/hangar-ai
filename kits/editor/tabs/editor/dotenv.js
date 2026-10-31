"use strict";
// Reads a dotenv file so that saving it changes only what was edited: every line
// keeps its exact bytes (comments, blanks, order, line endings, quotes) unless its
// own value changed. No DOM, no Hangar: the page and the tests both load this file.
(function (root, factory) {
  const api = factory();
  if (typeof module === "object" && module.exports) module.exports = api;
  else root.EditorDotenv = api;
})(typeof globalThis !== "undefined" ? globalThis : this, function () {
  const ASSIGNMENT = /^([ \t]*(?:export[ \t]+)?)([A-Za-z_][A-Za-z0-9_.-]*)([ \t]*=[ \t]*)/;
  const KEY = /^[A-Za-z_][A-Za-z0-9_.-]*$/;
  const BARE_SAFE = /^[A-Za-z0-9_./:@%+,=~-]+$/;
  // `# OLD_TOKEN=abc` is a value put aside, not an explanation: it is never shown as a hint.
  const COMMENTED_ASSIGNMENT = /^#+[ \t]*(?:export[ \t]+)?[A-Za-z_][A-Za-z0-9_.-]*[ \t]*=/;
  const ESCAPES = { n: "\n", r: "\r", t: "\t", '"': '"', "\\": "\\", $: "$" };
  const HINT_MAX = 300;

  const isKey = (name) => typeof name === "string" && KEY.test(name);

  function lineEnd(text, from) {
    for (let i = from; i < text.length; i++) {
      if (text[i] === "\n" || text[i] === "\r") return i;
    }
    return text.length;
  }

  function eolAt(text, at) {
    if (text[at] === "\r" && text[at + 1] === "\n") return "\r\n";
    return text[at] === "\n" || text[at] === "\r" ? text[at] : "";
  }

  /** Where the quote opened before `from` closes, or -1. */
  function closing(text, from, quote, escapes) {
    for (let i = from; i < text.length; i++) {
      if (escapes && text[i] === "\\") i++;
      else if (text[i] === quote) return i;
    }
    return -1;
  }

  function unquoted(inner, quote, escapes) {
    if (quote === '"') {
      return inner.replace(/\\([^])/g, (whole, char) => (ESCAPES[char] === undefined ? whole : ESCAPES[char]));
    }
    return escapes ? inner.split(`\\${quote}`).join(quote) : inner;
  }

  /**
   * A quoted value, over several lines if it says so. A quote counts as closed only
   * when nothing but a comment follows it: otherwise `A="x` would swallow the lines
   * below it, and what is a value would be read as keys, or the reverse.
   */
  function readQuoted(text, start) {
    const quote = text[start];
    for (const escapes of quote === '"' ? [true] : [false, true]) {
      const close = closing(text, start + 1, quote, escapes);
      if (close < 0) continue;
      const end = lineEnd(text, close + 1);
      const rest = text.slice(close + 1, end);
      if (!/^[ \t]*(?:#[^]*)?$/.test(rest)) continue;
      return { value: unquoted(text.slice(start + 1, close), quote, escapes), quote, trailing: rest, end };
    }
    return null;
  }

  function readBare(rest, afterBlank) {
    let cut = rest.length;
    for (let i = 0; i < rest.length; i++) {
      if (rest[i] === "#" && (i > 0 ? rest[i - 1] === " " || rest[i - 1] === "\t" : afterBlank)) {
        cut = i;
        break;
      }
    }
    let end = cut;
    while (end > 0 && (rest[end - 1] === " " || rest[end - 1] === "\t")) end--;
    return { value: rest.slice(0, end), trailing: rest.slice(end), quote: "" };
  }

  /** The text, as entries: a pair, or a line kept as it is. `serialize` gives back the same text. */
  function parse(text) {
    const entries = [];
    let at = 0;
    let crlf = 0;
    let lf = 0;
    while (at < text.length) {
      const end = lineEnd(text, at);
      const line = text.slice(at, end);
      const match = ASSIGNMENT.exec(line);
      let entryEnd = end;
      if (match) {
        const [head, lead, key, separator] = match;
        const start = at + head.length;
        const quoted = "\"'`".includes(text[start] || " ") ? readQuoted(text, start) : null;
        const read = quoted || readBare(text.slice(start, end), /[ \t]$/.test(separator));
        if (quoted) entryEnd = quoted.end;
        entries.push({
          kind: "pair",
          key,
          value: read.value,
          prefix: lead + key,
          separator,
          quote: read.quote,
          trailing: read.trailing,
          raw: text.slice(at, entryEnd),
          eol: eolAt(text, entryEnd),
        });
      } else {
        const trimmed = line.trim();
        entries.push({ kind: "other", raw: line, eol: eolAt(text, end), blank: trimmed === "", comment: trimmed[0] === "#" });
      }
      const eol = eolAt(text, entryEnd);
      if (eol === "\r\n") crlf++;
      else if (eol === "\n") lf++;
      at = entryEnd + eol.length;
    }
    return { entries, eol: crlf > lf ? "\r\n" : "\n" };
  }

  const serialize = (doc) => doc.entries.map((entry) => entry.raw + entry.eol).join("");

  /**
   * A value as the file should hold it. Single quotes are the least magical: no
   * escape, no expansion, several lines allowed. A value holding one needs double
   * quotes, and then every parser reads \\ \" \n and \r back.
   */
  function quoted(value, previous) {
    if (value === "") return previous ? previous + previous : "";
    if (BARE_SAFE.test(value)) return previous ? previous + value + previous : value;
    if (!value.includes("'")) return `'${value}'`;
    const escaped = value.replace(/\\/g, "\\\\").replace(/"/g, '\\"').replace(/\r/g, "\\r").replace(/\n/g, "\\n");
    return `"${escaped}"`;
  }

  function rendered(entry, value) {
    const body = quoted(value, entry.quote);
    const gap = body && entry.trailing[0] === "#" ? " " : "";
    return entry.prefix + entry.separator + body + gap + entry.trailing;
  }

  /**
   * The doc with these values set (a Map of key → value), each on the last line that
   * defines its key, the one parsers keep. A key the file does not have goes at the
   * end. Nothing else moves.
   */
  function withValues(doc, edits) {
    const remaining = new Map(edits);
    const last = new Map();
    doc.entries.forEach((entry, index) => {
      if (entry.kind === "pair") last.set(entry.key, index);
    });
    const entries = doc.entries.map((entry, index) => {
      if (entry.kind !== "pair" || last.get(entry.key) !== index || !remaining.has(entry.key)) return entry;
      const value = remaining.get(entry.key);
      remaining.delete(entry.key);
      return value === entry.value ? entry : { ...entry, value, raw: rendered(entry, value) };
    });
    for (const [key, value] of remaining) {
      const tail = entries[entries.length - 1];
      if (tail && tail.eol === "") entries[entries.length - 1] = { ...tail, eol: doc.eol };
      const fresh = { kind: "pair", key, value, prefix: key, separator: "=", quote: "", trailing: "", eol: doc.eol };
      entries.push({ ...fresh, raw: rendered(fresh, value) });
    }
    return { entries, eol: doc.eol };
  }

  /** One row per key, in the file's order: the value parsers keep, the comment above it, how many lines define it. */
  function rows(doc) {
    const found = new Map();
    let comments = [];
    for (const entry of doc.entries) {
      if (entry.kind === "other") {
        const line = entry.raw.trim();
        if (entry.comment && !COMMENTED_ASSIGNMENT.test(line)) comments.push(line.replace(/^#+[ \t]?/, ""));
        else if (!entry.comment) comments = [];
        continue;
      }
      const known = found.get(entry.key);
      if (known) {
        known.value = entry.value;
        known.count++;
      } else {
        found.set(entry.key, { key: entry.key, value: entry.value, count: 1, hint: comments.join(" ").slice(0, HINT_MAX) });
      }
      comments = [];
    }
    return [...found.values()];
  }

  /** Every value the file holds, those that a later line overrides included. */
  const allValues = (doc) => doc.entries.filter((entry) => entry.kind === "pair").map((entry) => entry.value);

  /** Lines that are neither a pair, a comment nor blank: never shown, kept as they are. */
  const otherLines = (doc) => doc.entries.filter((entry) => entry.kind === "other" && !entry.blank && !entry.comment).length;

  /** Which keys got a value and which lost theirs, by name. It returns no value. */
  function changedKeys(before, after) {
    const old = new Map(rows(before).map((row) => [row.key, row.value]));
    const set = [];
    const emptied = [];
    for (const row of rows(after)) {
      if (row.value === old.get(row.key)) continue;
      if (row.value !== "") set.push(row.key);
      else if (old.has(row.key)) emptied.push(row.key);
    }
    return { set, emptied };
  }

  const baseName = (path) => path.slice(path.lastIndexOf("/") + 1);

  function isDotenvPath(path) {
    const name = baseName(path).toLowerCase();
    return /^\.env(\..+)?$/.test(name) || /.\.env$/.test(name);
  }

  /** A template meant to be committed, with no secret to protect. */
  const isExamplePath = (path) => isDotenvPath(path) && /\.(example|sample|template|dist|defaults?)$/i.test(baseName(path));

  /** Where this file's template may be, the nearest name first. */
  function exampleCandidates(path) {
    const slash = path.lastIndexOf("/");
    const dir = slash < 0 ? "" : path.slice(0, slash + 1);
    const name = baseName(path);
    const names = [`${name}.example`, `${name}.sample`, ".env.example", ".env.sample", ".env.template", ".env.dist"];
    return [...new Set(names)].filter((other) => other !== name).map((other) => dir + other);
  }

  return { parse, serialize, withValues, rows, allValues, otherLines, changedKeys, isKey, isDotenvPath, isExamplePath, exampleCandidates };
});
