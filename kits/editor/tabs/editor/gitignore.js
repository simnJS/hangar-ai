"use strict";
// Whether .gitignore files ignore a path, by git's own rules. What git also reads
// and a tab cannot (.git/info/exclude, the global excludes, whether the file is
// already tracked) is said in the README. No DOM, no Hangar.
(function (root, factory) {
  const api = factory();
  if (typeof module === "object" && module.exports) module.exports = api;
  else root.EditorGitignore = api;
})(typeof globalThis !== "undefined" ? globalThis : this, function () {
  const literal = (char) => char.replace(/[\\^$.*+?()[\]{}|/-]/g, "\\$&");

  /** A character class at `at`, as a regular expression, and where it ends; null if it never closes. */
  function charClass(glob, at) {
    let i = at + 1;
    let out = "[";
    if (glob[i] === "!" || glob[i] === "^") {
      out += "^";
      i++;
    }
    if (glob[i] === "]") {
      out += "\\]";
      i++;
    }
    for (; i < glob.length; i++) {
      if (glob[i] === "]") return { source: `${out}]`, end: i };
      if (glob[i] === "\\" && i + 1 < glob.length) out += literal(glob[++i]);
      else if (glob[i] === "[") out += "\\[";
      else out += glob[i];
    }
    return null;
  }

  function toRegExp(glob) {
    const anchored = glob.includes("/");
    const body = glob.startsWith("/") ? glob.slice(1) : glob;
    let out = anchored ? "^" : "^(?:.*/)?";
    for (let i = 0; i < body.length; i++) {
      const char = body[i];
      if (char === "*" && body[i + 1] === "*" && (i === 0 || body[i - 1] === "/")) {
        if (body[i + 2] === "/") {
          out += "(?:.*/)?";
          i += 2;
          continue;
        }
        if (i + 2 === body.length) {
          out += ".+";
          i += 1;
          continue;
        }
      }
      if (char === "*") {
        while (body[i + 1] === "*") i++;
        out += "[^/]*";
      } else if (char === "?") {
        out += "[^/]";
      } else if (char === "\\" && i + 1 < body.length) {
        out += literal(body[++i]);
      } else if (char === "[") {
        const found = charClass(body, i);
        if (found) {
          out += found.source;
          i = found.end;
        } else {
          out += "\\[";
        }
      } else {
        out += literal(char);
      }
    }
    try {
      return new RegExp(`${out}$`);
    } catch {
      return null;
    }
  }

  /** The rules of one .gitignore, in order. */
  function rulesOf(text) {
    const rules = [];
    for (const raw of text.split(/\r\n?|\n/)) {
      let line = raw;
      // Trailing blanks do not count, unless the last is escaped.
      let end = line.length;
      while (end > 0 && (line[end - 1] === " " || line[end - 1] === "\t")) end--;
      if (end < line.length && line[end - 1] === "\\") end++;
      line = line.slice(0, end);
      if (!line || line[0] === "#") continue;
      const negated = line[0] === "!";
      if (negated) line = line.slice(1);
      const dirOnly = line.endsWith("/");
      if (dirOnly) line = line.slice(0, -1);
      if (!line) continue;
      const regex = toRegExp(line);
      if (regex) rules.push({ negated, dirOnly, regex });
    }
    return rules;
  }

  /** What the sources say of this path: the last rule that matches wins, deeper files last. */
  function decision(sources, path, isDir) {
    let ignored = false;
    for (const { dir, rules } of sources) {
      if (dir && !path.startsWith(`${dir}/`)) continue;
      const inside = dir ? path.slice(dir.length + 1) : path;
      for (const rule of rules) {
        if ((!rule.dirOnly || isDir) && rule.regex.test(inside)) ignored = !rule.negated;
      }
    }
    return ignored;
  }

  /**
   * @param {{ dir: string, text: string }[]} sources .gitignore files, root first; `dir` is "" for the root
   * @param {string} path relative to the root, with /
   */
  function isIgnored(sources, path) {
    const compiled = sources.map((source) => ({ dir: source.dir, rules: rulesOf(source.text) }));
    const parts = path.split("/");
    // A file cannot be brought back when one of its folders is ignored.
    for (let i = 1; i < parts.length; i++) {
      if (decision(compiled, parts.slice(0, i).join("/"), true)) return true;
    }
    return decision(compiled, path, false);
  }

  /** The .gitignore files that have a say on this path, root first. */
  function sourcesFor(path) {
    const parts = path.split("/").slice(0, -1);
    const out = [{ dir: "", path: ".gitignore" }];
    for (let i = 1; i <= parts.length; i++) {
      const dir = parts.slice(0, i).join("/");
      out.push({ dir, path: `${dir}/.gitignore` });
    }
    return out;
  }

  /** The line that ignores this very file, for the root's .gitignore. */
  function patternFor(path) {
    const escaped = path.replace(/[\\*?[]/g, "\\$&").replace(/ +$/, (spaces) => "\\ ".repeat(spaces.length));
    return /^[#!]/.test(escaped) ? `\\${escaped}` : escaped;
  }

  /** The text with that line added, in the file's line endings. */
  function withPattern(text, pattern) {
    const eol = text.includes("\r\n") ? "\r\n" : "\n";
    const closed = text === "" || /[\r\n]$/.test(text) ? text : text + eol;
    return `${closed}${pattern}${eol}`;
  }

  return { isIgnored, sourcesFor, patternFor, withPattern };
});
