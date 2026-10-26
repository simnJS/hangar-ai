import {
  useEffect,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
} from "react";
import { getVersion } from "@tauri-apps/api/app";
import { useT } from "../../i18n";
import { useStore } from "../../store";
import { arrowStep, stepIndex } from "../../lib/menuNav";
import { shortcutsSuspended } from "../../lib/useShortcuts";
import { DEFAULT_SETTINGS, type Settings, type ShellInfo } from "../../types";
import { Backdrop } from "../Backdrop";
import { Icon } from "../Icon";
import {
  buildCategories,
  type SettingCategory,
  type SettingItem,
} from "./schema";

/** Search ignores case and accents: "themes" finds "Thème". */
const fold = (text: string) =>
  text
    .normalize("NFD")
    .replace(/[̀-ͯ]/g, "")
    .toLowerCase();

type RowItem = Exclude<SettingItem, { kind: "custom" }>;

function ActionControl({
  item,
}: {
  item: Extract<SettingItem, { kind: "action" }>;
}) {
  const [armed, setArmed] = useState(false);
  const t = useT();

  if (!item.confirmLabel) {
    return (
      <button
        type="button"
        className={`btn ${item.danger ? "btn--danger" : ""}`}
        onClick={item.onAct}
      >
        {item.actionLabel}
      </button>
    );
  }

  return armed ? (
    <>
      <button type="button" className="btn btn--ghost" onClick={() => setArmed(false)}>
        {t("create.cancel")}
      </button>
      <button
        type="button"
        className={`btn ${item.danger ? "btn--danger" : "btn--primary"}`}
        onClick={() => {
          item.onAct();
          setArmed(false);
        }}
      >
        {item.confirmLabel}
      </button>
    </>
  ) : (
    <button type="button" className="btn" onClick={() => setArmed(true)}>
      {item.actionLabel}
    </button>
  );
}

function Control({ item }: { item: RowItem }) {
  switch (item.kind) {
    case "select":
      return (
        // The wrapper draws the chevrons: a native arrow cannot be recoloured,
        // and would stay the platform's grey on every theme.
        <span className="set-select">
          <select
            className="set-select__field"
            value={item.value}
            onChange={(e) => item.onChange(e.target.value)}
          >
            {item.choices.map((choice) => (
              <option key={choice.value} value={choice.value}>
                {choice.label}
              </option>
            ))}
          </select>
        </span>
      );

    case "segmented":
      return (
        <div className="seg" role="group" aria-label={item.label}>
          {item.choices.map((choice) => (
            <button
              key={choice.value}
              type="button"
              className={`seg__btn ${item.value === choice.value ? "is-active" : ""}`}
              aria-pressed={item.value === choice.value}
              onClick={() => item.onChange(choice.value)}
            >
              {choice.label}
            </button>
          ))}
        </div>
      );

    case "toggle":
      return (
        <span className="switch">
          <input
            type="checkbox"
            role="switch"
            checked={item.value}
            onChange={(e) => item.onChange(e.target.checked)}
          />
          <span className="switch__track" aria-hidden="true">
            <span className="switch__knob" />
          </span>
        </span>
      );

    case "input":
      return (
        <input
          type="text"
          className="set-input"
          value={item.value}
          placeholder={item.placeholder}
          spellCheck={false}
          onChange={(e) => item.onChange(e.target.value)}
        />
      );

    case "slider": {
      // The filled part of the track is painted from this, so the slider reads
      // as a level rather than as a knob floating on a line.
      const fill = ((item.value - item.min) / Math.max(1e-9, item.max - item.min)) * 100;
      return (
        <>
          <input
            type="range"
            className="set-range"
            min={item.min}
            max={item.max}
            step={item.step ?? 1}
            value={item.value}
            style={{ "--fill": `${fill}%` } as CSSProperties}
            onChange={(e) => item.onChange(Number(e.target.value))}
          />
          <span className="set-value">{item.display}</span>
        </>
      );
    }

    case "text":
      return <span className="set-text">{item.value}</span>;

    case "action":
      return <ActionControl item={item} />;
  }
}

function Row({ item }: { item: SettingItem }) {
  if (item.kind === "custom") {
    return (
      <div className={`set-row set-row--block ${item.bare ? "set-row--bare" : ""}`}>
        {item.label && (
          <span className="set-row__text">
            <span className="set-row__label">{item.label}</span>
            {item.hint && <span className="set-row__hint">{item.hint}</span>}
          </span>
        )}
        {item.render()}
      </div>
    );
  }

  // A <label> wraps the control it names, so clicking the text focuses it —
  // which only holds for the kinds backed by a real form element.
  const Tag =
    item.kind === "select" ||
    item.kind === "toggle" ||
    item.kind === "slider" ||
    item.kind === "input"
      ? "label"
      : "div";

  return (
    <Tag className={`set-row set-row--${item.kind}`}>
      <span className="set-row__text">
        <span className="set-row__label">{item.label}</span>
        {item.hint && <span className="set-row__hint">{item.hint}</span>}
      </span>
      <span className="set-row__control">
        <Control item={item} />
      </span>
    </Tag>
  );
}

/**
 * The settings window: categories on the left, the chosen one's grouped rows
 * on the right, the way the system's own settings are laid out. A sheet over
 * the workspace rather than a page instead of it — the terminals stay mounted
 * underneath, and opening the settings never disturbs a running agent.
 */
export function SettingsPage({
  shells,
  category = "general",
  onClose,
}: {
  shells: ShellInfo[];
  /** Category to open on — a shortcut can aim at one in particular. */
  category?: string;
  onClose: () => void;
}) {
  const { state, activeWorkspace, updateSettings, updateWorkspace } = useStore();
  const [categoryId, setCategoryId] = useState(category);
  const [query, setQuery] = useState("");
  const [version, setVersion] = useState("");
  const contentRef = useRef<HTMLDivElement>(null);
  const navRef = useRef<HTMLElement>(null);
  const t = useT();

  useEffect(() => {
    // Dev builds and older permission sets may not answer; the row just stays out.
    getVersion()
      .then(setVersion)
      .catch(() => setVersion(""));
  }, []);

  // Asking for a category while the page is already up moves to it.
  useEffect(() => setCategoryId(category), [category]);

  /**
   * Escape first empties the search, then closes: a filtered page would
   * otherwise disappear with no way back to what the query was hiding.
   * Capture phase, like the app shortcuts, so a focused terminal cannot eat it.
   */
  useEffect(() => {
    function onKey(event: globalThis.KeyboardEvent) {
      if (event.key !== "Escape") return;
      // A shortcut being recorded owns Escape: it cancels the recording, and
      // closing the page under it would be the last thing anyone wanted.
      if (shortcutsSuspended()) return;
      event.preventDefault();
      event.stopPropagation();
      if (query) setQuery("");
      else onClose();
    }
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [query, onClose]);

  const categories = buildCategories({
    t,
    settings: state.settings,
    shells,
    workspace: activeWorkspace,
    version,
    set: <K extends keyof Settings>(key: K, value: Settings[K]) =>
      updateSettings({ [key]: value } as Partial<Settings>),
    patchWorkspace: (patch) => {
      if (activeWorkspace) updateWorkspace(activeWorkspace.id, patch);
    },
    resetSettings: () => updateSettings(DEFAULT_SETTINGS),
  });

  const needle = fold(query.trim());

  /** Keeps only the items whose text matches, then drops what is left empty. */
  const visible: SettingCategory[] = !needle
    ? categories
    : categories
        .map((category) => ({
          ...category,
          sections: category.sections
            .map((section) => ({
              ...section,
              items: section.items.filter((item) =>
                fold(
                  [
                    item.kind === "custom" ? (item.label ?? "") : item.label,
                    item.hint ?? "",
                    item.keywords ?? "",
                    section.title,
                    category.label,
                  ].join(" "),
                ).includes(needle),
              ),
            }))
            .filter((section) => section.items.length > 0),
        }))
        .filter((category) => category.sections.length > 0);

  const active = visible.find((c) => c.id === categoryId) ?? visible[0] ?? null;

  // A new category starts at its top, not wherever the last one was scrolled.
  useEffect(() => {
    contentRef.current?.scrollTo({ top: 0 });
  }, [active?.id]);

  const countItems = (category: SettingCategory) =>
    category.sections.reduce((total, section) => total + section.items.length, 0);

  /** Up and Down walk the category list, the way a sidebar does. */
  function onNavKey(event: KeyboardEvent) {
    const step = arrowStep(event.key);
    if (!step) return;
    const index = visible.findIndex((c) => c.id === active?.id);
    const at = stepIndex(index, visible.length, step, false);
    if (at === null) return;
    const next = visible[at];
    event.preventDefault();
    setCategoryId(next.id);
    navRef.current
      ?.querySelector<HTMLButtonElement>(`[data-category="${next.id}"]`)
      ?.focus();
  }

  return (
    <Backdrop onClose={onClose} escape={false} className="modal-backdrop--settings">
      <div
        className="settings"
        role="dialog"
        aria-modal="true"
        aria-label={t("settings.title")}
      >
        <aside className="settings__side">
          <div className="settings__search">
            <Icon name="search" className="settings__search-icon" />
            <input
              autoFocus
              value={query}
              placeholder={t("settings.search")}
              aria-label={t("settings.search")}
              spellCheck={false}
              onChange={(e) => setQuery(e.target.value)}
            />
            {query && (
              <button
                type="button"
                className="settings__search-clear"
                title={t("settings.searchClear")}
                aria-label={t("settings.searchClear")}
                onClick={() => setQuery("")}
              >
                <Icon name="close" size={10} />
              </button>
            )}
          </div>

          <nav
            className="settings__nav"
            aria-label={t("settings.title")}
            ref={navRef}
            onKeyDown={onNavKey}
          >
            {visible.map((category) => {
              const current = active?.id === category.id;
              return (
                <button
                  key={category.id}
                  type="button"
                  data-category={category.id}
                  className={`settings__cat ${current ? "is-active" : ""}`}
                  aria-current={current ? "page" : undefined}
                  onClick={() => setCategoryId(category.id)}
                >
                  <Icon name={category.icon} size={15} className="settings__cat-icon" />
                  <span className="settings__cat-label">{category.label}</span>
                  {needle && (
                    <span className="settings__cat-count">{countItems(category)}</span>
                  )}
                </button>
              );
            })}
          </nav>
        </aside>

        <section className="settings__main">
          <header className="settings__head">
            <h2 className="settings__title">{active?.label ?? t("settings.title")}</h2>
            <button
              type="button"
              className="modal__close"
              onClick={onClose}
              aria-label={t("settings.close")}
              title={t("keymap.withKeys", { label: t("settings.close"), keys: "Esc" })}
            >
              <Icon name="close" />
            </button>
          </header>

          <div className="settings__content" ref={contentRef}>
            {active ? (
              <div className="settings__page">
                {active.sections.map((section) => {
                  const bare = section.items.every(
                    (item) => item.kind === "custom" && item.bare,
                  );
                  return (
                    <section className="set-section" key={section.id}>
                      {/* A group named like its page would only repeat the title
                          above it; its hint still has something to say. */}
                      {(section.title !== active.label || section.hint) && (
                        <header className="set-section__head">
                          {section.title !== active.label && (
                            <h3 className="set-section__title">{section.title}</h3>
                          )}
                          {section.hint && (
                            <p className="set-section__hint">{section.hint}</p>
                          )}
                        </header>
                      )}
                      <div className={bare ? "set-section__bare" : "set-group"}>
                        {section.items.map((item) => (
                          <Row key={item.id} item={item} />
                        ))}
                      </div>
                    </section>
                  );
                })}
              </div>
            ) : (
              <p className="settings__empty">{t("settings.noResults", { q: query })}</p>
            )}
          </div>
        </section>
      </div>
    </Backdrop>
  );
}
