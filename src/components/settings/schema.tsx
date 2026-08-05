import type { ReactNode } from "react";
import { LOCALES, type Translator } from "../../i18n";
import { buildPresence } from "../../lib/discord";
import { isMac } from "../../lib/keys";
import { THEMES } from "../../themes";
import { DiscordCard } from "./DiscordCard";
import { ShortcutsSettings } from "./ShortcutsSettings";
import { DEFAULT_SETTINGS, type Settings, type ShellInfo, type Workspace } from "../../types";

/**
 * The settings are described as data rather than laid out by hand: a new option
 * is one entry in `buildCategories`, and the page renders its control, files it
 * under its category and matches it against the search box for free.
 */

export interface Choice {
  value: string;
  label: string;
}

interface ItemBase {
  id: string;
  label: string;
  hint?: string;
  /** Extra words the search box matches, beyond the label and the hint. */
  keywords?: string;
}

export type SettingItem =
  | (ItemBase & {
      kind: "select";
      value: string;
      choices: Choice[];
      onChange: (value: string) => void;
    })
  | (ItemBase & {
      kind: "segmented";
      value: string;
      choices: Choice[];
      onChange: (value: string) => void;
    })
  | (ItemBase & { kind: "toggle"; value: boolean; onChange: (value: boolean) => void })
  /** Free text: an identifier, a path — anything with no shortlist to pick from. */
  | (ItemBase & {
      kind: "input";
      value: string;
      placeholder?: string;
      onChange: (value: string) => void;
    })
  | (ItemBase & {
      kind: "slider";
      value: number;
      min: number;
      max: number;
      step?: number;
      /** Formatted value shown next to the track. */
      display: string;
      onChange: (value: number) => void;
    })
  /** Read-only line: a version, a keyboard shortcut… */
  | (ItemBase & { kind: "text"; value: string })
  | (ItemBase & {
      kind: "action";
      actionLabel: string;
      /** Set to require a second click, for anything not worth undoing by hand. */
      confirmLabel?: string;
      danger?: boolean;
      onAct: () => void;
    })
  /** Full-width block for anything that does not fit a label/control row. */
  | (Omit<ItemBase, "label"> & { kind: "custom"; label?: string; render: () => ReactNode });

export interface SettingSection {
  id: string;
  title: string;
  hint?: string;
  items: SettingItem[];
}

export interface SettingCategory {
  id: string;
  label: string;
  icon: string;
  sections: SettingSection[];
}

export interface SettingsContext {
  t: Translator;
  settings: Settings;
  shells: ShellInfo[];
  workspace: Workspace | null;
  /** Empty until the Tauri side answers — the row hides itself meanwhile. */
  version: string;
  set: <K extends keyof Settings>(key: K, value: Settings[K]) => void;
  patchWorkspace: (patch: Partial<Workspace>) => void;
  resetSettings: () => void;
}

const FONT_STACKS = [
  // The shipped default, verbatim: a fresh install must land on a named card
  // rather than on "Custom".
  { label: "Cascadia Code", value: DEFAULT_SETTINGS.fontFamily },
  { label: "JetBrains Mono", value: '"JetBrains Mono", "Fira Code", Consolas, monospace' },
  { label: "Fira Code", value: '"Fira Code", "Cascadia Code", Consolas, monospace' },
  { label: "Consolas", value: 'Consolas, "Courier New", monospace' },
  { label: "Hack", value: 'Hack, "DejaVu Sans Mono", Consolas, monospace' },
  { label: "IBM Plex Mono", value: '"IBM Plex Mono", Consolas, monospace' },
  { label: "System", value: "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace" },
];

/** Dark and light palettes are listed apart: the two rarely interest the same user. */
function ThemeGrid({
  t,
  activeId,
  onPick,
}: {
  t: Translator;
  activeId: string;
  onPick: (id: string) => void;
}) {
  return (
    <div className="theme-groups">
      {(["dark", "light"] as const).map((appearance) => {
        const themes = THEMES.filter((theme) => theme.appearance === appearance);
        if (themes.length === 0) return null;
        return (
          <div className="theme-group" key={appearance}>
            <span className="theme-group__label">
              {t(appearance === "dark" ? "settings.themeDark" : "settings.themeLight")}
            </span>
            <div className="theme-grid">
              {themes.map((theme) => {
                const swatches = [
                  theme.xterm.red,
                  theme.xterm.green,
                  theme.xterm.yellow,
                  theme.xterm.blue,
                  theme.xterm.magenta,
                  theme.xterm.cyan,
                ];
                const active = activeId === theme.id;
                return (
                  <button
                    key={theme.id}
                    type="button"
                    className={`theme-card ${active ? "is-active" : ""}`}
                    onClick={() => onPick(theme.id)}
                    style={{
                      background: theme.xterm.background,
                      color: theme.xterm.foreground,
                      borderColor: active ? theme.accent : "transparent",
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
          </div>
        );
      })}
    </div>
  );
}

function FontGrid({
  value,
  customLabel,
  onPick,
}: {
  value: string;
  customLabel: string;
  onPick: (stack: string) => void;
}) {
  const known = FONT_STACKS.some((font) => font.value === value);
  const stacks = known ? FONT_STACKS : [...FONT_STACKS, { label: customLabel, value }];
  return (
    <div className="font-grid">
      {stacks.map((font) => (
        <button
          key={font.label}
          type="button"
          className={`font-card ${font.value === value ? "is-active" : ""}`}
          onClick={() => onPick(font.value)}
        >
          <span className="font-card__name">{font.label}</span>
          <span className="font-card__sample" style={{ fontFamily: font.value }}>
            Aa Bb 0123 =&gt; != -&gt;
          </span>
        </button>
      ))}
    </div>
  );
}

/** Shows typography, cursor and padding at once, on the live theme. */
function TerminalPreview({ settings }: { settings: Settings }) {
  return (
    <div
      className="term-preview"
      style={{
        fontFamily: settings.fontFamily,
        fontSize: settings.fontSize,
        lineHeight: settings.lineHeight,
        letterSpacing: settings.letterSpacing,
        padding: settings.padding,
      }}
    >
      <div>
        <span className="term-preview__prompt">❯</span> claude --resume
      </div>
      <div className="term-preview__dim">Hangar.AI · 4 panes · session restored</div>
      <div>
        0123456789 =&gt; != -&gt; |&amp;
        <span
          className={`term-cursor term-cursor--${settings.cursorStyle} ${
            settings.cursorBlink ? "is-blinking" : ""
          }`}
        />
      </div>
    </div>
  );
}

export function buildCategories(ctx: SettingsContext): SettingCategory[] {
  const { t, settings, shells, workspace, version, set, patchWorkspace } = ctx;

  // A workspace can pin its own theme; otherwise it follows the global one.
  const pinnedThemeId = workspace?.themeId ?? null;
  const effectiveThemeId = pinnedThemeId ?? settings.themeId;
  const pickTheme = (id: string) =>
    pinnedThemeId !== null ? patchWorkspace({ themeId: id }) : set("themeId", id);

  const shellChoices: Choice[] = shells.map((shell) => ({
    value: shell.id,
    label: shell.label,
  }));

  return [
    {
      id: "general",
      label: t("settings.catGeneral"),
      icon: "◈",
      sections: [
        {
          id: "language",
          title: t("settings.language"),
          items: [
            {
              id: "locale",
              kind: "segmented",
              label: t("settings.languageLabel"),
              keywords: "langue language locale",
              value: settings.locale ?? "",
              choices: [
                { value: "", label: t("settings.localeAuto") },
                ...LOCALES.map((locale) => ({ value: locale.id, label: locale.label })),
              ],
              onChange: (value) =>
                set("locale", (value || null) as Settings["locale"]),
            },
          ],
        },
        {
          id: "sessions",
          title: t("settings.sessions"),
          items: [
            {
              id: "autoResume",
              kind: "toggle",
              label: t("settings.autoResume"),
              hint: t("settings.autoResumeHint"),
              value: settings.autoResume,
              onChange: (value) => set("autoResume", value),
            },
            {
              id: "launchDelay",
              kind: "slider",
              label: t("settings.launchDelay"),
              hint: t("settings.launchDelayHint"),
              value: settings.launchDelayMs,
              min: 0,
              max: 4000,
              step: 100,
              display: `${settings.launchDelayMs} ms`,
              onChange: (value) => set("launchDelayMs", value),
            },
          ],
        },
      ],
    },

    {
      id: "appearance",
      label: t("settings.catAppearance"),
      icon: "◑",
      sections: [
        {
          id: "theme",
          title: t("settings.theme"),
          hint: t("settings.themeHint"),
          items: [
            ...(workspace
              ? [
                  {
                    id: "themeScope",
                    kind: "toggle" as const,
                    label: t("settings.themePerWorkspace"),
                    hint: t("settings.themePerWorkspaceHint", { name: workspace.name }),
                    value: pinnedThemeId !== null,
                    onChange: (on: boolean) =>
                      patchWorkspace({ themeId: on ? effectiveThemeId : null }),
                  },
                ]
              : []),
            {
              id: "themePick",
              kind: "custom",
              keywords: `theme ${THEMES.map((theme) => theme.name).join(" ")}`,
              render: () => (
                <ThemeGrid t={t} activeId={effectiveThemeId} onPick={pickTheme} />
              ),
            },
          ],
        },
      ],
    },

    {
      id: "terminal",
      label: t("settings.catTerminal"),
      icon: "❯",
      sections: [
        {
          id: "preview",
          title: t("settings.preview"),
          items: [
            {
              id: "termPreview",
              kind: "custom",
              keywords: "preview aperçu",
              render: () => <TerminalPreview settings={settings} />,
            },
          ],
        },
        {
          id: "typography",
          title: t("settings.typography"),
          items: [
            {
              id: "font",
              kind: "custom",
              label: t("settings.font"),
              hint: t("settings.fontHint"),
              keywords: `font police ${FONT_STACKS.map((f) => f.label).join(" ")}`,
              render: () => (
                <FontGrid
                  value={settings.fontFamily}
                  customLabel={t("settings.fontCustom")}
                  onPick={(stack) => set("fontFamily", stack)}
                />
              ),
            },
            {
              id: "fontSize",
              kind: "slider",
              label: t("settings.size"),
              value: settings.fontSize,
              min: 9,
              max: 24,
              display: `${settings.fontSize} px`,
              onChange: (value) => set("fontSize", value),
            },
            {
              id: "lineHeight",
              kind: "slider",
              label: t("settings.lineHeight"),
              value: settings.lineHeight,
              min: 1,
              max: 2,
              step: 0.05,
              display: settings.lineHeight.toFixed(2),
              onChange: (value) => set("lineHeight", value),
            },
            {
              id: "letterSpacing",
              kind: "slider",
              label: t("settings.letterSpacing"),
              value: settings.letterSpacing,
              min: -1,
              max: 3,
              step: 0.5,
              display: `${settings.letterSpacing} px`,
              onChange: (value) => set("letterSpacing", value),
            },
            {
              id: "padding",
              kind: "slider",
              label: t("settings.padding"),
              value: settings.padding,
              min: 0,
              max: 28,
              display: `${settings.padding} px`,
              onChange: (value) => set("padding", value),
            },
          ],
        },
        {
          id: "cursor",
          title: t("settings.cursorSection"),
          items: [
            {
              id: "cursorStyle",
              kind: "segmented",
              label: t("settings.cursor"),
              value: settings.cursorStyle,
              choices: [
                { value: "bar", label: t("settings.cursorBar") },
                { value: "block", label: t("settings.cursorBlock") },
                { value: "underline", label: t("settings.cursorUnderline") },
              ],
              onChange: (value) => set("cursorStyle", value as Settings["cursorStyle"]),
            },
            {
              id: "cursorBlink",
              kind: "toggle",
              label: t("settings.blink"),
              value: settings.cursorBlink,
              onChange: (value) => set("cursorBlink", value),
            },
          ],
        },
        {
          id: "history",
          title: t("settings.history"),
          items: [
            {
              id: "scrollback",
              kind: "slider",
              label: t("settings.scrollback"),
              hint: t("settings.scrollbackLines", { n: settings.scrollback }),
              value: settings.scrollback,
              min: 1000,
              max: 50000,
              step: 1000,
              display: settings.scrollback.toLocaleString(),
              onChange: (value) => set("scrollback", value),
            },
          ],
        },
      ],
    },

    {
      id: "shell",
      label: t("settings.catShell"),
      icon: "$",
      sections: [
        {
          id: "shell",
          title: t("settings.shell"),
          hint: t("settings.shellDetected", { n: shells.length }),
          items: [
            {
              id: "shellDefault",
              kind: "select",
              label: t("settings.shellDefault"),
              value: settings.shellId ?? "",
              choices: [
                {
                  value: "",
                  label: t("settings.shellAuto", {
                    name: shells[0]?.label ?? t("create.shellNone"),
                  }),
                },
                ...shellChoices,
              ],
              onChange: (value) => set("shellId", value || null),
            },
            ...(workspace
              ? [
                  {
                    id: "shellWorkspace",
                    kind: "select" as const,
                    label: t("settings.shellWorkspace"),
                    hint: t("settings.shellWorkspaceHint"),
                    value: workspace.shellId ?? "",
                    choices: [
                      { value: "", label: t("settings.shellFollow") },
                      ...shellChoices,
                    ],
                    onChange: (value: string) =>
                      patchWorkspace({ shellId: value || null }),
                  },
                ]
              : []),
          ],
        },
      ],
    },

    {
      id: "notifications",
      label: t("settings.catNotifications"),
      icon: "◎",
      sections: [
        {
          id: "notifications",
          title: t("settings.notifications"),
          items: [
            {
              id: "notifyOnIdle",
              kind: "toggle",
              label: t("settings.notifyOnIdle"),
              hint: t("settings.notifyOnIdleHint"),
              value: settings.notifyOnIdle,
              onChange: (value) => set("notifyOnIdle", value),
            },
            {
              id: "notifyIdleMs",
              kind: "slider",
              label: t("settings.notifyIdle"),
              value: settings.notifyIdleMs,
              min: 1000,
              max: 15000,
              step: 500,
              display: `${(settings.notifyIdleMs / 1000).toFixed(1)} s`,
              onChange: (value) => set("notifyIdleMs", value),
            },
            {
              id: "notifyAway",
              kind: "toggle",
              label: t("settings.notifyAway"),
              hint: t("settings.notifyAwayHint"),
              value: settings.notifyOnlyWhenAway,
              onChange: (value) => set("notifyOnlyWhenAway", value),
            },
          ],
        },
      ],
    },

    {
      id: "discord",
      label: t("settings.catDiscord"),
      icon: "❖",
      sections: [
        {
          id: "discordPreview",
          title: t("settings.preview"),
          items: [
            {
              id: "discordCard",
              kind: "custom",
              keywords: "discord rich presence statut status carte card",
              render: () => {
                // Built as if it were on, so the toggles below can be judged
                // before publishing anything; the card greys out while off.
                const preview = buildPresence({
                  settings: { ...settings, discordPresence: true },
                  workspace,
                  focusedPaneId: null,
                  t,
                });
                return preview ? (
                  <DiscordCard
                    t={t}
                    presence={preview}
                    enabled={settings.discordPresence}
                  />
                ) : null;
              },
            },
          ],
        },
        {
          id: "discord",
          title: t("settings.discord"),
          hint: t("settings.discordHint"),
          items: [
            {
              id: "discordPresence",
              kind: "toggle",
              label: t("settings.discordEnable"),
              hint: t("settings.discordEnableHint"),
              keywords: "discord presence rpc",
              value: settings.discordPresence,
              onChange: (value) => set("discordPresence", value),
            },
            {
              id: "discordShowWorkspace",
              kind: "toggle",
              label: t("settings.discordWorkspace"),
              hint: t("settings.discordWorkspaceHint"),
              value: settings.discordShowWorkspace,
              onChange: (value) => set("discordShowWorkspace", value),
            },
            {
              id: "discordShowAgents",
              kind: "toggle",
              label: t("settings.discordAgents"),
              hint: t("settings.discordAgentsHint"),
              value: settings.discordShowAgents,
              onChange: (value) => set("discordShowAgents", value),
            },
          ],
        },
        {
          id: "discordApp",
          title: t("settings.discordApp"),
          items: [
            {
              id: "discordAppId",
              kind: "input",
              label: t("settings.discordAppId"),
              hint: t("settings.discordAppIdHint"),
              keywords: "discord application id client",
              value: settings.discordAppId,
              placeholder: t("settings.discordAppIdPlaceholder"),
              onChange: (value) => set("discordAppId", value.trim()),
            },
          ],
        },
      ],
    },

    {
      id: "shortcuts",
      label: t("settings.catShortcuts"),
      icon: "⌨",
      sections: [
        {
          id: "keymap",
          title: t("settings.shortcutsAll"),
          // The sequence it names is a real default, so it has to be spelled
          // the way this platform spells it.
          hint: t("settings.shortcutsHint", { mod: isMac() ? "Cmd" : "Ctrl" }),
          items: [
            {
              // One block rather than a row per command: the editor brings its
              // own filter, and every command carries several keys plus their
              // conflicts, which no label-and-control row could hold.
              id: "keymap",
              kind: "custom",
              keywords:
                "shortcut shortcuts keyboard key keys binding keymap raccourci raccourcis clavier touche ctrl alt shift",
              render: () => <ShortcutsSettings />,
            },
          ],
        },
      ],
    },

    {
      id: "about",
      label: t("settings.catAbout"),
      icon: "ⓘ",
      sections: [
        {
          id: "about",
          title: t("settings.catAbout"),
          items: [
            { id: "appName", kind: "text", label: "Hangar.AI", value: "hangar-ai" },
            ...(version
              ? [
                  {
                    id: "version",
                    kind: "text" as const,
                    label: t("settings.version"),
                    value: version,
                  },
                ]
              : []),
          ],
        },
        {
          id: "maintenance",
          title: t("settings.maintenance"),
          items: [
            {
              id: "reset",
              kind: "action",
              label: t("settings.reset"),
              hint: t("settings.resetHint"),
              actionLabel: t("settings.reset"),
              confirmLabel: t("settings.resetConfirm"),
              danger: true,
              onAct: ctx.resetSettings,
            },
          ],
        },
      ],
    },
  ];
}
