import { ConnectedAccountSwitcher } from "./accounts/ConnectedAccountSwitcher";
import { useEffect, useId, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { Icon, type IconName } from "./Icon";
import { StateGlyph } from "./StateGlyph";
import { ToolbarActivity } from "./ToolbarActivity";
import { ToolbarMenu, type MenuCloseReason } from "./ToolbarMenu";
import { useT } from "../i18n";
import { getPaneActivity, type AgentActivity } from "../lib/agentState";
import { closedByPreset, MAX_PANES } from "../lib/layout";
import { useShortcutLabel, useShortcutTitle } from "../lib/useShortcuts";
import type { LayoutSize, Pane, Workspace } from "../types";

export type View = "terminals" | "board" | "memory" | "stats";

export const LAYOUTS: LayoutSize[] = [1, 2, 4, 8];

const VIEWS: { id: View; icon: IconName }[] = [
  { id: "terminals", icon: "terminal" },
  { id: "board", icon: "board" },
  { id: "memory", icon: "bookmark" },
  { id: "stats", icon: "chart" },
];

const LAYOUT_ICONS: Record<LayoutSize, IconName> = {
  1: "layout1",
  2: "layout2",
  4: "layout4",
  8: "layout8",
};

/** A kit tab the « Kits » menu opens (lib/kitTabs). */
export interface KitMenuEntry {
  key: string;
  /** As the kit's manifest names the tab. */
  title: string;
  /** The kit's name. */
  kit: string;
  onOpen: () => void;
}

/**
 * The bar over the workspace: which view, how the panes are laid out, and
 * the actions on all of them at once. Everything it does is the app's — it
 * only lays the controls out and reports clicks.
 */
export interface ToolbarProps {
  workspace: Workspace;
  view: View;
  onView: (view: View) => void;
  panes: Pane[];
  focusedPaneId: string | null;
  onFocusPane: (paneId: string) => void;
  dockShown: boolean;
  onToggleDock: () => void;
  /** An agent has a model open: the 3D preview's button is there at all. */
  modelsOpen: boolean;
  modelShown: boolean;
  /** Opened or rewritten since the preview was last on screen. */
  modelsUnseen: boolean;
  onToggleModels: () => void;
  /**
   * The tabs of the workspace's kits, for the « Kits » menu; null when no kit
   * is enabled on it, and the button is not there.
   */
  kitTabs: KitMenuEntry[] | null;
  /** The installed kits are still being read. */
  kitsLoading: boolean;
  /** The main window can send this workspace to a window of its own. */
  canDetach: boolean;
  onDetach: () => void;
  /** This is the workspace's own window, which can hand it back. */
  inOwnWindow: boolean;
  onReattach: () => void;
  onPreset: (size: LayoutSize) => void;
  onTidy: () => void;
  onSplit: () => void;
  onRestartAll: () => void;
  /** Panes running an agent: what "reset agents" would start afresh. */
  agentPaneCount: number;
  /** The reset is asking "are you sure?" instead of acting. */
  confirmReset: boolean;
  onArmReset: () => void;
  onCancelReset: () => void;
  onReset: () => void;
  /** Only the main window has a navigator to show or hide. */
  hasNavigator: boolean;
  navigatorOpen: boolean;
  onToggleNavigator: () => void;
  inspectorOpen: boolean;
  onToggleInspector: () => void;
  onOpenPalette: () => void;
  /** « Besoin de toi », first in the trail; nothing while nothing needs you. */
  needsYou?: ReactNode;
  /**
   * The workspace has an orchestrator or workers: the menu offers its
   * orchestration settings, and stopping its workers when it has any.
   */
  orchestrated: boolean;
  workerCount: number;
  onOpenOrchestration: () => void;
  /** Stopping the workers is asking "are you sure?" instead of acting. */
  confirmStopWorkers: boolean;
  onArmStopWorkers: () => void;
  onCancelStopWorkers: () => void;
  onStopWorkers: () => void;
}

export function Toolbar({
  workspace,
  view,
  onView,
  panes,
  focusedPaneId,
  onFocusPane,
  dockShown,
  onToggleDock,
  modelsOpen,
  modelShown,
  modelsUnseen,
  onToggleModels,
  kitTabs,
  kitsLoading,
  canDetach,
  onDetach,
  inOwnWindow,
  onReattach,
  onPreset,
  onTidy,
  onSplit,
  onRestartAll,
  agentPaneCount,
  confirmReset,
  onArmReset,
  onCancelReset,
  onReset,
  hasNavigator,
  navigatorOpen,
  onToggleNavigator,
  inspectorOpen,
  onToggleInspector,
  onOpenPalette,
  needsYou,
  orchestrated,
  workerCount,
  onOpenOrchestration,
  confirmStopWorkers,
  onArmStopWorkers,
  onCancelStopWorkers,
  onStopWorkers,
}: ToolbarProps) {
  const t = useT();
  const withKeys = useShortcutTitle();
  const keys = useShortcutLabel();
  const terminals = view === "terminals";

  const moreRef = useRef<HTMLButtonElement>(null);
  /** Opened from its button. An armed reset opens it too, from anywhere. */
  const [menuOpen, setMenuOpen] = useState(false);
  /** Either question, asked in the menu's place. */
  const confirming = confirmReset || confirmStopWorkers;
  const menuShown = menuOpen || confirming;
  /** The terminal actions did not fit in the bar and are listed in the menu. */
  const [folded, setFolded] = useState(false);
  const actionsRef = useRef<HTMLDivElement>(null);
  const confirmId = useId();
  const kitsRef = useRef<HTMLButtonElement>(null);
  const [kitsOpen, setKitsOpen] = useState(false);
  const closeKits = (reason: MenuCloseReason) => {
    setKitsOpen(false);
    if (reason === "escape" || reason === "tab") kitsRef.current?.focus();
  };
  // Another workspace's kits are another menu.
  useEffect(() => setKitsOpen(false), [workspace.id]);

  /**
   * A smaller preset closes the panes read last. When one of them still has
   * an agent at work, waiting on you or with an answer you have not read, the
   * segmented control — which looks like a harmless view switch — asks first,
   * from the button that was pressed.
   */
  const [presetAsk, setPresetAsk] = useState<{
    size: LayoutSize;
    closing: Pane[];
    busy: { pane: Pane; activity: AgentActivity }[];
  } | null>(null);
  const presetRef = useRef<HTMLButtonElement | null>(null);

  const choosePreset = (size: LayoutSize, button: HTMLButtonElement) => {
    const closing = closedByPreset(panes, workspace.tree, size);
    const busy = closing.flatMap((pane) => {
      const activity = getPaneActivity(pane.id)?.activity ?? "idle";
      return activity === "idle" ? [] : [{ pane, activity }];
    });
    if (!busy.length) return onPreset(size);
    presetRef.current = button;
    setPresetAsk({ size, closing, busy });
  };

  const closePresetAsk = (reason: MenuCloseReason) => {
    setPresetAsk(null);
    if (reason === "escape" || reason === "tab") presetRef.current?.focus();
  };

  // A question about this workspace's panes, in this view, and nowhere else —
  // and one at a time: a reset armed from its shortcut takes over.
  useEffect(() => {
    setPresetAsk(null);
  }, [workspace.id, view, confirmReset]);

  /** "Ben", "Ben and Cleo", "Ben, Cleo and Dan". */
  const nameList = (names: string[]) =>
    names.length < 2
      ? names.join("")
      : t("toolbar.presetNames", { names: names.slice(0, -1).join(", "), last: names[names.length - 1] });

  // The reset went through, or was called off elsewhere (a view change): the
  // menu it was showing in has nothing left to say.
  const wasConfirming = useRef(confirming);
  useEffect(() => {
    if (wasConfirming.current && !confirming) setMenuOpen(false);
    wasConfirming.current = confirming;
  }, [confirming]);

  /**
   * Escape and Tab give the keyboard back to the button that opened the menu.
   * A reset armed from a shortcut or the palette had no button: calling it off
   * hands the keyboard back to the terminal, which is what App's cancel does.
   */
  const closeMenu = (reason: MenuCloseReason) => {
    const fromButton = menuOpen;
    if (confirmReset) onCancelReset();
    if (confirmStopWorkers) onCancelStopWorkers();
    setMenuOpen(false);
    if ((reason === "escape" || reason === "tab") && fromButton) moreRef.current?.focus();
  };

  /** A menu entry runs once the menu is gone, so it never fights it for focus. */
  const pick = (action: () => void) => () => {
    setMenuOpen(false);
    action();
  };

  // Arrows walk the views like the radio group they are; the focus follows.
  const onViewKey = (event: KeyboardEvent<HTMLDivElement>) => {
    const at = VIEWS.findIndex((entry) => entry.id === view);
    const step =
      event.key === "ArrowRight" || event.key === "ArrowDown"
        ? 1
        : event.key === "ArrowLeft" || event.key === "ArrowUp"
          ? -1
          : 0;
    let next = step ? (at + step + VIEWS.length) % VIEWS.length : -1;
    if (event.key === "Home") next = 0;
    if (event.key === "End") next = VIEWS.length - 1;
    if (next < 0) return;
    event.preventDefault();
    onView(VIEWS[next].id);
    const radio = event.currentTarget.querySelectorAll<HTMLElement>('[role="radio"]')[next];
    radio?.focus();
    // Coming back to the terminals, the focused pane takes the keyboard as its
    // grid reappears. Walking the views with the arrows is not asking for that:
    // the next arrow would land in the terminal. Click a view to get it.
    requestAnimationFrame(() => radio?.focus());
  };

  const paletteKeys = keys("view.palette");
  const resetKeys = keys("pane.resetAll");

  return (
    <header className="toolbar">
      <div className="toolbar__lead">
        {hasNavigator && !navigatorOpen && (
          <button
            type="button"
            className="tbtn"
            onClick={onToggleNavigator}
            title={withKeys(t("toolbar.showNavigator"), "view.navigator")}
            aria-label={t("toolbar.showNavigator")}
          >
            <Icon name="sidebarLeft" size={16} />
          </button>
        )}

        <div className="toolbar__title">
          {/* The name alone: its folder is a detail, in the tooltip here and
              in the inspector's session. */}
          <h1 className="toolbar__name" title={`${workspace.name}\n${workspace.cwd}`}>
            {workspace.name}
          </h1>
        </div>

        <div
          className="segmented segmented--views"
          role="radiogroup"
          aria-label={t("view.group")}
          onKeyDown={onViewKey}
        >
          {VIEWS.map(({ id, icon }) => (
            <button
              key={id}
              type="button"
              role="radio"
              aria-checked={view === id}
              tabIndex={view === id ? 0 : -1}
              className="segmented__btn"
              onClick={() => onView(id)}
              title={withKeys(t(`cmd.view.${id}`), `view.${id}`)}
            >
              <span className="segmented__icon">
                <Icon name={icon} size={15} />
              </span>
              <span className="segmented__label">{t(`view.${id}`)}</span>
            </button>
          ))}
        </div>
      </div>

      <ToolbarActivity
        workspace={workspace}
        panes={panes}
        focusedPaneId={focusedPaneId}
        view={view}
        onView={onView}
        onFocusPane={onFocusPane}
      />

      <div className="toolbar__trail">
        {needsYou}
        {/* Claude account of this workspace: one click switches it (Comptes Claude). */}
        <ConnectedAccountSwitcher workspaceId={workspace.id} />
        {terminals && (
          <>
            <div className="segmented segmented--icons" role="group" aria-label={t("topbar.layout")}>
              {LAYOUTS.map((size) => (
                <button
                  key={size}
                  type="button"
                  className="segmented__btn"
                  aria-pressed={panes.length === size}
                  onClick={(event) => choosePreset(size, event.currentTarget)}
                  title={t("topbar.presetHint", { n: size })}
                  aria-label={t("topbar.presetHint", { n: size })}
                >
                  <Icon name={LAYOUT_ICONS[size]} size={16} />
                </button>
              ))}
            </div>
            {/* Folded into the "…" menu when the toolbar runs out of room;
                the presets stay, being the only way to them. */}
            <div className="toolbar__actions" ref={actionsRef}>
              {/* For the counts no preset matches, and for a grid that splits
                  and moves have pulled out of shape. */}
              <button
                type="button"
                className="tbtn"
                onClick={onTidy}
                disabled={panes.length < 2}
                title={withKeys(t("toolbar.tidyHint"), "pane.tidy")}
                aria-label={t("topbar.tidy")}
              >
                <Icon name="tidy" size={16} />
              </button>
              <span className="toolbar__sep" aria-hidden="true" />
              <button
                type="button"
                className="tbtn"
                onClick={onSplit}
                disabled={panes.length >= MAX_PANES}
                title={withKeys(t("toolbar.newTerminalHint"), "pane.split")}
                aria-label={t("toolbar.newTerminal")}
              >
                <Icon name="plus" size={16} />
              </button>
              <button
                type="button"
                className="tbtn"
                onClick={onRestartAll}
                title={withKeys(t("topbar.restartAllHint"), "pane.restartAll")}
                aria-label={t("topbar.restartAll")}
              >
                <Icon name="restart" size={15} />
              </button>
            </div>
          </>
        )}

        <button
          ref={moreRef}
          type="button"
          className={`tbtn ${menuShown ? "is-open" : ""}`}
          onClick={() => {
            if (menuShown) return closeMenu("outside");
            // Read when it opens: whether these fit is the toolbar's width
            // at this moment, which only CSS knows.
            const actions = actionsRef.current;
            setFolded(actions ? getComputedStyle(actions).display === "none" : false);
            setMenuOpen(true);
          }}
          title={t("toolbar.more")}
          aria-label={t("toolbar.more")}
          aria-haspopup="menu"
          aria-expanded={menuShown}
        >
          <Icon name="more" size={16} />
        </button>

        <span className="toolbar__sep" aria-hidden="true" />

        {kitTabs && (
          <button
            ref={kitsRef}
            type="button"
            className={`tbtn ${kitsOpen ? "is-open" : ""}`}
            onClick={() => setKitsOpen((open) => !open)}
            title={t("kitsTab.menuHint")}
            aria-label={t("kitsTab.menu")}
            aria-haspopup="menu"
            aria-expanded={kitsOpen}
          >
            <Icon name="window" size={16} />
          </button>
        )}
        {modelsOpen && (
          <button
            type="button"
            className={`tbtn tbtn--toggle ${modelsUnseen ? "tbtn--unseen" : ""}`}
            onClick={onToggleModels}
            aria-pressed={modelShown}
            data-drawer-toggle=""
            title={withKeys(t("model.toggleHint"), "view.modelPreview")}
            aria-label={t("model.panel")}
          >
            <Icon name="cube" size={16} />
          </button>
        )}
        <button
          type="button"
          className="tbtn tbtn--toggle"
          onClick={onToggleDock}
          aria-pressed={dockShown}
          // Opens and closes a drawer itself: a press on it is not "elsewhere".
          data-drawer-toggle=""
          title={withKeys(t("board.dockHint"), "view.boardDock")}
          aria-label={t("board.dock")}
        >
          <Icon name="board" size={16} />
        </button>
        <button
          type="button"
          className="tbtn tbtn--toggle"
          onClick={onToggleInspector}
          aria-pressed={inspectorOpen}
          data-drawer-toggle=""
          title={withKeys(t("cmd.view.inspector"), "view.inspector")}
          aria-label={t("toolbar.inspector")}
        >
          <Icon name="sidebarRight" size={16} />
        </button>
      </div>

      {kitsOpen && kitTabs && (
        <ToolbarMenu anchorRef={kitsRef} label={t("kitsTab.menu")} onClose={closeKits}>
          {kitTabs.length ? (
            kitTabs.map((entry) => (
              <button
                key={entry.key}
                type="button"
                role="menuitem"
                className="tmenu__item"
                onClick={() => {
                  setKitsOpen(false);
                  entry.onOpen();
                }}
              >
                <Icon name="window" size={14} />
                <span className="tmenu__label">{entry.title}</span>
                <span className="tmenu__keys">{entry.kit}</span>
              </button>
            ))
          ) : (
            <button type="button" role="menuitem" className="tmenu__item" disabled>
              <span className="tmenu__label">
                {kitsLoading ? t("kitsTab.menuLoading") : t("kitsTab.menuNone")}
              </span>
            </button>
          )}
        </ToolbarMenu>
      )}

      {presetAsk && (
        <ToolbarMenu
          anchorRef={presetRef}
          role="alertdialog"
          labelledBy={`${confirmId}-preset-title`}
          describedBy={`${confirmId}-preset-body`}
          onClose={closePresetAsk}
        >
          <p className="tmenu__title" id={`${confirmId}-preset-title`}>
            {t("toolbar.presetTitle", { names: nameList(presetAsk.closing.map((pane) => pane.name)) })}
          </p>
          <p className="tmenu__body" id={`${confirmId}-preset-body`}>
            {t("toolbar.presetBody", { n: presetAsk.closing.length })}
          </p>
          {/* Who is still busy, in the state language of the rest of the app. */}
          <ul className="tmenu__panes" aria-label={t("toolbar.presetBusy")}>
            {presetAsk.busy.map(({ pane, activity }) => (
              <li key={pane.id} className="tmenu__pane">
                <StateGlyph state={activity} size={7} />
                <span className="tmenu__pane-name">{pane.name}</span>
                <span className="tmenu__pane-state">{t(`pane.state.${activity}`)}</span>
              </li>
            ))}
          </ul>
          <div className="tmenu__actions">
            <button
              type="button"
              className="btn btn--ghost"
              onClick={() => closePresetAsk("escape")}
              // As for the reset: the answer that loses nothing has the keyboard.
              autoFocus
            >
              {t("create.cancel")}
            </button>
            <button
              type="button"
              className="btn btn--danger"
              onClick={() => {
                const { size } = presetAsk;
                setPresetAsk(null);
                presetRef.current?.focus();
                onPreset(size);
              }}
            >
              {t("toolbar.presetConfirm", { n: presetAsk.closing.length })}
            </button>
          </div>
        </ToolbarMenu>
      )}

      {menuShown &&
        (confirmReset ? (
          <ToolbarMenu
            anchorRef={moreRef}
            role="alertdialog"
            labelledBy={`${confirmId}-title`}
            describedBy={`${confirmId}-body`}
            onClose={closeMenu}
          >
            <p className="tmenu__title" id={`${confirmId}-title`}>
              {t("toolbar.resetTitle", { n: agentPaneCount })}
            </p>
            <p className="tmenu__body" id={`${confirmId}-body`}>
              {t("toolbar.resetBody")}
            </p>
            <div className="tmenu__actions">
              <button
                type="button"
                className="btn btn--ghost"
                onClick={() => closeMenu("escape")}
                // The safe answer has the keyboard: every conversation of the
                // workspace is one deliberate Tab and Enter away, never two
                // Enters from the "…" button.
                autoFocus
              >
                {t("create.cancel")}
              </button>
              <button type="button" className="btn btn--danger" onClick={onReset}>
                {t("topbar.resetAllConfirm", { n: agentPaneCount })}
              </button>
            </div>
          </ToolbarMenu>
        ) : confirmStopWorkers ? (
          <ToolbarMenu
            anchorRef={moreRef}
            role="alertdialog"
            labelledBy={`${confirmId}-stop-title`}
            describedBy={`${confirmId}-stop-body`}
            onClose={closeMenu}
          >
            <p className="tmenu__title" id={`${confirmId}-stop-title`}>
              {t("orch.stopTitle", { n: workerCount })}
            </p>
            <p className="tmenu__body" id={`${confirmId}-stop-body`}>
              {t("orch.stopBody")}
            </p>
            <div className="tmenu__actions">
              <button
                type="button"
                className="btn btn--ghost"
                onClick={() => closeMenu("escape")}
                // As for the reset: the answer that stops nothing has the
                // keyboard.
                autoFocus
              >
                {t("create.cancel")}
              </button>
              <button type="button" className="btn btn--danger" onClick={onStopWorkers}>
                {t("orch.stopConfirm", { n: workerCount })}
              </button>
            </div>
          </ToolbarMenu>
        ) : (
          <ToolbarMenu anchorRef={moreRef} label={t("toolbar.menu")} onClose={closeMenu}>
            {folded && terminals && (
              <>
                <button
                  type="button"
                  role="menuitem"
                  className="tmenu__item"
                  onClick={pick(onSplit)}
                  disabled={panes.length >= MAX_PANES}
                >
                  <Icon name="plus" size={14} />
                  <span className="tmenu__label">{t("toolbar.newTerminal")}</span>
                  {keys("pane.split") && <kbd className="tmenu__keys">{keys("pane.split")}</kbd>}
                </button>
                <button
                  type="button"
                  role="menuitem"
                  className="tmenu__item"
                  onClick={pick(onTidy)}
                  disabled={panes.length < 2}
                >
                  <Icon name="tidy" size={14} />
                  <span className="tmenu__label">{t("topbar.tidy")}</span>
                  {keys("pane.tidy") && <kbd className="tmenu__keys">{keys("pane.tidy")}</kbd>}
                </button>
                <button
                  type="button"
                  role="menuitem"
                  className="tmenu__item"
                  onClick={pick(onRestartAll)}
                >
                  <Icon name="restart" size={14} />
                  <span className="tmenu__label">{t("topbar.restartAll")}</span>
                  {keys("pane.restartAll") && (
                    <kbd className="tmenu__keys">{keys("pane.restartAll")}</kbd>
                  )}
                </button>
                <div className="tmenu__sep" role="separator" />
              </>
            )}
            {(canDetach || inOwnWindow) && (
              <>
                {canDetach && (
                  <button
                    type="button"
                    role="menuitem"
                    className="tmenu__item"
                    onClick={pick(onDetach)}
                    title={t("window.detachHint")}
                  >
                    <Icon name="window" size={14} />
                    <span className="tmenu__label">{t("toolbar.detach")}</span>
                  </button>
                )}
                {inOwnWindow && (
                  <button
                    type="button"
                    role="menuitem"
                    className="tmenu__item"
                    onClick={pick(onReattach)}
                    title={t("window.reattachHint")}
                  >
                    <Icon name="windowReturn" size={14} />
                    <span className="tmenu__label">{t("window.reattach")}</span>
                  </button>
                )}
                <div className="tmenu__sep" role="separator" />
              </>
            )}
            <button
              type="button"
              role="menuitem"
              className="tmenu__item"
              onClick={pick(onOpenPalette)}
            >
              <Icon name="search" size={14} />
              <span className="tmenu__label">{t("toolbar.palette")}</span>
              {paletteKeys && <kbd className="tmenu__keys">{paletteKeys}</kbd>}
            </button>
            {orchestrated && (
              <>
                <div className="tmenu__sep" role="separator" />
                <button
                  type="button"
                  role="menuitem"
                  className="tmenu__item"
                  onClick={pick(onOpenOrchestration)}
                >
                  <Icon name="orchestrator" size={14} />
                  <span className="tmenu__label">{t("orch.settingsMenu")}</span>
                  {keys("workspace.orchestration") && (
                    <kbd className="tmenu__keys">{keys("workspace.orchestration")}</kbd>
                  )}
                </button>
                {workerCount > 0 && (
                  <button
                    type="button"
                    role="menuitem"
                    className="tmenu__item tmenu__item--danger"
                    onClick={onArmStopWorkers}
                  >
                    <Icon name="stop" size={14} />
                    <span className="tmenu__label">{t("orch.stopMenu")}</span>
                    {keys("workspace.stopWorkers") && (
                      <kbd className="tmenu__keys">{keys("workspace.stopWorkers")}</kbd>
                    )}
                  </button>
                )}
              </>
            )}
            {/* Last, on its own, where the menu's first focus and a stray
                Enter never land: it throws away every conversation. */}
            <div className="tmenu__sep" role="separator" />
            <button
              type="button"
              role="menuitem"
              className="tmenu__item tmenu__item--danger"
              onClick={onArmReset}
              disabled={!agentPaneCount}
              title={agentPaneCount ? t("topbar.resetAllHint") : t("topbar.resetAllNone")}
            >
              <Icon name="reset" size={14} />
              <span className="tmenu__label">{t("toolbar.resetAll")}</span>
              {resetKeys && <kbd className="tmenu__keys">{resetKeys}</kbd>}
            </button>
          </ToolbarMenu>
        ))}
    </header>
  );
}
