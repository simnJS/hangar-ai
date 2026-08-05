import { useStore } from "../store";
import { THEMES } from "../themes";
import type { Settings, ShellInfo } from "../types";

const FONT_STACKS = [
  { label: "Cascadia Code", value: '"Cascadia Code", "Cascadia Mono", Consolas, monospace' },
  { label: "JetBrains Mono", value: '"JetBrains Mono", "Fira Code", Consolas, monospace' },
  { label: "Fira Code", value: '"Fira Code", "Cascadia Code", Consolas, monospace' },
  { label: "Consolas", value: 'Consolas, "Courier New", monospace' },
  { label: "Hack", value: 'Hack, "DejaVu Sans Mono", Consolas, monospace' },
  { label: "IBM Plex Mono", value: '"IBM Plex Mono", Consolas, monospace' },
  { label: "Système", value: 'ui-monospace, SFMono-Regular, Menlo, Consolas, monospace' },
];

function Row({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) {
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
        <h2>Apparence & réglages</h2>
        <button className="icon-btn" onClick={onClose} title="Fermer">
          ×
        </button>
      </header>

      <div className="drawer__body">
        <section className="group">
          <h3 className="group__title">
            Thème
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
                spécifique à ce workspace
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
          <h3 className="group__title">Shell</h3>

          <Row label="Par défaut" hint={`${shells.length} détecté(s)`}>
            <select
              value={state.settings.shellId ?? ""}
              onChange={(e) => updateSettings({ shellId: e.target.value || null })}
            >
              <option value="">Auto ({shells[0]?.label ?? "aucun"})</option>
              {shells.map((shell) => (
                <option key={shell.id} value={shell.id}>
                  {shell.label}
                </option>
              ))}
            </select>
          </Row>

          {activeWorkspace && (
            <Row label="Ce workspace" hint="prioritaire sur le défaut">
              <select
                value={activeWorkspace.shellId ?? ""}
                onChange={(e) =>
                  updateWorkspace(activeWorkspace.id, { shellId: e.target.value || null })
                }
              >
                <option value="">Suivre le défaut</option>
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
          <h3 className="group__title">Typographie</h3>

          <Row label="Police">
            <select value={settings.fontFamily} onChange={(e) => set("fontFamily", e.target.value)}>
              {FONT_STACKS.map((font) => (
                <option key={font.label} value={font.value}>
                  {font.label}
                </option>
              ))}
              {!FONT_STACKS.some((f) => f.value === settings.fontFamily) && (
                <option value={settings.fontFamily}>Personnalisée</option>
              )}
            </select>
          </Row>

          <Row label="Taille" hint={`${settings.fontSize}px`}>
            <input
              type="range"
              min={9}
              max={24}
              value={settings.fontSize}
              onChange={(e) => set("fontSize", Number(e.target.value))}
            />
          </Row>

          <Row label="Interligne" hint={settings.lineHeight.toFixed(2)}>
            <input
              type="range"
              min={1}
              max={2}
              step={0.05}
              value={settings.lineHeight}
              onChange={(e) => set("lineHeight", Number(e.target.value))}
            />
          </Row>

          <Row label="Espacement" hint={`${settings.letterSpacing}px`}>
            <input
              type="range"
              min={-1}
              max={3}
              step={0.5}
              value={settings.letterSpacing}
              onChange={(e) => set("letterSpacing", Number(e.target.value))}
            />
          </Row>

          <Row label="Marge interne" hint={`${settings.padding}px`}>
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
          <h3 className="group__title">Curseur & historique</h3>

          <Row label="Curseur">
            <select
              value={settings.cursorStyle}
              onChange={(e) => set("cursorStyle", e.target.value as Settings["cursorStyle"])}
            >
              <option value="bar">Barre</option>
              <option value="block">Bloc</option>
              <option value="underline">Souligné</option>
            </select>
          </Row>

          <Row label="Clignotement">
            <input
              type="checkbox"
              checked={settings.cursorBlink}
              onChange={(e) => set("cursorBlink", e.target.checked)}
            />
          </Row>

          <Row label="Scrollback" hint={`${settings.scrollback} lignes`}>
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
          <h3 className="group__title">Sessions</h3>

          <Row label="Reprise auto" hint="relance les agents sur leur dernière session">
            <input
              type="checkbox"
              checked={settings.autoResume}
              onChange={(e) => set("autoResume", e.target.checked)}
            />
          </Row>

          <Row label="Délai de lancement" hint={`${settings.launchDelayMs} ms`}>
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
