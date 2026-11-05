#!/usr/bin/env node
// Writes crates/hangar-i18n/src/generated.rs from the dictionaries beside it.
//
//     node crates/hangar-i18n/tools/generate.mjs          write the file
//     node crates/hangar-i18n/tools/generate.mjs --check  fail if it is stale
//
// The strings live in crates/hangar-i18n/dict/en.json and fr.json, one key per
// line, the French file in the English one's order. A translator edits them,
// runs this, and commits the result. The output is committed so that building
// the crate needs no Node; any Node from 18 on runs this script.
//
// Line endings do not count: `--check` compares with CRLF folded to LF, so it
// passes on a Windows clone with core.autocrlf=true, where git checks the
// files out with CRLF. The file is always written with LF.
//
// The dictionaries were the React app's src/i18n.ts, src/strings/*.ts and
// src/components/accounts/strings.ts (tag v1.12.1), merged as they were read:
// the accounts strings keep the `accounts.` prefix they were given here.

import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { typeset } from "./typography.mjs";

const CRATE = join(dirname(fileURLToPath(import.meta.url)), "..");
const OUT = join(CRATE, "src", "generated.rs");

/**
 * Text with LF line endings. A Windows clone with core.autocrlf=true checks
 * the files out with CRLF while the generator always writes LF, so the
 * dictionaries are read, and the committed output compared in `--check`,
 * with line endings folded to LF: only the content decides whether the file
 * is stale. Writing stays LF whatever the checkout.
 */
const lf = (text) => text.replace(/\r\n/g, "\n");

/** Refuses a no-break space typed into a source: the typesetter is the only one to set them. */
function checkHandSpacing(text, label) {
  // Written by hand, no-break spaces drift: some strings had them and most did
  // not. The typesetter sets them all, so the source must not hold any.
  if (/[  ]|\\u00a0|\\u202f/i.test(text)) {
    throw new Error(`${label}: a no-break space is typed in the source; write a plain space, the typesetter adds it`);
  }
}

/**
 * A dictionary file, refusing a key written twice: JSON.parse would keep the
 * last one without a word, and the first would silently be lost.
 */
function readDictionary(locale) {
  const file = `dict/${locale}.json`;
  const text = lf(readFileSync(join(CRATE, file), "utf8"));
  checkHandSpacing(text, file);
  const dict = JSON.parse(text);
  const seen = new Set();
  for (const [, key] of text.matchAll(/^ {2}("(?:[^"\\]|\\.)*"):/gm)) {
    const id = JSON.parse(key);
    if (seen.has(id)) throw new Error(`${file}: key ${id} is defined twice`);
    seen.add(id);
  }
  if (seen.size !== Object.keys(dict).length) {
    throw new Error(`${file}: write one key per line, two spaces in, as the file has them`);
  }
  for (const [key, value] of Object.entries(dict)) {
    if (typeof value !== "string") throw new Error(`${file}: ${key} is not a string`);
  }
  return dict;
}

const en = readDictionary("en");
const fr = readDictionary("fr");
{
  const missing = Object.keys(en).filter((k) => !(k in fr));
  const extra = Object.keys(fr).filter((k) => !(k in en));
  if (missing.length || extra.length) {
    throw new Error(`dict/fr.json lacks [${missing}] and has [${extra}] that dict/en.json does not`);
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

// Typography is applied here, once, as the web app did when its dictionaries
// loaded: tools/typography.mjs holds its rules.
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
  "// Source: crates/hangar-i18n/dict/en.json and fr.json, typeset by",
  "// tools/typography.mjs. Change the strings there and run the script again;",
  "// `--check` tells whether this file is stale.",
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
  if (!existsSync(OUT) || lf(readFileSync(OUT, "utf8")) !== output) {
    console.error("crates/hangar-i18n/src/generated.rs is stale: run node crates/hangar-i18n/tools/generate.mjs");
    process.exit(1);
  }
  console.log(`generated.rs is up to date (${ids.length} keys)`);
} else {
  writeFileSync(OUT, output);
  console.log(`wrote ${OUT}: ${ids.length} keys (${Object.keys(en).length} strings)`);
}
