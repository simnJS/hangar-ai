import {
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import { Icon, type IconName } from "./Icon";
import { StateGlyph } from "./StateGlyph";
import {
  LOCALES,
  translate,
  useLocale,
  useT,
  type TranslateKey,
  type TranslateParams,
} from "../i18n";
import { useStore } from "../store";
import {
  getPaneActivity,
  useWorkspaceActivity,
  type AgentActivity,
} from "../lib/agentState";
import { leafIds, normalizeTree } from "../lib/layout";
import {
  commandSynonyms,
  filterPalette,
  highlight,
  type MatchRange,
  type PaletteEntry,
} from "../lib/paletteMatch";
import { mergeCommands } from "../lib/savedCommands";
import {
  COMMANDS,
  shortcutLabel,
  type CommandId,
  type Keymap,
  type ShortcutSection,
} from "../lib/shortcuts";
import { getTerminal } from "../lib/terminalRegistry";
import type { ShortcutHandlers } from "../lib/useShortcuts";
import { AGENTS, type Pane, type Workspace } from "../types";

/**
 * Every command the app can run right now, its panes and its workspaces,
 * behind one search field. The commands are the ones the keyboard shortcuts
 * dispatch at this moment (App.buildHandlers), labelled with their
 * `cmd.*` strings, so the palette never offers what a key would refuse.
 */
export interface CommandPaletteProps {
  onClose: () => void;
  /** What each command does right now; a command absent here is unavailable. */
  handlers: ShortcutHandlers;
  keymap: Keymap;
  /** In sidebar order. */
  workspaces: Workspace[];
  activeWorkspaceId: string | null;
  focusedPaneId: string | null;
  onFocusPane: (workspaceId: string, paneId: string) => void;
  onActivateWorkspace: (workspaceId: string) => void;
  /** « Ouvrir le chef de projet »: the same action as the navigator's button. */
  onOpenManager: () => void;
  /**
   * Entries the app adds for what is not a command of its own — the tabs of
   * the active workspace's kits — listed after the view commands.
   */
  extraEntries?: PaletteExtraEntry[];
}

/** An entry the app hands the palette, its words already translated. */
export interface PaletteExtraEntry {
  /** Unique among the extra entries. */
  key: string;
  /** Its heading. */
  group: string;
  label: string;
  detail?: string;
  /** More words it is found by. */
  keywords?: string[];
  icon?: IconName;
  run: () => void;
}

interface Entry extends PaletteEntry {
  /** Unique; also what the option's DOM id is made of. */
  key: string;
  icon: ReactNode;
  detail?: ReactNode;
  keys?: string;
  run: () => void;
  /**
   * The command puts the focus somewhere of its own — a dialog, a field, the
   * reset confirmation — and the terminal must not take it back.
   */
  ownsFocus?: boolean;
}

/**
 * The order groups are listed in with nothing typed. Pane commands share the
 * heading of the pane list and workspace commands that of the workspace list,
 * rather than repeating a heading with a different meaning under it.
 */
const SECTION_ORDER: ShortcutSection[] = [
  "panes",
  "navigation",
  "view",
  "terminal",
  "commands",
  "app",
  "workspaces",
];

/**
 * Commands the palette leaves out. The digit families are listed by name
 * instead — "Go to Ben" says more than "Focus pane 2" — and dictation is
 * push-to-talk: run from a list, the next key released would end a recording
 * nobody is speaking into. Cycling regions is relative to where the keyboard
 * is, and from the palette that is always the palette itself.
 */
const hidden = (id: CommandId) =>
  /^pane\.focus\d$/.test(id) ||
  /^workspace\.go\d$/.test(id) ||
  id === "voice.dictate" ||
  id === "view.nextRegion" ||
  id === "view.prevRegion";

/** Commands that leave the focus on what they opened. */
const OWNS_FOCUS = new Set<CommandId>([
  "view.settings",
  "view.shortcuts",
  "view.mcp",
  "workspace.mcpServers",
  "view.broadcast",
  "workspace.new",
  "pane.sessions",
  "pane.resetAll",
  "terminal.find",
  "view.focusNavigator",
  "view.focusInspector",
  "workspace.orchestration",
  "workspace.stopWorkers",
  "view.needsYou",
]);

const ICONS: Partial<Record<CommandId, IconName>> = {
  "pane.split": "splitRight",
  "pane.splitRight": "splitRight",
  "pane.splitDown": "splitDown",
  "pane.close": "close",
  "pane.restart": "restart",
  "pane.restartAll": "restart",
  "pane.resetAll": "reset",
  "pane.tidy": "tidy",
  "pane.sessions": "clock",
  "pane.zoom": "expand",
  "workspace.new": "folderPlus",
  "view.settings": "gear",
  "view.shortcuts": "keyboard",
  "view.terminals": "terminal",
  "view.board": "board",
  "view.boardDock": "board",
  "view.modelPreview": "cube",
  "view.closeModels": "close",
  "view.memory": "bookmark",
  "view.stats": "chart",
  "view.mcp": "plug",
  "workspace.mcpServers": "plug",
  "view.broadcast": "broadcast",
  "view.navigator": "sidebarLeft",
  "view.inspector": "sidebarRight",
  "view.focusNavigator": "sidebarLeft",
  "view.focusInspector": "sidebarRight",
  "terminal.find": "search",
  "app.fullscreen": "expand",
  "pane.pinOrchestrator": "orchestrator",
  "pane.unpinOrchestrator": "orchestrator",
  "workspace.newOrchestrator": "orchestrator",
  "workspace.orchestration": "sliders",
  "workspace.stopWorkers": "stop",
  "workspace.sleep": "moon",
  "view.needsYou": "flag",
};

const SECTION_ICONS: Record<ShortcutSection, IconName> = {
  panes: "splitRight",
  navigation: "arrowRight",
  workspaces: "folder",
  view: "window",
  terminal: "terminal",
  commands: "bolt",
  voice: "mic",
  app: "window",
};

/**
 * What the toolbar calls the same commands. Someone who saw "Réinitialiser les
 * agents" on a button types that, not the longer sentence the shortcut list
 * uses, and should still find it.
 */
const ALIASES: Partial<Record<CommandId, TranslateKey[]>> = {
  "pane.split": ["toolbar.newTerminal"],
  "pane.restartAll": ["topbar.restartAll"],
  "pane.resetAll": ["topbar.resetAll"],
  "pane.tidy": ["topbar.tidy"],
  "view.boardDock": ["board.dock"],
  "view.modelPreview": ["model.panel"],
  "view.inspector": ["toolbar.inspector"],
  "view.broadcast": ["broadcast.send"],
  "pane.pinOrchestrator": ["orch.pin"],
  "workspace.orchestration": ["orch.title"],
  "workspace.sleep": ["sleep.action"],
  "view.needsYou": ["needsYou.title"],
};

const STATE_WORD = {
  working: "pane.state.working",
  waiting: "pane.state.waiting",
  yours: "pane.state.yours",
  idle: "pane.state.idle",
} as const;

/** Pane ids as they sit on screen, which is what Ctrl+N counts in. */
const readingOrder = (ws: Workspace) =>
  leafIds(normalizeTree(ws.panes, ws.tree)).filter(Boolean);

export function CommandPalette({
  onClose,
  handlers,
  keymap,
  workspaces,
  activeWorkspaceId,
  focusedPaneId,
  onFocusPane,
  onActivateWorkspace,
  onOpenManager,
  extraEntries = [],
}: CommandPaletteProps) {
  const t = useT();
  const locale = useLocale();
  const { state } = useStore();
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState(0);
  const listRef = useRef<HTMLDivElement>(null);
  // Where the keyboard was, to hand it back when the palette is dismissed.
  const [opener] = useState(() => document.activeElement as HTMLElement | null);

  // States are read when the list is built; this re-renders it when the
  // active workspace's agents change state while it is open.
  useWorkspaceActivity(activeWorkspaceId ?? "");

  /** "Attend ta réponse" reads "attend ta réponse" after a "·". */
  const lower = (text: string) => text.charAt(0).toLocaleLowerCase(locale) + text.slice(1);
  const agentLabel = (pane: Pane) =>
    pane.agent === "shell"
      ? t("pane.shell")
      : (AGENTS.find((agent) => agent.id === pane.agent)?.label ?? pane.agent);
  const keysOf = (id: CommandId) => shortcutLabel(keymap, id);
  /**
   * The same words in the languages not shown, as hidden keywords: someone
   * who reads the app in French may still type "settings", and the other
   * way round.
   */
  const elsewhere = (key: TranslateKey, params?: TranslateParams) =>
    LOCALES.filter((entry) => entry.id !== locale).map((entry) =>
      translate(entry.id, key, params),
    );

  const active = workspaces.find((ws) => ws.id === activeWorkspaceId) ?? null;

  // Built on every render rather than memoised: agent states are read from
  // outside React, and the subscription above is what asks for a new render.
  const entries = (() => {
    const list: Entry[] = [];
    const panesGroup = t("keymap.section.panes");
    const workspacesGroup = t("keymap.section.workspaces");

    const paneDetail = (ws: Workspace, pane: Pane, activity: AgentActivity) => (
      <>
        {ws.id !== activeWorkspaceId && `${ws.name} · `}
        {agentLabel(pane)} ·{" "}
        <span className={`palette__state palette__state--${activity}`}>
          {lower(t(STATE_WORD[activity]))}
        </span>
      </>
    );

    const paneEntry = (
      ws: Workspace,
      pane: Pane,
      activity: AgentActivity,
      group: string,
    ): Entry => {
      const at = ws.id === activeWorkspaceId ? readingOrder(ws).indexOf(pane.id) : -1;
      return {
        key: `pane:${pane.id}`,
        group,
        label: t("palette.goTo", { name: pane.name }),
        keywords: [
          pane.name,
          agentLabel(pane),
          t(STATE_WORD[activity]),
          ws.name,
          ...elsewhere("palette.goTo", { name: pane.name }),
          ...elsewhere(STATE_WORD[activity]),
        ],
        icon: <StateGlyph state={activity} size={7} />,
        detail: paneDetail(ws, pane, activity),
        keys: at >= 0 && at < 9 ? keysOf(`pane.focus${at + 1}` as CommandId) : "",
        run: () => onFocusPane(ws.id, pane.id),
      };
    };

    // What needs you, wherever it is: the active workspace first, then the
    // others in sidebar order.
    const suggested = new Set<string>();
    const near = active ? [active, ...workspaces.filter((ws) => ws !== active)] : workspaces;
    for (const wanted of ["waiting", "yours"] as const) {
      for (const ws of near) {
        for (const id of readingOrder(ws)) {
          const pane = ws.panes.find((entry) => entry.id === id);
          if (!pane || getPaneActivity(id)?.activity !== wanted) continue;
          suggested.add(id);
          list.push(paneEntry(ws, pane, wanted, t("palette.suggestions")));
        }
      }
    }

    if (active) {
      for (const id of readingOrder(active)) {
        const pane = active.panes.find((entry) => entry.id === id);
        if (!pane || suggested.has(id)) continue;
        list.push(paneEntry(active, pane, getPaneActivity(id)?.activity ?? "idle", panesGroup));
      }
    }

    const saved = active ? mergeCommands(active, state.settings) : [];
    for (const section of SECTION_ORDER) {
      const group = t(`keymap.section.${section}`);

      // The other workspaces lead their group, before the commands about them.
      if (section === "workspaces") {
        workspaces.forEach((ws, index) => {
          if (ws.id === activeWorkspaceId) return;
          const counts = { working: 0, waiting: 0, yours: 0 };
          for (const pane of ws.panes) {
            const activity = getPaneActivity(pane.id)?.activity;
            if (activity && activity !== "idle") counts[activity] += 1;
          }
          const [kind, text] = counts.waiting
            ? (["waiting", t("sidebar.activityWaiting", { n: counts.waiting })] as const)
            : counts.yours
              ? (["yours", t("sidebar.activityYours", { n: counts.yours })] as const)
              : counts.working
                ? (["working", t("sidebar.activityWorking", { n: counts.working })] as const)
                : ([null, ws.detached ? t("palette.inWindow") : t("status.panes", { n: ws.panes.length })] as const);
          list.push({
            key: `ws:${ws.id}`,
            group,
            label: t("palette.open", { name: ws.name }),
            keywords: [ws.cwd, ...elsewhere("palette.open", { name: ws.name })],
            icon: <Icon name="folder" size={13} />,
            detail: kind ? (
              <span className={`palette__state palette__state--${kind}`}>{text}</span>
            ) : (
              text
            ),
            keys: index < 9 ? keysOf(`workspace.go${index + 1}` as CommandId) : "",
            run: () => onActivateWorkspace(ws.id),
          });
        });
      }

      if (section === "workspaces") {
        list.push({
          key: "manager:open",
          group,
          label: t("manager.open"),
          keywords: [
            ...elsewhere("manager.open"),
            t("manager.hqName"),
            ...elsewhere("manager.hqName"),
            "manager",
            "HQ",
          ],
          icon: <Icon name="orchestrator" size={13} />,
          keys: "",
          run: onOpenManager,
        });
      }

      for (const command of COMMANDS) {
        if (command.section !== section || hidden(command.id)) continue;
        const run = handlers[command.id];
        if (!run) continue;
        // A saved command is listed by what it runs, not by its slot number.
        const slot = /^command\.run(\d)$/.exec(command.id);
        const savedCommand = slot ? saved[Number(slot[1]) - 1] : null;
        list.push({
          key: `cmd:${command.id}`,
          group: section === "panes" ? panesGroup : section === "workspaces" ? workspacesGroup : group,
          label: savedCommand
            ? t("palette.runSaved", { label: savedCommand.label.trim() || savedCommand.command })
            : t(command.labelKey, command.labelParams),
          keywords: [
            ...(savedCommand
              ? [savedCommand.command]
              : elsewhere(command.labelKey, command.labelParams)),
            ...(ALIASES[command.id] ?? []).flatMap((key) => [t(key), ...elsewhere(key)]),
            ...commandSynonyms(command.id),
            group,
            ...elsewhere(`keymap.section.${section}`),
          ],
          icon: <Icon name={ICONS[command.id] ?? SECTION_ICONS[section]} size={13} />,
          keys: keysOf(command.id),
          run,
          ownsFocus: OWNS_FOCUS.has(command.id),
        });
      }

      if (section === "view") {
        for (const extra of extraEntries) {
          list.push({
            key: `extra:${extra.key}`,
            group: extra.group,
            label: extra.label,
            keywords: [...(extra.keywords ?? []), extra.group],
            icon: <Icon name={extra.icon ?? "window"} size={13} />,
            detail: extra.detail,
            keys: "",
            run: extra.run,
          });
        }
      }
    }
    return list;
  })();

  const groups = filterPalette(entries, query);
  const flat = groups.flatMap((group) => group.entries);
  const current = Math.min(selected, Math.max(0, flat.length - 1));
  const optionId = (index: number) => `palette-option-${index}`;

  // The selected row stays in view; the first one brings its heading along.
  useLayoutEffect(() => {
    const list = listRef.current;
    if (!list) return;
    if (current === 0) list.scrollTop = 0;
    else document.getElementById(optionId(current))?.scrollIntoView({ block: "nearest" });
  }, [current, query]);

  /** Dismissed without running anything: the keyboard goes back where it was. */
  const dismiss = () => {
    onClose();
    requestAnimationFrame(() => {
      if (opener && opener !== document.body && opener.isConnected) opener.focus();
      else getTerminal(focusedPaneId)?.focus();
    });
  };

  /**
   * Closed first, run on the next frame: whatever the command opens must not
   * open under the palette, or be closed with it.
   */
  const run = (entry: Entry) => {
    onClose();
    requestAnimationFrame(() => {
      entry.run();
      if (entry.ownsFocus) return;
      // A pane the command focused takes the keyboard on its own; anything
      // else hands it back to the terminal that had it.
      requestAnimationFrame(() => {
        const at = document.activeElement;
        if (!at || at === document.body) getTerminal(focusedPaneId)?.focus();
      });
    });
  };

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.nativeEvent.isComposing) return;
    const count = flat.length;
    switch (event.key) {
      case "ArrowDown":
        event.preventDefault();
        if (count) setSelected((current + 1) % count);
        break;
      case "ArrowUp":
        event.preventDefault();
        if (count) setSelected((current - 1 + count) % count);
        break;
      case "Enter":
        event.preventDefault();
        if (flat[current]) run(flat[current].entry);
        break;
      case "Escape":
        event.preventDefault();
        event.stopPropagation();
        dismiss();
        break;
      // The field is the dialog's only stop: the focus stays in it.
      case "Tab":
        event.preventDefault();
        break;
    }
  };

  const label = (text: string, ranges: MatchRange[]) =>
    highlight(text, ranges).map((part, i) =>
      part.hit ? <mark key={i}>{part.text}</mark> : <span key={i}>{part.text}</span>,
    );

  // Pressed and released on the scrim, like the app's other dialogs: a drag
  // that started in the field must not close the palette.
  const pressedScrim = useRef(false);
  const paletteKeys = keysOf("view.palette");
  let index = -1;

  return (
    <div
      className="palette"
      onMouseDown={(event) => {
        pressedScrim.current = event.target === event.currentTarget;
      }}
      onMouseUp={(event) => {
        const close = pressedScrim.current && event.target === event.currentTarget;
        pressedScrim.current = false;
        if (close) dismiss();
      }}
    >
      <div
        className="palette__panel"
        role="dialog"
        aria-modal="true"
        aria-label={t("palette.label")}
        // Nothing in the panel but the field takes the focus, so a click on a
        // row or a heading never leaves the keyboard nowhere.
        onMouseDown={(event) => {
          if ((event.target as HTMLElement).tagName !== "INPUT") event.preventDefault();
        }}
      >
        <div className="palette__search">
          <Icon name="search" size={18} />
          <input
            className="palette__input"
            autoFocus
            value={query}
            placeholder={t("palette.placeholder")}
            onChange={(event) => {
              setQuery(event.target.value);
              setSelected(0);
            }}
            onKeyDown={onKeyDown}
            role="combobox"
            aria-expanded="true"
            aria-controls="palette-list"
            aria-autocomplete="list"
            aria-activedescendant={flat.length ? optionId(current) : undefined}
            aria-label={t("palette.label")}
            autoComplete="off"
            spellCheck={false}
          />
          <button
            type="button"
            className="palette__esc"
            tabIndex={-1}
            onClick={dismiss}
            aria-label={t("palette.close")}
          >
            {t("palette.escKey")}
          </button>
        </div>

        <div className="palette__list" ref={listRef}>
          <div id="palette-list" role="listbox" aria-label={t("palette.results")}>
            {groups.map((group, groupIndex) => (
              <div
                key={group.group}
                role="group"
                aria-labelledby={`palette-group-${groupIndex}`}
                className="palette__group"
              >
                <div
                  className="palette__heading"
                  id={`palette-group-${groupIndex}`}
                  role="presentation"
                >
                  {group.group}
                </div>
                {group.entries.map(({ entry, match }) => {
                  index += 1;
                  const at = index;
                  const on = at === current;
                  return (
                    <div
                      key={entry.key}
                      id={optionId(at)}
                      role="option"
                      aria-selected={on}
                      className={`palette__option ${on ? "is-selected" : ""}`}
                      onMouseMove={() => at !== current && setSelected(at)}
                      onClick={() => run(entry)}
                    >
                      <span className="palette__icon" aria-hidden="true">
                        {entry.icon}
                      </span>
                      <span className="palette__label">{label(entry.label, match.ranges)}</span>
                      {entry.detail && <span className="palette__detail">{entry.detail}</span>}
                      {entry.keys && <kbd className="palette__keys">{entry.keys}</kbd>}
                    </div>
                  );
                })}
              </div>
            ))}
          </div>
          {!flat.length && (
            <p className="palette__empty" role="status">
              {t("palette.empty", { query: query.trim() })}
            </p>
          )}
        </div>

        <div className="palette__foot" aria-hidden="true">
          <span>
            <kbd>↑↓</kbd>
            {t("palette.navigate")}
          </span>
          <span>
            <kbd>↵</kbd>
            {t("palette.run")}
          </span>
          <span>
            <kbd>{t("palette.escKey")}</kbd>
            {t("palette.close")}
          </span>
          {paletteKeys && (
            <span className="palette__foot-end">
              {t("palette.anywhere", { keys: paletteKeys })}
            </span>
          )}
        </div>
      </div>
    </div>
  );
}
