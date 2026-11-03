#!/usr/bin/env node
// Writes crates/hangar-i18n/src/generated.rs from the strings of the React app.
//
//     node crates/hangar-i18n/tools/generate.mjs          write the file
//     node crates/hangar-i18n/tools/generate.mjs --check  fail if it is stale
//
// The TypeScript stays the source of truth while the web app and the native one
// live side by side: a translator edits src/i18n.ts, src/strings/*.ts or
// src/components/accounts/strings.ts as before, runs this, and commits the
// result. The output is committed so that building the crate needs no Node.
//
// It needs Node 22.18 or later, which strips TypeScript types on import. The
// string modules and src/lib/typography.ts are imported as they are, so the
// typesetting is not re-implemented here, it is the app's own. src/i18n.ts and
// the accounts strings also hold React hooks and imports of the store, so they
// are not imported: the two dictionary literals are cut out of the source and
// evaluated on their own, with the string modules they spread in handed over.

import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "..");
const OUT = join(ROOT, "crates", "hangar-i18n", "src", "generated.rs");

const read = (path) => readFileSync(join(ROOT, path), "utf8");
const load = (path) => import(pathToFileURL(join(ROOT, path)).href);

/**
 * The text of the object literal whose `{` is the last character of `marker`,
 * up to and including its matching `}`. It walks the source rather than
 * counting braces blindly because the strings hold `{name}` placeholders.
 */
function objectLiteral(source, marker, file) {
  const start = source.indexOf(marker);
  if (start < 0) throw new Error(`${file}: cannot find \`${marker}\``);
  const open = start + marker.length - 1;
  let depth = 0;
  for (let i = open; i < source.length; i++) {
    const c = source[i];
    if (c === '"' || c === "'" || c === "`") {
      for (i++; source[i] !== c; i++) if (source[i] === "\\") i++;
    } else if (c === "/" && source[i + 1] === "/") {
      while (source[i] !== "\n") i++;
    } else if (c === "/" && source[i + 1] === "*") {
      i = source.indexOf("*/", i + 2) + 1;
    } else if (c === "{") {
      depth++;
    } else if (c === "}" && --depth === 0) {
      return source.slice(open, i + 1);
    }
  }
  throw new Error(`${file}: \`${marker}\` is never closed`);
}

const evaluate = (literal, scope) =>
  new Function(...Object.keys(scope), `return (${literal});`)(...Object.values(scope));

/** Refuses a no-break space typed into a source: the typesetter is the only one to set them. */
function checkHandSpacing(literal, label) {
  // Written by hand, no-break spaces drift: some strings had them and most did
  // not. The typesetter sets them all, so the source must not hold any.
  if (/[  ]|\\u00a0|\\u202f/i.test(literal)) {
    throw new Error(`${label}: a no-break space is typed in the source; write a plain space, the typesetter adds it`);
  }
}

const { typeset } = await load("src/lib/typography.ts");

// The dictionaries, each as { en, fr } of plain objects.
const experiments = await load("src/strings/experiments.ts");
const kitsManage = await load("src/strings/kitsManage.ts");
const kitsTab = await load("src/strings/kitsTab.ts");
const worktrees = await load("src/strings/worktrees.ts");
const workspaceFromGit = await load("src/strings/workspaceFromGit.ts");
const paneBranch = await load("src/strings/paneBranch.ts");

const i18nSource = read("src/i18n.ts");
const spread = { experiments, kitsManage, kitsTab, worktrees, workspaceFromGit };
const enLiteral = objectLiteral(i18nSource, "const en = {", "src/i18n.ts");
const frLiteral = objectLiteral(i18nSource, "const fr: Record<Key, string> = {", "src/i18n.ts");
checkHandSpacing(frLiteral, "src/i18n.ts fr");
const main = { en: evaluate(enLiteral, spread), fr: evaluate(frLiteral, spread) };
for (const name of [...Object.keys(spread), "paneBranch"]) checkHandSpacing(read(`src/strings/${name}.ts`), `src/strings/${name}.ts`);

// The accounts components keep their strings beside them and name them
// `settings.title`, `switcher.back`… which clash with the central dictionary's
// own `settings.*`, so they live under an `accounts.` prefix here.
const accountsSource = read("src/components/accounts/strings.ts");
const accounts = {
  en: evaluate(objectLiteral(accountsSource, "export const en = {", "accounts/strings.ts"), {}),
  fr: evaluate(objectLiteral(accountsSource, "export const fr: Record<AccountsKey, string> = {", "accounts/strings.ts"), {}),
};
checkHandSpacing(accountsSource, "src/components/accounts/strings.ts");
const prefixed = (dict, prefix) => Object.fromEntries(Object.entries(dict).map(([k, v]) => [prefix + k, v]));

const sources = [
  ["src/i18n.ts and src/strings/*.ts", main],
  ["src/strings/paneBranch.ts", paneBranch],
  ["src/components/accounts/strings.ts", { en: prefixed(accounts.en, "accounts."), fr: prefixed(accounts.fr, "accounts.") }],
];

const en = {};
const fr = {};
for (const [label, dict] of sources) {
  const missing = Object.keys(dict.en).filter((k) => !(k in dict.fr));
  const extra = Object.keys(dict.fr).filter((k) => !(k in dict.en));
  if (missing.length || extra.length) {
    throw new Error(`${label}: French lacks [${missing}] and has [${extra}] that English does not`);
  }
  for (const k of Object.keys(dict.en)) {
    if (k in en) throw new Error(`${label}: key ${k} is defined twice`);
    en[k] = dict.en[k];
    fr[k] = dict.fr[k];
  }
}

// Plural keys come as `_one` / `_other`. The runtime models those two forms
// (plus the `many` French has for whole millions, which the dictionaries never
// fill), so any other suffix is refused instead of being taken for a plain key.
const messages = new Map(); // id -> { en: {one, other}, fr: {one, other} }
const FORM = /^(.*)_(zero|one|two|few|many|other)$/;
for (const key of Object.keys(en)) {
  const match = FORM.exec(key);
  if (!match) {
    messages.set(key, { en: { one: en[key], other: en[key] }, fr: { one: fr[key], other: fr[key] } });
    continue;
  }
  const [, base, form] = match;
  if (form !== "one" && form !== "other") {
    throw new Error(`${key}: the plural form \`${form}\` is not modelled; extend plural.rs and this script first`);
  }
  if (base in en) throw new Error(`${base} is both a plain key and a plural one`);
  if (!(`${base}_one` in en) || !(`${base}_other` in en)) throw new Error(`${base} needs both _one and _other`);
  if (form === "one") {
    messages.set(base, {
      en: { one: en[key], other: en[`${base}_other`] },
      fr: { one: fr[key], other: fr[`${base}_other`] },
    });
  }
}

// Typography is applied here, once, exactly as the app does when its
// dictionaries load: `typeset` is typography.ts itself.
for (const message of messages.values()) {
  for (const locale of ["en", "fr"]) {
    const forms = message[locale];
    forms.one = typeset(forms.one, locale);
    forms.other = typeset(forms.other, locale);
  }
}

const compare = (a, b) => Buffer.compare(Buffer.from(a), Buffer.from(b));
const ids = [...messages.keys()].sort(compare);

/** `pane.state.working` -> `PaneStateWorking`. */
function variant(id) {
  const name = id
    .split(/[^A-Za-z0-9]+/)
    .filter(Boolean)
    .map((part) => part[0].toUpperCase() + part.slice(1))
    .join("");
  return /^[0-9]/.test(name) ? `K${name}` : name;
}
const names = new Map();
for (const id of ids) {
  const name = variant(id);
  if (names.has(name)) throw new Error(`keys ${names.get(name)} and ${id} both become \`Key::${name}\``);
  names.set(name, id);
}

/** A Rust string literal. Invisible characters are escaped so a diff can show them. */
function literal(text) {
  let out = '"';
  for (const c of text) {
    if (c === "\\") out += "\\\\";
    else if (c === '"') out += '\\"';
    else if (c === "\n") out += "\\n";
    else if (c === "\r") out += "\\r";
    else if (c === "\t") out += "\\t";
    else if (c !== " " && /[\p{Z}\p{C}]/u.test(c)) out += `\\u{${c.codePointAt(0).toString(16)}}`;
    else out += c;
  }
  return `${out}"`;
}

const text = ({ one, other }) =>
  one === other ? `Text::plain(${literal(one)})` : `Text::plural(${literal(one)}, ${literal(other)})`;

const lines = [];
lines.push(
  "// @generated by crates/hangar-i18n/tools/generate.mjs, do not edit.",
  "//",
  "// Source: src/i18n.ts, src/strings/*.ts and src/components/accounts/strings.ts,",
  "// typeset as the app typesets them (src/lib/typography.ts). Change the strings",
  "// there and run the script again; `--check` tells whether this file is stale.",
  "",
  "use crate::dictionary::{Message, Text};",
  "",
  "/// Every interface string, as a name the compiler checks. A plural message is",
  "/// one key (`SidebarActivityWorking`), not its `_one` and `_other` strings.",
  "///",
  "/// The variants follow the order of `MESSAGES`, which is sorted by id, so a key",
  "/// is an index into it.",
  "#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]",
  "#[repr(u16)]",
  "pub enum Key {",
  ...ids.flatMap((id) => [`    /// \`${id}\``, `    ${variant(id)},`]),
  "}",
  "",
  `/// How many keys there are.`,
  `pub const KEY_COUNT: usize = ${ids.length};`,
  "",
  "impl Key {",
  "    /// Every key, in the order of their ids.",
  `    pub const ALL: [Key; KEY_COUNT] = [`,
  ...ids.map((id) => `        Key::${variant(id)},`),
  "    ];",
  "}",
  "",
  "/// The messages, sorted by id (byte order), one per `Key`.",
  "pub(crate) static MESSAGES: [Message; KEY_COUNT] = [",
  ...ids.map((id) => {
    const m = messages.get(id);
    return `    Message { id: ${literal(id)}, en: ${text(m.en)}, fr: ${text(m.fr)} },`;
  }),
  "];",
  "",
);

const output = lines.join("\n");
if (process.argv.includes("--check")) {
  if (!existsSync(OUT) || readFileSync(OUT, "utf8") !== output) {
    console.error("crates/hangar-i18n/src/generated.rs is stale: run node crates/hangar-i18n/tools/generate.mjs");
    process.exit(1);
  }
  console.log(`generated.rs is up to date (${ids.length} keys)`);
} else {
  writeFileSync(OUT, output);
  console.log(`wrote ${OUT}: ${ids.length} keys (${Object.keys(en).length} strings)`);
}
