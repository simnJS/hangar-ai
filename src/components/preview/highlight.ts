import hljs from "highlight.js/lib/core";
import bash from "highlight.js/lib/languages/bash";
import c from "highlight.js/lib/languages/c";
import cpp from "highlight.js/lib/languages/cpp";
import csharp from "highlight.js/lib/languages/csharp";
import css from "highlight.js/lib/languages/css";
import diff from "highlight.js/lib/languages/diff";
import dockerfile from "highlight.js/lib/languages/dockerfile";
import dos from "highlight.js/lib/languages/dos";
import go from "highlight.js/lib/languages/go";
import ini from "highlight.js/lib/languages/ini";
import java from "highlight.js/lib/languages/java";
import javascript from "highlight.js/lib/languages/javascript";
import json from "highlight.js/lib/languages/json";
import kotlin from "highlight.js/lib/languages/kotlin";
import less from "highlight.js/lib/languages/less";
import lua from "highlight.js/lib/languages/lua";
import makefile from "highlight.js/lib/languages/makefile";
import markdown from "highlight.js/lib/languages/markdown";
import php from "highlight.js/lib/languages/php";
import plaintext from "highlight.js/lib/languages/plaintext";
import powershell from "highlight.js/lib/languages/powershell";
import python from "highlight.js/lib/languages/python";
import ruby from "highlight.js/lib/languages/ruby";
import rust from "highlight.js/lib/languages/rust";
import scss from "highlight.js/lib/languages/scss";
import shell from "highlight.js/lib/languages/shell";
import sql from "highlight.js/lib/languages/sql";
import swift from "highlight.js/lib/languages/swift";
import typescript from "highlight.js/lib/languages/typescript";
import xml from "highlight.js/lib/languages/xml";
import yaml from "highlight.js/lib/languages/yaml";
import { splitHighlighted } from "../../lib/preview";

/**
 * Syntax colouring for the preview's text, code and Markdown: highlight.js
 * 11, its core and the languages an agent's files are written in — not the
 * 190 of its full build. It is maintained, escapes the text it is given, and
 * its output is spans with classes, which the theme colours (styles.css).
 *
 * Loaded on demand, through a dynamic import, the first time a text viewer
 * needs it: none of it is in the main bundle.
 */

const LANGUAGES = {
  bash,
  c,
  cpp,
  csharp,
  css,
  diff,
  dockerfile,
  dos,
  go,
  ini,
  java,
  javascript,
  json,
  kotlin,
  less,
  lua,
  makefile,
  markdown,
  php,
  plaintext,
  powershell,
  python,
  ruby,
  rust,
  scss,
  shell,
  sql,
  swift,
  typescript,
  xml,
  yaml,
};

for (const [name, language] of Object.entries(LANGUAGES)) hljs.registerLanguage(name, language);

/**
 * `text` as highlighted HTML, its characters escaped; null for a language
 * that is not registered, by name or by alias (`ts`, `sh`, `html`, `yml`…).
 */
export function highlight(text: string, language: string): string | null {
  if (!language || !hljs.getLanguage(language)) return null;
  return hljs.highlight(text, { language, ignoreIllegals: true }).value;
}

/** The same, one string of HTML per line of `text` (lib/preview › splitHighlighted). */
export function highlightLines(text: string, language: string): string[] | null {
  const html = highlight(text, language);
  return html === null ? null : splitHighlighted(html);
}
