import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import { Icon, type IconName } from "./Icon";
import { ToolbarActivity } from "./ToolbarActivity";
import { ToolbarMenu, type MenuCloseReason } from "./ToolbarMenu";
import { useT } from "../i18n";
import { MAX_PANES } from "../lib/layout";
import { useShortcutLabel, useShortcutTitle } from "../lib/useShortcuts";
import type { LayoutSize, Pane, Workspace } from "../types";

export type View = "terminals" | "board" | "memory";

export const LAYOUTS: LayoutSize[] = [1, 2, 4, 8];

const VIEWS: { id: View; icon: IconName }[] = [
  { id: "terminals", icon: "terminal" },
  { id: "board", icon: "board" },
  { id: "memory", icon: "bookmark" },
];

const LAYOUT_ICONS: Record<LayoutSize, IconName> = {
  1: "layout1",
  2: "layout2",
  4: "layout4",
  8: "layout8",
};

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
}: ToolbarProps) {
  const t = useT();
  const withKeys = useShortcutTitle();
  const keys = useShortcutLabel();
  const terminals = view === "terminals";

  const moreRef = useRef<HTMLButtonElement>(null);
  /** Opened from its button. An armed reset opens it too, from anywhere. */
  const [menuOpen, setMenuOpen] = useState(false);
  const menuShown = menuOpen || confirmReset;
  /** The terminal actions did not fit in the bar and are listed in the menu. */
  const [folded, setFolded] = useState(false);
  const actionsRef = useRef<HTMLDivElement>(null);
  const confirmId = useId();

  // The reset went through, or was called off elsewhere (a view change): the
  // menu it was showing in has nothing left to say.
  const wasConfirming = useRef(confirmReset);
  useEffect(() => {
    if (wasConfirming.current && !confirmReset) setMenuOpen(false);
    wasConfirming.current = confirmReset;
  }, [confirmReset]);

  /**
   * Escape gives the keyboard back to the button that opened the menu. A reset
   * armed from a shortcut or the palette had no button: calling it off hands
   * the keyboard back to the terminal, which is what App's cancel does.
   */
  const closeMenu = (reason: MenuCloseReason) => {
    const fromButton = menuOpen;
    if (confirmReset) onCancelReset();
    setMenuOpen(false);
    if (reason === "escape" && fromButton) moreRef.current?.focus();
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
          <h1 className="toolbar__name" title={workspace.name}>
            {workspace.name}
          </h1>
          <span className="toolbar__path" title={workspace.cwd}>
            {/* Isolated left-to-right inside a right-to-left box, so the
                ellipsis eats the start of the path and nothing else moves. */}
            <span dir="ltr">{workspace.cwd}</span>
          </span>
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
        {terminals && (
          <>
            <div className="segmented segmented--icons" role="group" aria-label={t("topbar.layout")}>
              {LAYOUTS.map((size) => (
                <button
                  key={size}
                  type="button"
                  className="segmented__btn"
                  aria-pressed={panes.length === size}
                  onClick={() => onPreset(size)}
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

        <button
          type="button"
          className="tbtn tbtn--toggle"
          onClick={onToggleDock}
          aria-pressed={dockShown}
          title={withKeys(t("board.dockHint"), "view.boardDock")}
          aria-label={t("board.dock")}
        >
          <Icon name="boardPanel" size={16} />
        </button>
        <button
          type="button"
          className="tbtn tbtn--toggle"
          onClick={onToggleInspector}
          aria-pressed={inspectorOpen}
          title={withKeys(t("cmd.view.inspector"), "view.inspector")}
          aria-label={t("toolbar.inspector")}
        >
          <Icon name="sidebarRight" size={16} />
        </button>
      </div>

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
              <button type="button" className="btn btn--ghost" onClick={() => closeMenu("escape")}>
                {t("create.cancel")}
              </button>
              <button
                type="button"
                className="btn btn--danger"
                onClick={onReset}
                // Armed from the keyboard as often as from a click, so the
                // answer is one Enter away either way.
                autoFocus
              >
                {t("topbar.resetAllConfirm", { n: agentPaneCount })}
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
            {(canDetach || inOwnWindow) && (
              <>
                <div className="tmenu__sep" role="separator" />
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
              </>
            )}
            <div className="tmenu__sep" role="separator" />
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
          </ToolbarMenu>
        ))}
    </header>
  );
}
