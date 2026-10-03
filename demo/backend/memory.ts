/**
 * The global memory, in memory.
 *
 * The real one is a single memory.json for the whole app, written by agents
 * over MCP and by the user in the Memory view. Here it is a list seeded with
 * the kind of facts agents leave behind, and it resets with the page. The
 * rules are the ones in src-tauri/src/memory.rs: creation is an upsert on the
 * title, a rename onto a taken title is refused, and every write raises
 * `memory:changed`.
 */
import { dispatch } from "../mock/event";
import type { Memory, MemoryEntry, MemoryPatch, NewMemory } from "../../src/lib/memory";

const MAX_ENTRIES = 1000;
const MAX_TITLE_CHARS = 200;
const MAX_CONTENT_CHARS = 20_000;
const MAX_TAGS = 10;

const HANGAR = "C:\\dev\\hangar-ai";
const STORE = "C:\\dev\\storefront";

let serial = 0;
const hours = (n: number) => Date.now() - n * 3600_000;

function entry(
  title: string,
  content: string,
  tags: string[],
  workspace: string,
  agent: string,
  ageHours: number,
): MemoryEntry {
  return {
    id: `mem-${++serial}`,
    title,
    content,
    tags,
    workspace,
    agent,
    created_at: hours(ageHours * 3),
    updated_at: hours(ageHours),
  };
}

const entries: MemoryEntry[] = [
  entry(
    "Board claims go through one lock",
    "Every board write in src-tauri/src/board.rs takes the same mutex, so two agents racing for a task cannot both win. Never read board.json, change it and write it back from outside that lock: the MCP tools and the HTTP routes already do it right.",
    ["rust", "board", "concurrency"],
    HANGAR,
    "claude",
    2,
  ),
  entry(
    "WSL panes need wslpath on the way in",
    "A Windows cwd handed to wsl.exe opens in the Linux home instead. Convert it with `wslpath -u` before spawning, and back with `wslpath -w` for anything the Windows side reads.",
    ["wsl", "pty", "windows"],
    HANGAR,
    "claude",
    5,
  ),
  entry(
    "Run cargo fmt before pushing",
    "CI runs `cargo fmt --check` and fails the whole matrix on one misplaced brace. The release workflow skips the check, so a release can go out with code CI would have rejected.",
    ["ci", "rust"],
    HANGAR,
    "codex",
    9,
  ),
  entry(
    "i18n: every string in en and fr",
    "User-facing text lives in src/i18n.ts, one key per string, with both languages filled in the same change. A key missing in fr falls back to en silently, so check both tables by hand.",
    ["i18n", "frontend", "convention"],
    HANGAR,
    "gemini",
    20,
  ),
  entry(
    "Prices are integers in cents",
    "Every amount in the cart and the checkout is an integer number of cents, with the currency next to it. Never add two amounts in different currencies: convert first, at the rate stored on the order.",
    ["money", "checkout", "convention"],
    STORE,
    "claude",
    30,
  ),
  entry(
    "Product images come from the CDN, never /public",
    "The grid reads image URLs from the catalog API. The files in /public are placeholders for local dev and are not deployed, so a path to them works on your machine and breaks in production.",
    ["frontend", "images"],
    STORE,
    "user",
    52,
  ),
];

/** Same event the Rust side raises, so MemoryView refreshes on its own. */
function changed() {
  dispatch("memory:changed", "");
}

const titleKey = (title: string) => title.trim().toLowerCase();

function cleanTitle(title: string): string {
  const trimmed = (title ?? "").trim();
  if (!trimmed) throw new Error("title must not be empty");
  const length = [...trimmed].length;
  if (length > MAX_TITLE_CHARS) {
    throw new Error(`title is ${length} characters, the limit is ${MAX_TITLE_CHARS}`);
  }
  return trimmed;
}

function cleanContent(content: string): string {
  const trimmed = (content ?? "").trim();
  if (!trimmed) throw new Error("content must not be empty");
  const length = [...trimmed].length;
  if (length > MAX_CONTENT_CHARS) {
    throw new Error(
      `content is ${length} characters, the limit is ${MAX_CONTENT_CHARS}; split it into several entries`,
    );
  }
  return trimmed;
}

function cleanTags(tags: string[] = []): string[] {
  const kept: string[] = [];
  for (const tag of tags) {
    const trimmed = tag.trim();
    if (!trimmed) continue;
    if (kept.some((seen) => seen.toLowerCase() === trimmed.toLowerCase())) continue;
    kept.push(trimmed);
  }
  if (kept.length > MAX_TAGS) throw new Error(`${kept.length} tags, the limit is ${MAX_TAGS}`);
  return kept;
}

export function load(): Memory {
  return { entries: entries.map((e) => ({ ...e, tags: [...e.tags] })) };
}

/** An upsert on the title, like the real store: re-learning a fact updates it. */
export function create(input: NewMemory): MemoryEntry {
  const title = cleanTitle(input.title);
  const content = cleanContent(input.content);
  const tags = cleanTags(input.tags);
  const key = titleKey(title);
  const stamp = Date.now();

  const existing = entries.find((e) => titleKey(e.title) === key);
  if (existing) {
    existing.title = title;
    existing.content = content;
    existing.tags = tags;
    if (input.workspace) existing.workspace = input.workspace;
    if (input.agent) existing.agent = input.agent;
    existing.updated_at = stamp;
    changed();
    return { ...existing };
  }

  if (entries.length >= MAX_ENTRIES) {
    throw new Error(
      `memory holds ${MAX_ENTRIES} entries, which is the limit; delete something before writing more`,
    );
  }

  const created: MemoryEntry = {
    id: `mem-${++serial}`,
    title,
    content,
    tags,
    workspace: input.workspace ?? "",
    agent: input.agent ?? "",
    created_at: stamp,
    updated_at: stamp,
  };
  entries.push(created);
  changed();
  return { ...created };
}

export function update(id: string, patch: MemoryPatch): MemoryEntry {
  const title = patch.title !== undefined ? cleanTitle(patch.title) : undefined;
  const content = patch.content !== undefined ? cleanContent(patch.content) : undefined;
  const tags = patch.tags !== undefined ? cleanTags(patch.tags) : undefined;

  if (title !== undefined) {
    const key = titleKey(title);
    if (entries.some((e) => e.id !== id && titleKey(e.title) === key)) {
      throw new Error(`another entry is already titled '${title}'`);
    }
  }

  const found = entries.find((e) => e.id === id);
  if (!found) throw new Error("memory entry not found");
  if (title !== undefined) found.title = title;
  if (content !== undefined) found.content = content;
  if (tags !== undefined) found.tags = tags;
  found.updated_at = Date.now();
  changed();
  return { ...found };
}

export function remove(id: string): void {
  const at = entries.findIndex((e) => e.id === id);
  if (at < 0) throw new Error("memory entry not found");
  entries.splice(at, 1);
  changed();
}
