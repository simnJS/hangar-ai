import { useCallback, useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  MEMORY_CHANGED_EVENT,
  memoryCreate,
  memoryDelete,
  memoryLoad,
  memoryUpdate,
  type MemoryEntry,
} from "../lib/memory";
import { useLocale, useT } from "../i18n";

interface Props {
  /** Active workspace: the origin stamped on what is written from here. */
  cwd: string;
}

interface Draft {
  title: string;
  content: string;
  tags: string;
}

const EMPTY_DRAFT: Draft = { title: "", content: "", tags: "" };

/** Last segment of a path — how a project is recognised at a glance. */
function shortWorkspace(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

/** Free text in, tag list out; duplicates and blanks dropped. */
function parseTags(raw: string): string[] {
  const seen = new Set<string>();
  const tags: string[] = [];
  for (const part of raw.split(",")) {
    const tag = part.trim();
    const key = tag.toLowerCase();
    if (!tag || seen.has(key)) continue;
    seen.add(key);
    tags.push(tag);
  }
  return tags;
}

/** Same rule as the MCP search: every term has to appear somewhere. */
function matches(entry: MemoryEntry, terms: string[]): boolean {
  if (!terms.length) return true;
  const haystack =
    `${entry.title}\n${entry.content}\n${entry.tags.join(" ")}`.toLowerCase();
  return terms.every((term) => haystack.includes(term));
}

export function MemoryView({ cwd }: Props) {
  const [entries, setEntries] = useState<MemoryEntry[]>([]);
  const [query, setQuery] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [composing, setComposing] = useState(false);
  const [draft, setDraft] = useState<Draft>(EMPTY_DRAFT);
  /** Entries whose full content is on screen. */
  const [openIds, setOpenIds] = useState<string[]>([]);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [edit, setEdit] = useState<Draft>(EMPTY_DRAFT);
  /** Delete asks twice, like the destructive actions in the settings do. */
  const [armedId, setArmedId] = useState<string | null>(null);
  const t = useT();
  const locale = useLocale();

  const when = (ms: number) => new Date(ms).toLocaleString(locale);

  const refresh = useCallback(() => {
    memoryLoad()
      .then((memory) => {
        setEntries(memory.entries ?? []);
        // A load that works retires the failure of the last one — without
        // this, one transient lock leaves its banner over a healthy list for
        // the rest of the session.
        setError(null);
      })
      // memory_load fails loudly when memory.json exists but cannot be read;
      // surfacing it keeps an unreadable memory from looking like an empty one.
      .catch((err) => {
        setEntries([]);
        setError(String(err));
      });
  }, []);

  useEffect(refresh, [refresh]);

  // Agents write through the HTTP API; the backend emits this so the window
  // reflects their work without polling.
  useEffect(() => {
    const unlisten = listen<string>(MEMORY_CHANGED_EVENT, () => refresh());
    return () => {
      unlisten.then((off) => off()).catch(() => undefined);
    };
  }, [refresh]);

  const terms = useMemo(
    () => query.trim().toLowerCase().split(/\s+/).filter(Boolean),
    [query],
  );

  const visible = useMemo(
    () =>
      entries
        .filter((entry) => matches(entry, terms))
        .sort((a, b) => b.updated_at - a.updated_at),
    [entries, terms],
  );

  /** Every write funnels through here, so a rejected limit is always shown. */
  async function run(action: () => Promise<unknown>) {
    setError(null);
    try {
      await action();
      refresh();
      return true;
    } catch (err) {
      setError(String(err));
      return false;
    }
  }

  async function create() {
    const title = draft.title.trim();
    const content = draft.content.trim();
    if (!title || !content) return;
    const ok = await run(() =>
      memoryCreate({
        title,
        content,
        tags: parseTags(draft.tags),
        workspace: cwd,
        agent: "user",
      }),
    );
    if (!ok) return;
    setDraft(EMPTY_DRAFT);
    setComposing(false);
  }

  function startEdit(entry: MemoryEntry) {
    setEditingId(entry.id);
    setEdit({
      title: entry.title,
      content: entry.content,
      tags: entry.tags.join(", "),
    });
    setArmedId(null);
    setError(null);
  }

  async function saveEdit(entry: MemoryEntry) {
    const title = edit.title.trim();
    const content = edit.content.trim();
    if (!title || !content) return;
    const ok = await run(() =>
      memoryUpdate(entry.id, { title, content, tags: parseTags(edit.tags) }),
    );
    if (ok) setEditingId(null);
  }

  async function remove(entry: MemoryEntry) {
    setArmedId(null);
    await run(() => memoryDelete(entry.id));
  }

  const toggle = (id: string) =>
    setOpenIds((current) =>
      current.includes(id) ? current.filter((it) => it !== id) : [...current, id],
    );

  return (
    <div className="memory">
      <div className="memory__bar">
        <span className="memory__count">
          {terms.length
            ? t("memory.results", { n: visible.length })
            : t("memory.entries", { n: entries.length })}
        </span>

        <input
          className="memory__search"
          type="search"
          placeholder={t("memory.search")}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />

        <span className="memory__hint">{t("memory.hint")}</span>

        <button
          className="btn btn--ghost"
          onClick={() => {
            setComposing((on) => !on);
            setError(null);
          }}
        >
          {t("memory.new")}
        </button>
      </div>

      {error && (
        <p className="memory__error" role="alert">
          {error}
        </p>
      )}

      <div className="memory__list">
        {composing && (
          <section className="memory__composer">
            <input
              className="text-input"
              autoFocus
              placeholder={t("memory.titlePlaceholder")}
              value={draft.title}
              onChange={(e) => setDraft({ ...draft, title: e.target.value })}
            />
            <textarea
              className="text-input text-area"
              rows={5}
              placeholder={t("memory.contentPlaceholder")}
              value={draft.content}
              onChange={(e) => setDraft({ ...draft, content: e.target.value })}
            />
            <input
              className="text-input"
              placeholder={t("memory.tagsPlaceholder")}
              value={draft.tags}
              onChange={(e) => setDraft({ ...draft, tags: e.target.value })}
            />
            <div className="memory__composer-foot">
              <span className="form-hint">{t("memory.upsertHint")}</span>
              <button
                className="btn btn--ghost"
                onClick={() => {
                  setComposing(false);
                  setDraft(EMPTY_DRAFT);
                  setError(null);
                }}
              >
                {t("create.cancel")}
              </button>
              <button
                className="btn btn--primary"
                onClick={create}
                disabled={!draft.title.trim() || !draft.content.trim()}
              >
                {t("memory.create")}
              </button>
            </div>
          </section>
        )}

        {/* Never under an error: a memory that could not be read is not one
            that has nothing in it, and saying "empty" would invite rewriting
            facts that are still on disk. */}
        {entries.length === 0 && !error && (
          <div className="memory__empty">
            <h3>{t("memory.empty")}</h3>
            <p>{t("memory.emptyBody")}</p>
          </div>
        )}

        {entries.length > 0 && visible.length === 0 && (
          <p className="memory__none">{t("memory.noResults", { q: query.trim() })}</p>
        )}

        {visible.map((entry) => {
          const open = openIds.includes(entry.id);
          const editing = editingId === entry.id;
          const workspace = entry.workspace ? shortWorkspace(entry.workspace) : "";

          return (
            <article key={entry.id} className={`mem ${open ? "mem--open" : ""}`}>
              <header className="mem__head">
                <button
                  className="mem__toggle"
                  onClick={() => toggle(entry.id)}
                  aria-expanded={open}
                  title={open ? t("memory.collapse") : t("memory.expand")}
                >
                  <span className="mem__caret" aria-hidden="true">
                    {open ? "▾" : "▸"}
                  </span>
                  <span className="mem__title">{entry.title}</span>
                </button>
                <span
                  className="mem__time"
                  title={t("memory.updatedAt", { date: when(entry.updated_at) })}
                >
                  {when(entry.updated_at)}
                </span>
              </header>

              <div className="mem__meta">
                {entry.tags.map((tag) => (
                  <span key={tag} className="chip">
                    {tag}
                  </span>
                ))}
                {entry.agent && (
                  <span
                    className="chip chip--assignee"
                    title={t("memory.byAgent", { agent: entry.agent })}
                  >
                    {entry.agent}
                  </span>
                )}
                {workspace && (
                  <span
                    className="chip"
                    title={t("memory.fromWorkspace", { path: entry.workspace })}
                  >
                    {workspace}
                  </span>
                )}
              </div>

              {editing ? (
                <div className="mem__editor">
                  <input
                    className="text-input"
                    value={edit.title}
                    onChange={(e) => setEdit({ ...edit, title: e.target.value })}
                  />
                  <textarea
                    className="text-input text-area"
                    rows={8}
                    value={edit.content}
                    onChange={(e) => setEdit({ ...edit, content: e.target.value })}
                  />
                  <input
                    className="text-input"
                    placeholder={t("memory.tagsPlaceholder")}
                    value={edit.tags}
                    onChange={(e) => setEdit({ ...edit, tags: e.target.value })}
                  />
                  <div className="mem__actions">
                    <span className="form-hint">{t("memory.tagsHint")}</span>
                    <button className="btn btn--ghost" onClick={() => setEditingId(null)}>
                      {t("create.cancel")}
                    </button>
                    <button
                      className="btn btn--primary"
                      onClick={() => saveEdit(entry)}
                      disabled={!edit.title.trim() || !edit.content.trim()}
                    >
                      {t("memory.save")}
                    </button>
                  </div>
                </div>
              ) : (
                <>
                  <p className={`mem__content ${open ? "" : "mem__content--clamp"}`}>
                    {entry.content}
                  </p>

                  {open && (
                    <div className="mem__actions">
                      <span className="mem__created">
                        {t("memory.createdAt", { date: when(entry.created_at) })}
                      </span>
                      <button className="btn btn--ghost" onClick={() => startEdit(entry)}>
                        {t("memory.edit")}
                      </button>
                      {armedId === entry.id ? (
                        <>
                          <button className="btn btn--ghost" onClick={() => setArmedId(null)}>
                            {t("create.cancel")}
                          </button>
                          <button className="btn btn--danger" onClick={() => remove(entry)}>
                            {t("memory.deleteConfirm")}
                          </button>
                        </>
                      ) : (
                        <button className="btn btn--danger" onClick={() => setArmedId(entry.id)}>
                          {t("memory.delete")}
                        </button>
                      )}
                    </div>
                  )}
                </>
              )}
            </article>
          );
        })}
      </div>
    </div>
  );
}
