import { invoke } from "@tauri-apps/api/core";
import { openPath, revealItemInDir } from "@tauri-apps/plugin-opener";
import { modelUrl } from "./modelFiles";

/**
 * The files the preview shows besides 3D models: what Rust says a file is
 * (`preview_inspect`, src-tauri), which viewer draws each kind, and the
 * decisions the viewers make before they draw anything — how much of a text
 * file to read, which language colours it, whether JSON is laid out again,
 * where a Markdown image or link leads.
 *
 * Nothing here renders: components/preview does, and loads its highlighter
 * and Markdown parser on demand. This module stays light enough for the
 * control handlers (lib/control) to import.
 *
 * Every file is served by the same `hangar-model` scheme as the models
 * (modelUrl), from the same allowed folders; text kinds come as text/plain,
 * and an HTML page is sandboxed by the scheme's own CSP as well as by its
 * frame.
 */

export const PREVIEW_KINDS = [
  "model",
  "image",
  "video",
  "audio",
  "text",
  "markdown",
  "pdf",
  "html",
] as const;
export type PreviewKind = (typeof PREVIEW_KINDS)[number];
/** Everything but a 3D model, which has a store entry of its own shape. */
export type FileKind = Exclude<PreviewKind, "model">;

/** What `preview_inspect` answers. */
export interface PreviewFile {
  /** Canonical absolute path, as Rust resolved it. */
  path: string;
  kind: PreviewKind;
  /** The exact Content-Type the file is served with: `text/plain; charset=utf-8` for text. */
  mime: string;
  size: number;
  /** The file's name, without its folder. */
  name: string;
  /** Epoch milliseconds: a newer one means the file was rewritten, and is read again. */
  modifiedMs: number;
}

export const isPreviewKind = (value: unknown): value is PreviewKind =>
  typeof value === "string" && (PREVIEW_KINDS as readonly string[]).includes(value);

/** Rust's verdict on a file: what it is, or why the preview may not open it. */
export const previewInspect = (path: string) => invoke<unknown>("preview_inspect", { path });

/**
 * `preview_inspect`'s answer, checked: a kind this window has no viewer for
 * is refused here rather than drawn as an empty tab. The message is read by
 * an agent.
 */
export function checkPreviewFile(answer: unknown): PreviewFile {
  const raw = (answer ?? {}) as Record<string, unknown>;
  const path = typeof raw.path === "string" ? raw.path : "";
  const name =
    typeof raw.name === "string" && raw.name ? raw.name : path.split(/[\\/]/).pop() || path;
  if (!path) throw new Error("The preview got no path back for that file. Try again.");
  if (!isPreviewKind(raw.kind)) {
    throw new Error(
      `The preview has no viewer for ${name}${typeof raw.kind === "string" ? ` (${raw.kind})` : ""}. ` +
        `It shows: ${PREVIEW_KINDS.join(", ")}.`,
    );
  }
  const count = (value: unknown) => (typeof value === "number" && value >= 0 ? value : 0);
  return {
    path,
    kind: raw.kind,
    mime: typeof raw.mime === "string" ? raw.mime : "",
    size: count(raw.size),
    name,
    modifiedMs: count(raw.modifiedMs),
  };
}

/**
 * Where a viewer loads a file from. Shown again, a file has a new revision,
 * and the URL says so: an image or a page the webview already holds would
 * otherwise come back from its own memory, whatever the scheme's no-store.
 * `asText` asks for an HTML page's source, served as plain text.
 */
export function previewUrl(path: string, revision: number, asText = false): string {
  const query = [asText ? "as=text" : null, revision ? `rev=${revision}` : null].filter(Boolean);
  return modelUrl(path) + (query.length ? `?${query.join("&")}` : "");
}

/** The component that draws a kind: video and audio share a player, text and code one view. */
export type PreviewViewer = "model" | "image" | "media" | "code" | "markdown" | "pdf" | "html";

const VIEWERS: Record<PreviewKind, PreviewViewer> = {
  model: "model",
  image: "image",
  video: "media",
  audio: "media",
  text: "code",
  markdown: "markdown",
  pdf: "pdf",
  html: "html",
};

export const viewerOf = (kind: PreviewKind): PreviewViewer => VIEWERS[kind];

/* ── text and code ────────────────────────────────────────────── */

/** How much of a text file is read: its head, so a 2 GB log never lands in the webview. */
export const TEXT_HEAD_BYTES = 256 * 1024;
/** And how many of its lines are drawn, each one a row of the view. */
export const TEXT_HEAD_LINES = 5000;

/**
 * The highlighter's language for a file, by its extension or its whole name;
 * null for plain text, which is drawn as it is rather than guessed at.
 * Every name here is one components/preview/highlight registers.
 */
export function languageOf(path: string): string | null {
  const name = (path.split(/[\\/]/).pop() ?? "").toLowerCase();
  if (NAMES[name]) return NAMES[name];
  const dot = name.lastIndexOf(".");
  if (dot <= 0 && !name.startsWith(".")) return null;
  return EXTENSIONS[name.slice(dot + 1)] ?? null;
}

const NAMES: Record<string, string> = {
  dockerfile: "dockerfile",
  containerfile: "dockerfile",
  makefile: "makefile",
  gnumakefile: "makefile",
  ".bashrc": "bash",
  ".zshrc": "bash",
  ".profile": "bash",
  ".env": "ini",
  ".editorconfig": "ini",
  ".gitconfig": "ini",
};

const EXTENSIONS: Record<string, string> = {
  js: "javascript",
  mjs: "javascript",
  cjs: "javascript",
  jsx: "javascript",
  ts: "typescript",
  mts: "typescript",
  cts: "typescript",
  tsx: "typescript",
  json: "json",
  jsonc: "json",
  json5: "json",
  jsonl: "json",
  ndjson: "json",
  geojson: "json",
  webmanifest: "json",
  html: "xml",
  htm: "xml",
  xhtml: "xml",
  xml: "xml",
  svg: "xml",
  plist: "xml",
  csproj: "xml",
  vue: "xml",
  css: "css",
  scss: "scss",
  less: "less",
  md: "markdown",
  markdown: "markdown",
  mdx: "markdown",
  py: "python",
  pyi: "python",
  pyw: "python",
  rs: "rust",
  go: "go",
  java: "java",
  kt: "kotlin",
  kts: "kotlin",
  swift: "swift",
  c: "c",
  h: "c",
  cpp: "cpp",
  cc: "cpp",
  cxx: "cpp",
  hpp: "cpp",
  hh: "cpp",
  hxx: "cpp",
  cs: "csharp",
  sh: "bash",
  bash: "bash",
  zsh: "bash",
  ps1: "powershell",
  psm1: "powershell",
  psd1: "powershell",
  bat: "dos",
  cmd: "dos",
  yml: "yaml",
  yaml: "yaml",
  toml: "ini",
  ini: "ini",
  cfg: "ini",
  conf: "ini",
  sql: "sql",
  diff: "diff",
  patch: "diff",
  php: "php",
  rb: "ruby",
  lua: "lua",
};

/** A text file as the view draws it: the head that was read, and whether the file goes on past it. */
export interface TextHead {
  text: string;
  truncated: boolean;
}

/**
 * Cuts what was read down to what is drawn. Read short of the whole file, the
 * last line is a piece of one and goes, unless it is the only one — a
 * minified file is all one line. Past TEXT_HEAD_LINES, the rest goes too.
 * Windows line ends and a byte-order mark are not the text's.
 */
export function textHead(read: string, cut: boolean, maxLines = TEXT_HEAD_LINES): TextHead {
  let text = read.replace(/^﻿/, "").replace(/\r\n?/g, "\n");
  let truncated = cut;
  if (cut) {
    const end = text.lastIndexOf("\n");
    if (end > 0) text = text.slice(0, end);
  }
  const lines = text.split("\n");
  if (lines.length > maxLines) {
    text = lines.slice(0, maxLines).join("\n");
    truncated = true;
  }
  return { text, truncated };
}

/** The lines a text is drawn as: a final line break does not open an empty last row. */
export function textLines(text: string): string[] {
  const lines = text.split("\n");
  if (lines.length > 1 && lines[lines.length - 1] === "") lines.pop();
  return lines;
}

/** The whole file's size from a 206's `Content-Range: bytes 0-99/1234`; null without one. */
export function rangeTotal(header: string | null): number | null {
  const total = header?.match(/\/(\d+)\s*$/);
  return total ? Number(total[1]) : null;
}

/**
 * JSON laid out for reading — only when it is all on one line, as a minified
 * file is. A file its author laid out is shown as written, so its line
 * numbers are the file's; JSON Lines is one value per line and stays so.
 * Invalid JSON is shown as it is. The layout moves whitespace only: numbers
 * and strings keep their exact spelling, where a parse and a stringify would
 * round 12345678901234567890.
 */
export function prettyJson(text: string): string | null {
  const body = text.trim();
  if (!body || body.includes("\n") || !/^[[{]/.test(body)) return null;
  try {
    JSON.parse(body);
  } catch {
    return null;
  }
  const indent = (depth: number) => "\n" + "  ".repeat(depth);
  let out = "";
  let depth = 0;
  for (let i = 0; i < body.length; i++) {
    const ch = body[i];
    if (ch === '"') {
      let end = i + 1;
      while (end < body.length && body[end] !== '"') end += body[end] === "\\" ? 2 : 1;
      out += body.slice(i, end + 1);
      i = end;
    } else if (ch === "{" || ch === "[") {
      let next = i + 1;
      while (/\s/.test(body[next] ?? "")) next++;
      if (body[next] === (ch === "{" ? "}" : "]")) {
        out += ch + body[next];
        i = next;
      } else {
        out += ch + indent(++depth);
      }
    } else if (ch === "}" || ch === "]") {
      out += indent(--depth) + ch;
    } else if (ch === ",") {
      out += "," + indent(depth);
    } else if (ch === ":") {
      out += ": ";
    } else if (!/\s/.test(ch)) {
      out += ch;
    }
  }
  return out;
}

const ESCAPES: Record<string, string> = { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" };
export const escapeHtml = (text: string) => text.replace(/[&<>"']/g, (ch) => ESCAPES[ch]);

/**
 * Highlighted HTML cut into lines, each one whole: a span the highlighter
 * left open across a line break — a block comment, a template string — is
 * closed at the end of the line and opened again on the next, so every line
 * can be a row of its own beside its number, wrapped or not. As many lines
 * as the text has line breaks plus one, like `text.split("\n")`.
 */
export function splitHighlighted(html: string): string[] {
  const lines: string[] = [];
  const open: string[] = [];
  let line = "";
  let last = 0;
  const token = /<span[^>]*>|<\/span>|\n/g;
  for (let match = token.exec(html); match; match = token.exec(html)) {
    line += html.slice(last, match.index);
    last = token.lastIndex;
    if (match[0] === "\n") {
      lines.push(line + "</span>".repeat(open.length));
      line = open.join("");
    } else if (match[0] === "</span>") {
      open.pop();
      line += match[0];
    } else {
      open.push(match[0]);
      line += match[0];
    }
  }
  lines.push(line + html.slice(last));
  return lines;
}

/* ── Markdown ─────────────────────────────────────────────────── */

/**
 * Where a Markdown image is loaded from: beside the file for a relative path,
 * through the same scheme; an absolute path on the disk, through the scheme
 * too; a web or inline image as it is. Anything else — `javascript:`,
 * `file:` — loads nothing. The scheme still refuses what is outside the
 * allowed folders.
 */
export function markdownImageSrc(src: string, fileUrl: string): string | null {
  const given = src.trim();
  if (!given) return null;
  if (/^[a-zA-Z]:[\\/]/.test(given) || given.startsWith("\\\\")) return modelUrl(given);
  if (/^https?:\/\//i.test(given)) return given;
  if (given.startsWith("//")) return `https:${given}`;
  if (/^data:image\/(png|gif|jpe?g|webp|avif);/i.test(given)) return given;
  if (/^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(given)) return null;
  try {
    return new URL(given.replace(/\\/g, "/"), fileUrl).href;
  } catch {
    return null;
  }
}

/** What a click on a link of a rendered Markdown file does. */
export type MarkdownLink =
  /** A heading of the same document, by its id. */
  | { kind: "anchor"; id: string }
  /** A web page or an address, opened by the system, never in the app's window. */
  | { kind: "external"; url: string }
  /** Anything else — a file beside it, a script — does nothing. */
  | { kind: "none" };

export function markdownLink(href: string): MarkdownLink {
  const given = href.trim();
  if (given.startsWith("#") && given.length > 1) {
    let id = given.slice(1);
    try {
      id = decodeURIComponent(id);
    } catch {
      // Kept as written.
    }
    return { kind: "anchor", id };
  }
  if (/^(https?:\/\/|mailto:)/i.test(given)) return { kind: "external", url: given };
  return { kind: "none" };
}

/**
 * A heading's id, the way GitHub makes them, so `[see](#setting-up)` in a
 * README finds its heading: lower case, punctuation gone, spaces as dashes.
 * `taken` counts the ids already given, for the -1, -2 of a repeated title.
 */
export function headingId(text: string, taken: Map<string, number>): string {
  const base =
    text
      .trim()
      .toLowerCase()
      .replace(/[^\p{L}\p{N}\s_-]/gu, "")
      .replace(/\s/g, "-") || "section";
  const seen = taken.get(base) ?? 0;
  taken.set(base, seen + 1);
  return seen ? `${base}-${seen}` : base;
}

/* ── images ───────────────────────────────────────────────────── */

export interface Size {
  width: number;
  height: number;
}

/** How an image sits in its stage: its scale, and its centre's offset from the stage's centre, in pixels. */
export interface ImageFrame {
  zoom: number;
  x: number;
  y: number;
}

export const ZOOM_MIN = 0.02;
export const ZOOM_MAX = 32;

/**
 * The zoom that shows the whole image with a margin around it — never past
 * its own pixels: an icon shows at its size, sharp, rather than blown up.
 */
export function fitZoom(image: Size, stage: Size, margin = 16): number {
  if (image.width <= 0 || image.height <= 0) return 1;
  const room = (side: number) => Math.max(1, side - margin * 2);
  const zoom = Math.min(1, room(stage.width) / image.width, room(stage.height) / image.height);
  return Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, zoom));
}

/**
 * `frame` zoomed by `factor` around `point` (from the stage's centre): the
 * spot under the pointer stays under it, as a map does. Held within
 * ZOOM_MIN–ZOOM_MAX.
 */
export function zoomAround(frame: ImageFrame, factor: number, point = { x: 0, y: 0 }): ImageFrame {
  const zoom = Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, frame.zoom * factor));
  const k = zoom / frame.zoom;
  return { zoom, x: point.x - (point.x - frame.x) * k, y: point.y - (point.y - frame.y) * k };
}

/* ── PDF ──────────────────────────────────────────────────────── */

/**
 * Whether the webview draws a PDF itself: WebView2 and WebKit on macOS say so
 * (`navigator.pdfViewerEnabled`); WebKitGTK does not have a viewer, and says
 * nothing or false. Only a yes is taken: a frame the webview cannot fill
 * stays blank or downloads the file.
 */
export const pdfInline = (nav: { pdfViewerEnabled?: boolean } | undefined): boolean =>
  nav?.pdfViewerEnabled === true;

/**
 * Hands a PDF to the app the system opens it with — the one kind Hangar may
 * open that way (`opener:allow-open-path`, scoped to PDF files in
 * src-tauri/capabilities). Refused anyway, it shows the file in its folder
 * instead, and says which it did.
 */
export async function openExternally(path: string): Promise<"opened" | "revealed"> {
  try {
    await openPath(path);
    return "opened";
  } catch {
    await revealItemInDir(path);
    return "revealed";
  }
}

/** Shows the file in the system's file manager, selected: the way out for any other kind. */
export const showInFolder = (path: string) => revealItemInDir(path);

/** A Content-Type without its parameters, for a line that says what a file is. */
export const mimeType = (mime: string) => mime.split(";")[0].trim();

/** The extension `?as=text` gives the source of, besides an HTML page's (src-tauri/preview.rs). */
export const hasSource = (path: string) => /\.svg$/i.test(path);
