import type { Share, ShareKind } from "./api";
import { isImage } from "./model";
import type { Language } from "./highlight";

/**
 * Which viewer a share opens in on the Shares screen, and how much of a text
 * it reads. The PC says what a share is (its `kind`); the phone only picks
 * the viewer for it.
 */

const SHOWN: readonly ShareKind[] = ["image", "video", "audio", "text", "markdown", "pdf", "html"];

/**
 * The share's viewer: its kind when the phone has a viewer for it, `file`
 * (the Open button) for any other. A share kept from before the PC told
 * kinds apart is a picture if it is one, and a file otherwise, as it was.
 */
export function viewerOf(share: Share): ShareKind {
  if (share.kind === undefined) return isImage(share) ? "image" : "file";
  return SHOWN.includes(share.kind) ? share.kind : "file";
}

/**
 * The most of a text the phone reads and draws: a long log or a long file
 * shows its head, with a note, and opens whole with Open.
 */
export const TEXT_HEAD_BYTES = 128 * 1024;

const LANGUAGES: Record<string, Language> = {
  ts: "js",
  tsx: "js",
  js: "js",
  jsx: "js",
  mjs: "js",
  cjs: "js",
  rs: "rust",
  go: "go",
  java: "clike",
  kt: "clike",
  c: "clike",
  h: "clike",
  cpp: "clike",
  hpp: "clike",
  cs: "clike",
  swift: "clike",
  dart: "clike",
  php: "clike",
  py: "python",
  gd: "python",
  rb: "ruby",
  sh: "shell",
  bash: "shell",
  zsh: "shell",
  ps1: "shell",
  bat: "batch",
  cmd: "batch",
  sql: "sql",
  lua: "lua",
  css: "css",
  scss: "css",
  json: "json",
  jsonc: "json",
  yml: "yaml",
  yaml: "yaml",
  toml: "toml",
  html: "markup",
  htm: "markup",
  xml: "markup",
  svg: "markup",
  vue: "markup",
  svelte: "markup",
  verse: "verse",
};

/** The language a file's name says it is written in; plain for prose, logs and data. */
export function languageOf(name: string): Language {
  const extension = /\.([a-z0-9]+)$/i.exec(name)?.[1]?.toLowerCase() ?? "";
  return LANGUAGES[extension] ?? "plain";
}

/** Prose and logs read wrapped; code keeps its lines, and scrolls sideways. */
export const wrapsByDefault = (language: Language) => language === "plain";

/** What the file type badge says: the extension, or nothing. */
export const extensionOf = (name: string) => /\.([a-z0-9]{1,5})$/i.exec(name)?.[1]?.toUpperCase() ?? "";
