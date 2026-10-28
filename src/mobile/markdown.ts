/**
 * Claude's markdown as a tree the chat draws with React: headings, paragraphs,
 * lists (nested, numbered, task lists), fenced code, quotes, tables, rules;
 * inline code, bold, italics, strikethrough and links.
 *
 * A renderer of its own rather than a library: the few forms Claude writes
 * are all it needs, and the tree only ever holds text, so nothing in a
 * message can reach the page as HTML. Raw HTML stays as the text it is;
 * a link keeps its address only when it is http(s) or mailto. A line break
 * inside a paragraph stays a line break, as Claude Code shows it.
 */

export type Inline =
  | { type: "text"; text: string }
  | { type: "code"; text: string }
  | { type: "strong" | "em" | "del"; children: Inline[] }
  | { type: "link"; href: string; children: Inline[] }
  | { type: "break" };

export type Align = "left" | "center" | "right" | null;

export interface ListItem {
  /** A task list's box: checked or not; null for a plain item. */
  task: boolean | null;
  children: Block[];
}

export type Block =
  | { type: "heading"; level: number; children: Inline[] }
  | { type: "paragraph"; children: Inline[] }
  | { type: "code"; lang: string; text: string }
  | { type: "quote"; children: Block[] }
  | { type: "list"; ordered: boolean; start: number; items: ListItem[] }
  | { type: "table"; align: Align[]; head: Inline[][]; rows: Inline[][][] }
  | { type: "rule" };

export function parseMarkdown(source: string): Block[] {
  return parseBlocks(source.replace(/\r\n?/g, "\n").replace(/\t/g, "    ").split("\n"));
}

/* ── Blocks ────────────────────────────────────────────────────── */

const FENCE = /^ {0,3}(`{3,}|~{3,})\s*([^\s`]*)[^`]*$/;
const HEADING = /^ {0,3}(#{1,6})(?:[ \t]+(.*?))?(?:[ \t]+#+)?[ \t]*$/;
const RULE = /^ {0,3}([-*_])(?:[ \t]*\1){2,}[ \t]*$/;
const QUOTE = /^ {0,3}> ?(.*)$/;
const LIST_ITEM = /^( *)([-*+]|\d{1,9}[.)])(?:( +)(.*))?$/;
const TABLE_DELIMITER = /^ {0,3}\|?[ \t]*:?-+:?[ \t]*(?:\|[ \t]*:?-+:?[ \t]*)*\|?[ \t]*$/;

const indentOf = (line: string) => line.length - line.trimStart().length;
const blank = (line: string) => line.trim() === "";

interface Marker {
  indent: number;
  ordered: boolean;
  start: number;
  /** Where the item's text starts: what its continuation lines are indented by. */
  contentIndent: number;
  text: string;
}

function listMarker(line: string): Marker | null {
  const match = LIST_ITEM.exec(line);
  if (!match) return null;
  const [, spaces, bullet, gap = "", text = ""] = match;
  // A marker alone on its line opens an item only before something.
  if (!gap && text === "") return null;
  const ordered = /\d/.test(bullet);
  const indent = spaces.length;
  // Five spaces or more after the marker: the text starts after one.
  const width = gap.length > 4 ? 1 : Math.max(1, gap.length);
  return {
    indent,
    ordered,
    start: ordered ? Number.parseInt(bullet, 10) : 1,
    contentIndent: indent + bullet.length + width,
    text: gap.length > 4 ? `${gap.slice(1)}${text}` : text,
  };
}

const isTableStart = (lines: string[], i: number) =>
  lines[i].includes("|") && i + 1 < lines.length && lines[i + 1].includes("|") && TABLE_DELIMITER.test(lines[i + 1]);

/** Whether `line` starts a block of its own, ending a paragraph above it. */
function startsBlock(lines: string[], i: number): boolean {
  const line = lines[i];
  return (
    FENCE.test(line) ||
    HEADING.test(line) ||
    RULE.test(line) ||
    QUOTE.test(line) ||
    listMarker(line) !== null ||
    isTableStart(lines, i)
  );
}

function parseBlocks(lines: string[]): Block[] {
  const blocks: Block[] = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    if (blank(line)) {
      i++;
      continue;
    }

    const fence = FENCE.exec(line);
    if (fence) {
      const [, marker, lang] = fence;
      const indent = indentOf(line);
      const closing = new RegExp(`^ {0,3}${marker[0] === "`" ? "`" : "~"}{${marker.length},}[ \\t]*$`);
      const body: string[] = [];
      i++;
      while (i < lines.length && !closing.test(lines[i])) {
        body.push(lines[i].slice(Math.min(indent, indentOf(lines[i]))));
        i++;
      }
      i++;
      blocks.push({ type: "code", lang, text: body.join("\n") });
      continue;
    }

    const heading = HEADING.exec(line);
    if (heading) {
      blocks.push({ type: "heading", level: heading[1].length, children: parseInline(heading[2] ?? "") });
      i++;
      continue;
    }

    if (RULE.test(line)) {
      blocks.push({ type: "rule" });
      i++;
      continue;
    }

    if (QUOTE.test(line)) {
      const inner: string[] = [];
      while (i < lines.length && !blank(lines[i])) {
        const quoted = QUOTE.exec(lines[i]);
        // A line without its ">" carries the quote's paragraph on.
        if (!quoted && startsBlock(lines, i)) break;
        inner.push(quoted ? quoted[1] : lines[i]);
        i++;
      }
      blocks.push({ type: "quote", children: parseBlocks(inner) });
      continue;
    }

    const marker = listMarker(line);
    if (marker) {
      i = parseList(lines, i, marker, blocks);
      continue;
    }

    if (isTableStart(lines, i)) {
      i = parseTable(lines, i, blocks);
      continue;
    }

    const paragraph: string[] = [];
    while (i < lines.length && !blank(lines[i]) && (paragraph.length === 0 || !startsBlock(lines, i))) {
      paragraph.push(lines[i].trim());
      i++;
    }
    blocks.push({ type: "paragraph", children: parseInline(paragraph.join("\n")) });
  }
  return blocks;
}

/**
 * A list from line `i`: its items and what each holds, nested lists included.
 * Lenient as a chat needs: any item indented past this list's marker nests
 * in the item above, whatever the exact indent.
 */
function parseList(lines: string[], i: number, first: Marker, blocks: Block[]): number {
  const items: ListItem[] = [];
  let marker: Marker | null = first;
  while (marker) {
    const body = [marker.text];
    const contentIndent: number = marker.contentIndent;
    i++;
    let lastBlank = false;
    while (i < lines.length) {
      const line = lines[i];
      if (blank(line)) {
        lastBlank = true;
        body.push("");
        i++;
        continue;
      }
      const indent = indentOf(line);
      const nested = listMarker(line);
      if (indent >= contentIndent || (nested && indent > first.indent)) {
        body.push(line.slice(Math.min(indent, contentIndent)));
        lastBlank = false;
        i++;
        continue;
      }
      // A line of the item's paragraph that lost its indent.
      if (!lastBlank && !startsBlock(lines, i)) {
        body.push(line.trim());
        i++;
        continue;
      }
      break;
    }
    while (body.length && blank(body[body.length - 1])) body.pop();
    const task = /^\[([ xX])\][ \t]+/.exec(body[0]);
    if (task) body[0] = body[0].slice(task[0].length);
    items.push({ task: task ? task[1] !== " " : null, children: parseBlocks(body) });

    const next = i < lines.length ? listMarker(lines[i]) : null;
    marker = next && next.ordered === first.ordered && next.indent <= first.indent + 3 ? next : null;
  }
  blocks.push({ type: "list", ordered: first.ordered, start: first.start, items });
  return i;
}

/** A row's cells, split on the pipes outside code and escapes. */
function cells(line: string): string[] {
  const row = line.trim().replace(/^\|/, "");
  const out: string[] = [];
  let cell = "";
  let ticks = 0;
  for (let at = 0; at < row.length; at++) {
    const char = row[at];
    if (char === "\\" && row[at + 1] === "|") {
      cell += "|";
      at++;
    } else if (char === "`") {
      ticks = ticks ? 0 : 1;
      cell += char;
    } else if (char === "|" && !ticks) {
      out.push(cell.trim());
      cell = "";
    } else {
      cell += char;
    }
  }
  if (cell.trim() || !row.endsWith("|")) out.push(cell.trim());
  return out;
}

function parseTable(lines: string[], i: number, blocks: Block[]): number {
  const head = cells(lines[i]);
  const align: Align[] = cells(lines[i + 1]).map((cell) => {
    const left = cell.startsWith(":");
    const right = cell.endsWith(":");
    return left && right ? "center" : right ? "right" : left ? "left" : null;
  });
  i += 2;
  const rows: Inline[][][] = [];
  while (i < lines.length && !blank(lines[i]) && lines[i].includes("|")) {
    const row = cells(lines[i]);
    rows.push(head.map((_, column) => parseInline(row[column] ?? "")));
    i++;
  }
  blocks.push({
    type: "table",
    align: head.map((_, column) => align[column] ?? null),
    head: head.map((cell) => parseInline(cell)),
    rows,
  });
  return i;
}

/* ── Inline ────────────────────────────────────────────────────── */

const PUNCTUATION = /[!"#$%&'()*+,\-./:;<=>?@[\\\]^_`{|}~]/;
const SAFE_HREF = /^(https?:\/\/|mailto:)/i;
const BARE_URL = /^https?:\/\/[^\s<>]+/i;
const WORD = /[\p{L}\p{N}]/u;

/** The address a link may keep: http(s) and mailto only. */
export function safeHref(href: string): string | null {
  const trimmed = href.trim();
  return SAFE_HREF.test(trimmed) ? trimmed : null;
}

/** Where the `]` matching the `[` at `open` is, brackets nested; -1 without one. */
function closingBracket(text: string, open: number): number {
  let depth = 0;
  for (let at = open; at < text.length; at++) {
    const char = text[at];
    if (char === "\\") at++;
    else if (char === "[") depth++;
    else if (char === "]" && --depth === 0) return at;
  }
  return -1;
}

/** A link's `(address "title")` from `open`: the address and where it ends. */
function destination(text: string, open: number): { href: string; end: number } | null {
  if (text[open] !== "(") return null;
  let depth = 0;
  for (let at = open; at < text.length; at++) {
    const char = text[at];
    if (char === "\\") at++;
    else if (char === "(") depth++;
    else if (char === ")" && --depth === 0) {
      const inside = text.slice(open + 1, at).trim();
      const href = inside.startsWith("<") ? inside.slice(1, inside.indexOf(">")) : inside.split(/\s+/)[0] ?? "";
      return { href, end: at + 1 };
    }
  }
  return null;
}

/** Trailing punctuation that ends a sentence, not the address. */
function trimUrl(url: string): string {
  let end = url.length;
  while (end > 0) {
    const char = url[end - 1];
    if (/[.,;:!?'"*_~]/.test(char)) end--;
    else if (char === ")" && (url.slice(0, end).match(/\(/g)?.length ?? 0) < (url.slice(0, end).match(/\)/g)?.length ?? 0)) end--;
    else break;
  }
  return url.slice(0, end);
}

/**
 * Where the emphasis opened by `delimiter` at `from` closes: the next run of
 * it that follows text, not space; for `_`, one that ends a word.
 */
function closingDelimiter(text: string, delimiter: string, from: number): number {
  const char = delimiter[0];
  for (let at = from; at < text.length; at++) {
    if (text[at] === "\\") {
      at++;
      continue;
    }
    if (text[at] === "`") {
      const run = /^`+/.exec(text.slice(at))![0];
      const close = text.indexOf(run, at + run.length);
      if (close !== -1) at = close + run.length - 1;
      continue;
    }
    if (!text.startsWith(delimiter, at)) continue;
    let run = 0;
    while (text[at + run] === char) run++;
    // A single delimiter skips a double one, which is another emphasis.
    if (delimiter.length === 1 && run === 2) {
      at += 1;
      continue;
    }
    const before = text[at - 1];
    const after = text[at + delimiter.length];
    if (at > from && before !== undefined && !/\s/.test(before) && !(char === "_" && after !== undefined && WORD.test(after))) {
      return at;
    }
    at += run - 1;
  }
  return -1;
}

function opens(text: string, at: number, delimiter: string): boolean {
  if (!text.startsWith(delimiter, at)) return false;
  const after = text[at + delimiter.length];
  if (after === undefined || /\s/.test(after)) return false;
  // snake_case stays as it is.
  if (delimiter[0] === "_" && at > 0 && WORD.test(text[at - 1])) return false;
  return true;
}

const EMPHASIS: { delimiter: string; type: "strong" | "em" | "del" }[] = [
  { delimiter: "**", type: "strong" },
  { delimiter: "__", type: "strong" },
  { delimiter: "~~", type: "del" },
  { delimiter: "*", type: "em" },
  { delimiter: "_", type: "em" },
];

export function parseInline(text: string): Inline[] {
  const out: Inline[] = [];
  let plain = "";
  const flush = () => {
    if (plain) out.push({ type: "text", text: plain });
    plain = "";
  };
  const push = (node: Inline) => {
    flush();
    out.push(node);
  };

  let at = 0;
  outer: while (at < text.length) {
    const char = text[at];

    if (char === "\\" && at + 1 < text.length && PUNCTUATION.test(text[at + 1])) {
      plain += text[at + 1];
      at += 2;
      continue;
    }

    if (char === "\n") {
      push({ type: "break" });
      at++;
      continue;
    }

    if (char === "`") {
      const run = /^`+/.exec(text.slice(at))![0];
      const close = text.indexOf(run, at + run.length);
      // The closing run is exactly as long as the opening one.
      if (close !== -1 && text[close + run.length] !== "`") {
        let code = text.slice(at + run.length, close).replace(/\n/g, " ");
        if (code.startsWith(" ") && code.endsWith(" ") && code.trim()) code = code.slice(1, -1);
        push({ type: "code", text: code });
        at = close + run.length;
        continue;
      }
      plain += run;
      at += run.length;
      continue;
    }

    if (char === "<") {
      const auto = /^<((?:https?:\/\/|mailto:)[^\s<>]+)>/i.exec(text.slice(at));
      if (auto) {
        push({ type: "link", href: auto[1], children: [{ type: "text", text: auto[1].replace(/^mailto:/i, "") }] });
        at += auto[0].length;
        continue;
      }
    }

    if (char === "[" || (char === "!" && text[at + 1] === "[")) {
      const open = char === "!" ? at + 1 : at;
      const close = closingBracket(text, open);
      const target = close === -1 ? null : destination(text, close + 1);
      if (target) {
        const label = text.slice(open + 1, close);
        const href = safeHref(target.href);
        // An image is never fetched: its description, linked when it may be.
        const children = char === "!" ? [{ type: "text", text: label || target.href } as Inline] : parseInline(label);
        if (href) push({ type: "link", href, children });
        else {
          flush();
          out.push(...children);
        }
        at = target.end;
        continue;
      }
    }

    if ((char === "h" || char === "H") && (at === 0 || !WORD.test(text[at - 1]))) {
      const bare = BARE_URL.exec(text.slice(at));
      if (bare) {
        const url = trimUrl(bare[0]);
        push({ type: "link", href: url, children: [{ type: "text", text: url }] });
        at += url.length;
        continue;
      }
    }

    for (const { delimiter, type } of EMPHASIS) {
      if (!opens(text, at, delimiter)) continue;
      const close = closingDelimiter(text, delimiter, at + delimiter.length);
      if (close === -1) continue;
      push({ type, children: parseInline(text.slice(at + delimiter.length, close)) });
      at = close + delimiter.length;
      continue outer;
    }

    plain += char;
    at++;
  }
  flush();
  // An unsafe link's words join the text around them.
  return out.reduce<Inline[]>((merged, node) => {
    const last = merged[merged.length - 1];
    if (node.type === "text" && last?.type === "text") merged[merged.length - 1] = { type: "text", text: last.text + node.text };
    else merged.push(node);
    return merged;
  }, []);
}

/** The text of inline nodes, without their marks. */
export function plainText(nodes: Inline[]): string {
  return nodes
    .map((node) => {
      switch (node.type) {
        case "text":
        case "code":
          return node.text;
        case "break":
          return "\n";
        default:
          return plainText(node.children);
      }
    })
    .join("");
}
