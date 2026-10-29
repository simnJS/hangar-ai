/**
 * Code coloured for reading on the phone: comments, strings, numbers and
 * keywords, and a markup's tags and attributes. A scanner of its own, small
 * enough for the few dozen languages agents write, not a parser: it never
 * fails, it only colours less. What it gives back is text in pieces, so a
 * file can no more put HTML on the page than a chat message can, and the
 * pieces put end to end are the file as it was.
 */

export type Language =
  | "js"
  | "rust"
  | "go"
  | "clike"
  | "python"
  | "ruby"
  | "shell"
  | "batch"
  | "sql"
  | "lua"
  | "css"
  | "json"
  | "yaml"
  | "toml"
  | "markup"
  | "verse"
  | "plain";

export type TokenKind = "plain" | "comment" | "string" | "number" | "keyword" | "tag" | "attr";

export interface Token {
  kind: TokenKind;
  text: string;
}

interface Grammar {
  /** What starts a comment to the end of its line. A lone `#` does so only after a space or at a line's start. */
  line: string[];
  block: [string, string][];
  /** Strings that may run over several lines, opened and closed by these. */
  long?: [string, string][];
  /** Quotes of one-line strings, but those in `multiline`. */
  quotes: string[];
  multiline?: string[];
  keywords: ReadonlySet<string>;
  /** Keywords in any case, as SQL and PowerShell read them. */
  caseless?: boolean;
  word?: RegExp;
  /** Read right after a name or a string: if it matches, that was a key. */
  keyBefore?: RegExp;
}

const words = (list: string) => new Set(list.split(" "));

const C_COMMENTS = { line: ["//"], block: [["/*", "*/"]] as [string, string][] };

const GRAMMARS: Record<Exclude<Language, "markup" | "plain">, Grammar> = {
  js: {
    ...C_COMMENTS,
    quotes: ['"', "'", "`"],
    multiline: ["`"],
    keywords: words(
      "break case catch class const continue debugger default delete do else export extends finally for function if import in instanceof let new return super switch this throw try typeof var void while with yield async await of static get set true false null undefined interface type enum implements private protected public readonly abstract declare namespace as from keyof infer never unknown any number string boolean satisfies",
    ),
  },
  rust: {
    ...C_COMMENTS,
    quotes: ['"'],
    multiline: ['"'],
    keywords: words(
      "as async await break const continue crate dyn else enum extern false fn for if impl in let loop match mod move mut pub ref return self Self static struct super trait true type unsafe use where while Some None Ok Err",
    ),
  },
  go: {
    ...C_COMMENTS,
    quotes: ['"', "'", "`"],
    multiline: ["`"],
    keywords: words(
      "break case chan const continue default defer else fallthrough for func go goto if import interface map package range return select struct switch type var true false nil iota",
    ),
  },
  clike: {
    ...C_COMMENTS,
    quotes: ['"', "'"],
    long: [['"""', '"""']],
    keywords: words(
      "abstract auto bool boolean break byte case catch char class const continue default delete do double else enum extends final finally float for fun func goto if implements import in inline int interface internal is let long namespace new null nil object open override package private protected public return sealed self short signed sizeof static struct super switch synchronized template this throw throws try typedef typename union unsigned using val var virtual void volatile when while true false echo function",
    ),
  },
  python: {
    line: ["#"],
    block: [],
    long: [
      ['"""', '"""'],
      ["'''", "'''"],
    ],
    quotes: ['"', "'"],
    keywords: words(
      "and as assert async await break class continue def del elif else except False finally for from global if import in is lambda None nonlocal not or pass raise return True try while with yield match case self extends func var const signal export",
    ),
  },
  ruby: {
    line: ["#"],
    block: [["=begin", "=end"]],
    quotes: ['"', "'"],
    keywords: words(
      "alias and begin break case class def do else elsif end ensure false for if in module next nil not or redo rescue retry return self super then true undef unless until when while yield require puts",
    ),
  },
  shell: {
    line: ["#"],
    block: [["<#", "#>"]],
    quotes: ['"', "'"],
    multiline: ['"', "'"],
    caseless: true,
    keywords: words(
      "if then else elif fi case esac for while until do done in function return exit export local readonly echo set unset source param begin process end foreach try catch finally throw",
    ),
  },
  batch: {
    line: ["::", "REM ", "rem ", "Rem "],
    block: [],
    quotes: ['"'],
    caseless: true,
    keywords: words("echo set if else goto call exit for in do not exist defined errorlevel setlocal endlocal off on"),
  },
  sql: {
    line: ["--"],
    block: [["/*", "*/"]],
    quotes: ["'", '"'],
    multiline: ["'"],
    caseless: true,
    keywords: words(
      "select from where and or not insert into values update set delete create table index view drop alter add primary key foreign references join left right inner outer full on group by order having limit offset as distinct union all case when then else end null is in like between exists default unique check constraint begin commit rollback with returning",
    ),
  },
  lua: {
    line: ["--"],
    block: [["--[[", "]]"]],
    long: [["[[", "]]"]],
    quotes: ['"', "'"],
    keywords: words(
      "and break do else elseif end false for function goto if in local nil not or repeat return then true until while",
    ),
  },
  css: {
    line: [],
    block: [["/*", "*/"]],
    quotes: ['"', "'"],
    word: /[@A-Za-z_-][\w-]*/y,
    keyBefore: /[ \t]*:(?!:)/y,
    keywords: words("@media @import @font-face @keyframes @supports @layer @container @charset @use @include @mixin important"),
  },
  json: {
    ...C_COMMENTS,
    quotes: ['"'],
    keyBefore: /\s*:/y,
    keywords: words("true false null"),
  },
  yaml: {
    line: ["#"],
    block: [],
    quotes: ['"', "'"],
    word: /[A-Za-z_][\w.-]*/y,
    keyBefore: /[ \t]*:(?=\s|$)/y,
    keywords: words("true false null yes no on off"),
  },
  toml: {
    line: ["#"],
    block: [],
    long: [
      ['"""', '"""'],
      ["'''", "'''"],
    ],
    quotes: ['"', "'"],
    word: /[A-Za-z_][\w.-]*/y,
    keyBefore: /[ \t]*=/y,
    keywords: words("true false"),
  },
  verse: {
    line: ["#"],
    block: [["<#", "#>"]],
    quotes: ['"'],
    keywords: words(
      "using module class struct interface enum if then else for loop return var set block case break logic int float string void true false spawn sync race rush branch defer suspends transacts decides computes converges reads writes allocates public private protected internal override final abstract where of at and or not external option",
    ),
  },
};

const NUMBER = /0[xX][\da-fA-F_]+|\d[\d_]*(?:\.\d[\d_]*)?(?:[eE][+-]?\d+)?/y;
const WORD = /[A-Za-z_$][\w$]*/y;
const isWordChar = (c: string | undefined) => c !== undefined && /[\w$]/.test(c);

/** The source cut into coloured pieces; put end to end, they are the source. */
export function highlight(source: string, language: Language): Token[] {
  if (language === "plain") return source ? [{ kind: "plain", text: source }] : [];
  const tokens = new Tokens();
  if (language === "markup") scanMarkup(source, tokens);
  else scanCode(source, GRAMMARS[language], tokens);
  return tokens.done();
}

/** Pieces, plain ones run together: fewer to draw. */
class Tokens {
  private list: Token[] = [];
  private plain = "";

  push(kind: TokenKind, text: string) {
    if (!text) return;
    if (kind === "plain") {
      this.plain += text;
      return;
    }
    this.flush();
    this.list.push({ kind, text });
  }

  done(): Token[] {
    this.flush();
    return this.list;
  }

  private flush() {
    if (this.plain) this.list.push({ kind: "plain", text: this.plain });
    this.plain = "";
  }
}

/** Where what opened at `from` closes, past `close`; the end of the source if it never does. */
function closing(source: string, from: number, close: string): number {
  const at = source.indexOf(close, from);
  return at < 0 ? source.length : at + close.length;
}

/** Past a string opened by `quote` at `start`: an escaped quote does not close it, a line's end does unless it may span lines. */
function endOfString(source: string, start: number, quote: string, multiline: boolean): number {
  for (let at = start + 1; at < source.length; at++) {
    const c = source[at];
    if (c === "\\") at++;
    else if (c === quote) return at + 1;
    else if (c === "\n" && !multiline) return at;
  }
  return source.length;
}

function scanCode(source: string, grammar: Grammar, tokens: Tokens) {
  const word = grammar.word ?? WORD;
  const isKey = (at: number) => {
    if (!grammar.keyBefore) return false;
    grammar.keyBefore.lastIndex = at;
    return grammar.keyBefore.test(source);
  };
  let at = 0;
  outer: while (at < source.length) {
    const c = source[at];
    for (const [open, close] of grammar.block) {
      if (source.startsWith(open, at)) {
        const end = closing(source, at + open.length, close);
        tokens.push("comment", source.slice(at, end));
        at = end;
        continue outer;
      }
    }
    for (const open of grammar.line) {
      if (!source.startsWith(open, at)) continue;
      if (open === "#" && at > 0 && !/\s/.test(source[at - 1])) continue;
      const end = source.indexOf("\n", at);
      const stop = end < 0 ? source.length : end;
      tokens.push("comment", source.slice(at, stop));
      at = stop;
      continue outer;
    }
    for (const [open, close] of grammar.long ?? []) {
      if (source.startsWith(open, at)) {
        const end = closing(source, at + open.length, close);
        tokens.push("string", source.slice(at, end));
        at = end;
        continue outer;
      }
    }
    if (grammar.quotes.includes(c)) {
      const end = endOfString(source, at, c, grammar.multiline?.includes(c) ?? false);
      tokens.push(isKey(end) ? "attr" : "string", source.slice(at, end));
      at = end;
      continue;
    }
    if (/\d/.test(c) && !isWordChar(source[at - 1])) {
      NUMBER.lastIndex = at;
      const number = NUMBER.exec(source);
      if (number) {
        tokens.push("number", number[0]);
        at += number[0].length;
        continue;
      }
    }
    word.lastIndex = at;
    const name = word.exec(source)?.[0];
    if (name) {
      const keyword = grammar.keywords.has(grammar.caseless ? name.toLowerCase() : name);
      tokens.push(keyword ? "keyword" : isKey(at + name.length) ? "attr" : "plain", name);
      at += name.length;
      continue;
    }
    tokens.push("plain", c);
    at++;
  }
}

const TAG_OPEN = /<\/?[!?]?[\w:.-]*/y;
const ATTRIBUTE = /[^\s=>/"']+/y;
const UNQUOTED = /[^\s>]+/y;

/** HTML, XML and SVG: comments, tags and their attributes; text between them is plain. */
function scanMarkup(source: string, tokens: Tokens) {
  let at = 0;
  while (at < source.length) {
    if (source.startsWith("<!--", at)) {
      const end = closing(source, at + 4, "-->");
      tokens.push("comment", source.slice(at, end));
      at = end;
      continue;
    }
    if (source[at] !== "<" || !/[A-Za-z/!?]/.test(source[at + 1] ?? "")) {
      const next = source.indexOf("<", at + 1);
      const end = next < 0 ? source.length : next;
      tokens.push("plain", source.slice(at, end));
      at = end;
      continue;
    }
    TAG_OPEN.lastIndex = at;
    const open = TAG_OPEN.exec(source)![0];
    tokens.push("tag", open);
    at += open.length;
    // Its attributes, to the `>` that closes it.
    while (at < source.length) {
      const c = source[at];
      if (/\s/.test(c)) {
        tokens.push("plain", c);
        at++;
      } else if (c === ">" || source.startsWith("/>", at) || source.startsWith("?>", at)) {
        const close = c === ">" ? ">" : source.slice(at, at + 2);
        tokens.push("tag", close);
        at += close.length;
        break;
      } else if (c === '"' || c === "'") {
        const end = endOfString(source, at, c, true);
        tokens.push("string", source.slice(at, end));
        at = end;
      } else if (c === "=") {
        tokens.push("plain", c);
        at++;
        if (source[at] !== '"' && source[at] !== "'") {
          UNQUOTED.lastIndex = at;
          const value = UNQUOTED.exec(source)?.[0] ?? "";
          tokens.push("string", value);
          at += value.length;
        }
      } else {
        ATTRIBUTE.lastIndex = at;
        const name = ATTRIBUTE.exec(source)?.[0] ?? c;
        tokens.push("attr", name);
        at += name.length;
      }
    }
  }
}
