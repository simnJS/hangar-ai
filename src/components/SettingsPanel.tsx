import { useStore } from "../store";
import { LOCALES, useT } from "../i18n";
import { THEMES } from "../themes";
import type { Settings, ShellInfo } from "../types";

const FONT_STACKS = [
  { label: "Cascadia Code", value: '"Cascadia Code", "Cascadia Mono", Consolas, monospace' },
  { label: "JetBrains Mono", value: '"JetBrains Mono", "Fira Code", Consolas, monospace' },
  { label: "Fira Code", value: '"Fira Code", "Cascadia Code", Consolas, monospace' },
  { label: "Consolas", value: 'Consolas, "Courier New", monospace' },
  { label: "Hack", value: 'Hack, "DejaVu Sans Mono", Consolas, monospace' },
  { label: "IBM Plex Mono", value: '"IBM Plex Mono", Consolas, monospace' },
  { label: "System", value: 'ui-monospace, SFMono-Regular, Menlo, Consolas, monospace' },
];

function Row({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <label className="field">
      <span className="field__label">
        {label}
        {hint && <span className="field__hint">{hint}</span>}
      </span>
      <span className="field__control">{children}</span>
    </label>
  );
}

export function SettingsPanel({
  shells,
  onClose,
}: {
  shells: ShellInfo[];
  onClose: () => void;
}) {
  const { state, activeWorkspace, updateSettings, updateWorkspace } = useStore();
  const settings = state.settings;
  const t = useT();

  const set = <K extends keyof Settings>(key: K, value: Settings[K]) =>
    updateSettings({ [key]: value } as Partial<Settings>);

  // A workspace can pin its own theme; otherwise it follows the global one.
  const workspaceThemeId = activeWorkspace?.themeId ?? null;
  const effectiveThemeId = workspaceThemeId ?? settings.themeId;

  function pickTheme(id: string) {
    if (workspaceThemeId !== null && activeWorkspace) {
      updateWorkspace(activeWorkspace.id, { themeId: id });
    } else {
      updateSettings({ themeId: id });
    }
  }

  return (
    <div className="drawer">
      <header className="drawer__head">
        <h2>{t("settings.title")}</h2>
        <button className="icon-btn" onClick={onClose} title={t("settings.close")}>
          ×
        </button>
      </header>

      <div className="drawer__body">
        <section className="group">
          <h3 className="group__title">{t("settings.language")}</h3>

          <Row label={t("settings.languageLabel")}>
            <select
              value={settings.locale ?? ""}
              onChange={(e) =>
                set("locale", (e.target.value || null) as Settings["locale"])
              }
            >
              <option value="">Auto</option>
              {LOCALES.map((locale) => (
                <option key={locale.id} value={locale.id}>
                  {locale.label}
                </option>
              ))}
            </select>
          </Row>
        </section>

        <section className="group">
          <h3 className="group__title">
            {t("settings.theme")}
            {activeWorkspace && (
              <label className="group__toggle">
                <input
                  type="checkbox"
                  checked={workspaceThemeId !== null}
                  onChange={(e) =>
                    updateWorkspace(activeWorkspace.id, {
                      themeId: e.target.checked ? effectiveThemeId : null,
                    })
                  }
                />
                {t("settings.themePerWorkspace")}
              </label>
            )}
          </h3>

          <div className="themes">
            {THEMES.map((theme) => {
              const swatches = [
                theme.xterm.red,
                theme.xterm.green,
                theme.xterm.yellow,
                theme.xterm.blue,
                theme.xterm.magenta,
                theme.xterm.cyan,
              ];
              return (
                <button
                  key={theme.id}
                  className={`theme-card ${effectiveThemeId === theme.id ? "is-active" : ""}`}
                  onClick={() => pickTheme(theme.id)}
                  style={{
                    background: theme.xterm.background,
                    color: theme.xterm.foreground,
                    borderColor:
                      effectiveThemeId === theme.id ? theme.accent : "transparent",
                  }}
                >
                  <span className="theme-card__name">{theme.name}</span>
                  <span className="theme-card__swatches">
                    {swatches.map((color, i) => (
                      <i key={i} style={{ background: color }} />
                    ))}
                  </span>
                </button>
              );
            })}
          </div>
        </section>

        <section className="group">
          <h3 className="group__title">{t("settings.shell")}</h3>

          <Row
            label={t("settings.shellDefault")}
            hint={t("settings.shellDetected", { n: shells.length })}
          >
            <select
              value={settings.shellId ?? ""}
              onChange={(e) => updateSettings({ shellId: e.target.value || null })}
            >
              <option value="">
                {t("settings.shellAuto", {
                  name: shells[0]?.label ?? t("create.shellNone"),
                })}
              </option>
              {shells.map((shell) => (
                <option key={shell.id} value={shell.id}>
                  {shell.label}
                </option>
              ))}
            </select>
          </Row>

          {activeWorkspace && (
            <Row
              label={t("settings.shellWorkspace")}
              hint={t("settings.shellWorkspaceHint")}
            >
              <select
                value={activeWorkspace.shellId ?? ""}
                onChange={(e) =>
                  updateWorkspace(activeWorkspace.id, { shellId: e.target.value || null })
                }
              >
                <option value="">{t("settings.shellFollow")}</option>
                {shells.map((shell) => (
                  <option key={shell.id} value={shell.id}>
                    {shell.label}
                  </option>
                ))}
              </select>
            </Row>
          )}
        </section>

        <section className="group">
          <h3 className="group__title">{t("settings.typography")}</h3>

          <Row label={t("settings.font")}>
            <select value={settings.fontFamily} onChange={(e) => set("fontFamily", e.target.value)}>
              {FONT_STACKS.map((font) => (
                <option key={font.label} value={font.value}>
                  {font.label}
                </option>
              ))}
              {!FONT_STACKS.some((f) => f.value === settings.fontFamily) && (
                <option value={settings.fontFamily}>{t("settings.fontCustom")}</option>
              )}
            </select>
          </Row>

          <Row label={t("settings.size")} hint={`${settings.fontSize}px`}>
            <input
              type="range"
              min={9}
              max={24}
              value={settings.fontSize}
              onChange={(e) => set("fontSize", Number(e.target.value))}
            />
          </Row>

          <Row label={t("settings.lineHeight")} hint={settings.lineHeight.toFixed(2)}>
            <input
              type="range"
              min={1}
              max={2}
              step={0.05}
              value={settings.lineHeight}
              onChange={(e) => set("lineHeight", Number(e.target.value))}
            />
          </Row>

          <Row label={t("settings.letterSpacing")} hint={`${settings.letterSpacing}px`}>
            <input
              type="range"
              min={-1}
              max={3}
              step={0.5}
              value={settings.letterSpacing}
              onChange={(e) => set("letterSpacing", Number(e.target.value))}
            />
          </Row>

          <Row label={t("settings.padding")} hint={`${settings.padding}px`}>
            <input
              type="range"
              min={0}
              max={28}
              value={settings.padding}
              onChange={(e) => set("padding", Number(e.target.value))}
            />
          </Row>
        </section>

        <section className="group">
          <h3 className="group__title">{t("settings.cursorSection")}</h3>

          <Row label={t("settings.cursor")}>
            <select
              value={settings.cursorStyle}
              onChange={(e) => set("cursorStyle", e.target.value as Settings["cursorStyle"])}
            >
              <option value="bar">{t("settings.cursorBar")}</option>
              <option value="block">{t("settings.cursorBlock")}</option>
              <option value="underline">{t("settings.cursorUnderline")}</option>
            </select>
          </Row>

          <Row label={t("settings.blink")}>
            <input
              type="checkbox"
              checked={settings.cursorBlink}
              onChange={(e) => set("cursorBlink", e.target.checked)}
            />
          </Row>

          <Row
            label={t("settings.scrollback")}
            hint={t("settings.scrollbackLines", { n: settings.scrollback })}
          >
            <input
              type="range"
              min={1000}
              max={50000}
              step={1000}
              value={settings.scrollback}
              onChange={(e) => set("scrollback", Number(e.target.value))}
            />
          </Row>
        </section>

        <section className="group">
          <h3 className="group__title">{t("settings.sessions")}</h3>

          <Row label={t("settings.autoResume")} hint={t("settings.autoResumeHint")}>
            <input
              type="checkbox"
              checked={settings.autoResume}
              onChange={(e) => set("autoResume", e.target.checked)}
            />
          </Row>

          <Row label={t("settings.launchDelay")} hint={`${settings.launchDelayMs} ms`}>
            <input
              type="range"
              min={0}
              max={4000}
              step={100}
              value={settings.launchDelayMs}
              onChange={(e) => set("launchDelayMs", Number(e.target.value))}
            />
          </Row>
        </section>
      </div>
    </div>
  );
}
