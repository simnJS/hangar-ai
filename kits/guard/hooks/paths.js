"use strict";
// Paths, as text: nothing here touches the disk, so a symlink is not followed.
// Every path comes out with forward slashes. On Windows, `C:\x`, `C:/x`, `/c/x`
// and `/cygdrive/c/x` are all the same path, and case does not count.

function normalize(input, base, platform) {
  let text = String(input);
  if (platform === "win32") {
    text = text.replace(/\\/g, "/");
    const msys = /^\/(?:cygdrive\/)?([a-zA-Z])(\/.*)?$/.exec(text);
    if (msys) text = `${msys[1]}:${msys[2] || "/"}`;
    const driveRelative = /^([a-zA-Z]):(?!\/)(.*)$/.exec(text);
    if (driveRelative) text = `${driveRelative[1]}:/${driveRelative[2]}`;
  }
  const drive = platform === "win32" ? /^([a-zA-Z]:)\/(.*)$/.exec(text) : null;
  let prefix;
  let rest;
  if (drive) {
    prefix = `${drive[1]}/`;
    rest = drive[2];
  } else if (text.startsWith("/")) {
    prefix = "/";
    rest = text;
  } else {
    if (base === null || base === undefined) return null;
    return normalize(`${normalize(base, null, platform)}/${text}`, null, platform);
  }
  const stack = [];
  for (const segment of rest.split("/")) {
    if (!segment || segment === ".") continue;
    if (segment === "..") stack.pop();
    else stack.push(segment);
  }
  return prefix + stack.join("/");
}

const key = (path, platform) => (platform === "win32" ? path.toLowerCase() : path);

const same = (a, b, platform) => key(a, platform) === key(b, platform);

/** Whether `child` lies strictly under `parent`. */
function isUnder(child, parent, platform) {
  const c = key(child, platform);
  let p = key(parent, platform);
  if (c === p) return false;
  if (!p.endsWith("/")) p += "/";
  return c.startsWith(p);
}

const isFilesystemRoot = (path) => path === "/" || /^[a-zA-Z]:\/$/.test(path);

const compiled = new Map();

function globRegExp(glob, anchorStart) {
  const id = `${anchorStart ? "^" : ""}${glob}`;
  if (!compiled.has(id)) {
    const body = glob
      .split("")
      .map((c) => (c === "*" ? "[^/]*" : c === "?" ? "[^/]" : c.replace(/[.+^${}()|[\]\\]/g, "\\$&")))
      .join("");
    compiled.set(id, new RegExp(`${anchorStart ? "^" : "(?:^|/)"}${body}$`, "i"));
  }
  return compiled.get(id);
}

/**
 * Whether a path answers to a rule's pattern. `*` and `?` stay within one path
 * segment, case does not count. Without a slash the pattern is for the file's
 * name, with one for the end of its path, and `~/` ties it to the home folder.
 */
function matchesPattern(path, pattern, home) {
  if (pattern.startsWith("~/")) return globRegExp(`${home.replace(/\/$/, "")}/${pattern.slice(2)}`, true).test(path);
  return globRegExp(pattern, false).test(path);
}

/** `a{b,c}d` → `abd`, `acd`, at most 64 of them. */
function expandBraces(text) {
  const done = [];
  const queue = [text];
  while (queue.length) {
    const item = queue.shift();
    const match = done.length + queue.length < 64 ? /\{([^{}]*,[^{}]*)\}/.exec(item) : null;
    if (!match) {
      done.push(item);
      continue;
    }
    for (const option of match[1].split(",")) {
      queue.push(item.slice(0, match.index) + option + item.slice(match.index + match[0].length));
    }
  }
  return done.slice(0, 64);
}

module.exports = { normalize, same, isUnder, isFilesystemRoot, matchesPattern, expandBraces };
