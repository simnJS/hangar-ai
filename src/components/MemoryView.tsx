import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  MEMORY_CHANGED_EVENT,
  memoryCreate,
  memoryDelete,
  memoryLoad,
  memoryUpdate,
  type MemoryEntry,
} from "../lib/memory";
import { compactPath, shortDay, signerName, wasUpdated } from "../lib/memoryFormat";
import { useLocale, useT } from "../i18n";
import { BoardIcon } from "./BoardIcons";
import { Icon } from "./Icon";

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
  const haystack = `${entry.title}\n${entry.content}\n${entry.tags.join(" ")}`.toLowerCase();
  return terms.every((term) => haystack.includes(term));
}

export function MemoryView({ cwd }: Props) {
  /** null until the first read answers: a slow read is not an empty memory. */
  const [entries, setEntries] = useState<MemoryEntry[] | null>(null);
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
  const searchRef = useRef<HTMLInputElement>(null);
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

  const terms = useMemo(() => query.trim().toLowerCase().split(/\s+/).filter(Boolean), [query]);

  const all = useMemo(() => entries ?? [], [entries]);
  const visible = useMemo(
    () => all.filter((entry) => matches(entry, terms)).sort((a, b) => b.updated_at - a.updated_at),
    [all, terms],
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

  function startComposing() {
    setComposing(true);
    setError(null);
  }

  /** "Written by Ava", "Updated Oct 21", in the row's right column. */
  function origin(entry: MemoryEntry) {
    const signer = signerName(entry.agent);
    const updated = wasUpdated(entry.created_at, entry.updated_at);
    const at = updated ? entry.updated_at : entry.created_at;
    const day = shortDay(at, Date.now(), locale);
    const dated =
      day === "today"
        ? t(updated ? "memory.updatedToday" : "memory.createdToday")
        : t(updated ? "memory.updatedAt" : "memory.createdAt", { date: day });
    return { signer, dated, at, updated };
  }

  const loaded = entries !== null;

  return (
    <div className="memory">
      <div className="memory__bar">
        <h2 className="memory__count">
          {!loaded
            ? t("memory.title")
            : terms.length
              ? t("memory.results", { n: visible.length })
              : t("memory.entries", { n: all.length })}
        </h2>

        <label className="memory__search">
          <BoardIcon name="search" size={12} />
          <input
            ref={searchRef}
            type="search"
            placeholder={t("memory.search")}
            aria-label={t("memory.search")}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              // Escape empties the field first, the way a search field does.
              if (e.key === "Escape" && query) {
                e.stopPropagation();
                setQuery("");
              }
            }}
          />
        </label>

        <span className="memory__hint" title={t("memory.hint")}>
          <BoardIcon name="plug" size={12} />
          <span className="memory__hint-text">{t("memory.hint")}</span>
        </span>

        <button
          type="button"
          className="btn memory__new"
          aria-expanded={composing}
          onClick={() => (composing ? setComposing(false) : startComposing())}
        >
          <Icon name="plus" size={12} />
          {t("memory.new")}
        </button>
      </div>

      {error && (
        <p className="memory__error" role="alert">
          <Icon name="alert" size={12} />
          <span>{error}</span>
        </p>
      )}

      <div className="memory__list">
        {composing && (
          <section className="memory__composer" aria-label={t("memory.new")}>
            <input
              className="memory__input memory__input--title"
              autoFocus
              placeholder={t("memory.titlePlaceholder")}
              aria-label={t("memory.titlePlaceholder")}
              value={draft.title}
              onChange={(e) => setDraft({ ...draft, title: e.target.value })}
            />
            <textarea
              className="memory__input memory__input--area"
              rows={5}
              placeholder={t("memory.contentPlaceholder")}
              aria-label={t("memory.contentPlaceholder")}
              value={draft.content}
              onChange={(e) => setDraft({ ...draft, content: e.target.value })}
            />
            <input
              className="memory__input memory__input--tags"
              placeholder={t("memory.tagsPlaceholder")}
              aria-label={t("memory.tagsPlaceholder")}
              value={draft.tags}
              onChange={(e) => setDraft({ ...draft, tags: e.target.value })}
            />
            <div className="memory__composer-foot">
              <span className="memory__note">{t("memory.upsertHint")}</span>
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

        {!loaded && (
          <div className="memory__skeleton" role="status" aria-label={t("memory.loading")}>
            {[0, 1, 2, 3].map((row) => (
              <div key={row} className="mem mem--ghost">
                <div className="mem__main">
                  <span className="skeleton skeleton--title" />
                  <span className="skeleton skeleton--line" />
                  <span className="skeleton skeleton--line skeleton--short" />
                </div>
                <div className="mem__by">
                  <span className="skeleton skeleton--meta" />
                </div>
              </div>
            ))}
          </div>
        )}

        {/* Never under an error: a memory that could not be read is not one
            that has nothing in it, and saying "empty" would invite rewriting
            facts that are still on disk. */}
        {loaded && all.length === 0 && !error && !composing && (
          <div className="memory__empty">
            <span className="memory__empty-icon" aria-hidden>
              <BoardIcon name="bookmark" size={20} />
            </span>
            <h3>{t("memory.empty")}</h3>
            <p>{t("memory.emptyBody")}</p>
            <button type="button" className="btn btn--primary" onClick={startComposing}>
              <Icon name="plus" size={12} />
              {t("memory.emptyAction")}
            </button>
          </div>
        )}

        {all.length > 0 && visible.length === 0 && (
          <div className="memory__none">
            <p>{t("memory.noResults", { q: query.trim() })}</p>
            <button
              type="button"
              className="btn btn--ghost"
              onClick={() => {
                setQuery("");
                searchRef.current?.focus();
              }}
            >
              {t("memory.clearSearch")}
            </button>
          </div>
        )}

        {visible.map((entry) => {
          const open = openIds.includes(entry.id);
          const editing = editingId === entry.id;
          const { signer, dated, at, updated } = origin(entry);

          return (
            <article
              key={entry.id}
              className={`mem ${open ? "mem--open" : ""} ${editing ? "mem--editing" : ""}`}
            >
              <div className="mem__main">
                {editing ? (
                  <div className="mem__editor">
                    <input
                      className="memory__input memory__input--title"
                      aria-label={t("memory.titlePlaceholder")}
                      value={edit.title}
                      onChange={(e) => setEdit({ ...edit, title: e.target.value })}
                    />
                    <textarea
                      className="memory__input memory__input--area"
                      rows={8}
                      aria-label={t("memory.contentPlaceholder")}
                      value={edit.content}
                      onChange={(e) => setEdit({ ...edit, content: e.target.value })}
                    />
                    <input
                      className="memory__input memory__input--tags"
                      placeholder={t("memory.tagsPlaceholder")}
                      aria-label={t("memory.tagsPlaceholder")}
                      value={edit.tags}
                      onChange={(e) => setEdit({ ...edit, tags: e.target.value })}
                    />
                    <div className="mem__actions">
                      <span className="memory__note">{t("memory.tagsHint")}</span>
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
                    <h3 className="mem__head">
                      <button
                        type="button"
                        className="mem__toggle"
                        onClick={() => toggle(entry.id)}
                        aria-expanded={open}
                        title={open ? t("memory.collapse") : t("memory.expand")}
                      >
                        <span className="mem__caret" aria-hidden>
                          <BoardIcon name="chevron" size={10} />
                        </span>
                        <span className="mem__title">{entry.title}</span>
                      </button>
                    </h3>

                    <p className={`mem__content ${open ? "" : "mem__content--clamp"}`}>
                      {entry.content}
                    </p>

                    {entry.tags.length > 0 && (
                      <div className="mem__tags">
                        {entry.tags.map((tag) => (
                          <span key={tag} className="chip">
                            {tag}
                          </span>
                        ))}
                      </div>
                    )}

                    {open && (
                      <div className="mem__actions">
                        <span className="memory__note">
                          {t("memory.createdAt", { date: when(entry.created_at) })}
                        </span>
                        <button className="btn btn--ghost" onClick={() => startEdit(entry)}>
                          <Icon name="pencil" size={11} />
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
                          <button
                            className="btn btn--ghost mem__delete"
                            onClick={() => setArmedId(entry.id)}
                          >
                            <BoardIcon name="trash" size={11} />
                            {t("memory.delete")}
                          </button>
                        )}
                      </div>
                    )}
                  </>
                )}
              </div>

              <div className="mem__by">
                <span className="mem__signer" title={t("memory.byAgent", { agent: entry.agent })}>
                  {signer === null ? (
                    t("memory.byYou")
                  ) : (
                    <>
                      {t("memory.writtenBy")} <b>{signer}</b>
                    </>
                  )}
                </span>
                <span
                  className="mem__date"
                  title={t(updated ? "memory.updatedAt" : "memory.createdAt", { date: when(at) })}
                >
                  {dated}
                </span>
                {entry.workspace && (
                  <span
                    className="mem__path"
                    title={t("memory.fromWorkspace", { path: entry.workspace })}
                  >
                    {compactPath(entry.workspace)}
                  </span>
                )}
              </div>
            </article>
          );
        })}
      </div>
    </div>
  );
}
