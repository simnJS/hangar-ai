import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { useT } from "../../i18n";
import { useStore } from "../../store";
import { shortcutsSuspended } from "../../lib/useShortcuts";
import { DEFAULT_SETTINGS, type Settings, type ShellInfo } from "../../types";
import {
  buildCategories,
  type SettingCategory,
  type SettingItem,
} from "./schema";

/** Search ignores case and accents: "themes" finds "Thème". */
const fold = (text: string) =>
  text
    .normalize("NFD")
    .replace(/[\u0300-\u036f]/g, "")
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
        <select
          className="set-select"
          value={item.value}
          onChange={(e) => item.onChange(e.target.value)}
        >
          {item.choices.map((choice) => (
            <option key={choice.value} value={choice.value}>
              {choice.label}
            </option>
          ))}
        </select>
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

    case "slider":
      return (
        <>
          <input
            type="range"
            className="set-range"
            min={item.min}
            max={item.max}
            step={item.step ?? 1}
            value={item.value}
            onChange={(e) => item.onChange(Number(e.target.value))}
          />
          <span className="set-value">{item.display}</span>
        </>
      );

    case "text":
      return <span className="set-text">{item.value}</span>;

    case "action":
      return <ActionControl item={item} />;
  }
}

function Row({ item }: { item: SettingItem }) {
  if (item.kind === "custom") {
    return (
      <div className="set-row set-row--block">
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
    <Tag className="set-row">
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
    function onKey(event: KeyboardEvent) {
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

  const countItems = (category: SettingCategory) =>
    category.sections.reduce((total, section) => total + section.items.length, 0);

  return (
    <div
      className="settings"
      role="dialog"
      aria-modal="true"
      aria-label={t("settings.title")}
    >
      <header className="settings__head">
        <h2 className="settings__title">
          <span className="settings__title-icon" aria-hidden="true">
            ⚙
          </span>
          {t("settings.title")}
        </h2>

        <div className="settings__search">
          <span className="settings__search-icon" aria-hidden="true">
            ⌕
          </span>
          <input
            autoFocus
            value={query}
            placeholder={t("settings.search")}
            aria-label={t("settings.search")}
            onChange={(e) => setQuery(e.target.value)}
          />
          {query && (
            <button
              className="icon-btn"
              title={t("settings.searchClear")}
              onClick={() => setQuery("")}
            >
              ×
            </button>
          )}
        </div>

        <button className="icon-btn" onClick={onClose} title={t("settings.close")}>
          ×
        </button>
      </header>

      <div className="settings__body">
        <nav className="settings__rail" aria-label={t("settings.title")}>
          {visible.map((category) => (
            <button
              key={category.id}
              className={`settings__nav ${active?.id === category.id ? "is-active" : ""}`}
              onClick={() => setCategoryId(category.id)}
            >
              <span className="settings__nav-icon" aria-hidden="true">
                {category.icon}
              </span>
              <span className="settings__nav-label">{category.label}</span>
              {needle && (
                <span className="settings__nav-count">{countItems(category)}</span>
              )}
            </button>
          ))}
        </nav>

        <div className="settings__content">
          {active ? (
            <div className="settings__page">
              <h3 className="settings__page-title">{active.label}</h3>

              {active.sections.map((section) => (
                <section className="set-section" key={section.id}>
                  <h4 className="set-section__title">{section.title}</h4>
                  {section.hint && <p className="set-section__hint">{section.hint}</p>}
                  <div className="set-section__rows">
                    {section.items.map((item) => (
                      <Row key={item.id} item={item} />
                    ))}
                  </div>
                </section>
              ))}
            </div>
          ) : (
            <p className="settings__empty">{t("settings.noResults", { q: query })}</p>
          )}
        </div>
      </div>
    </div>
  );
}
