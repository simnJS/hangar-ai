import { Icon } from "./Icon";
import { useT } from "../i18n";
import { MAX_PANES } from "../lib/layout";
import { useShortcutTitle } from "../lib/useShortcuts";
import type { LayoutSize, Pane, Workspace } from "../types";

export type View = "terminals" | "board" | "memory";

export const LAYOUTS: LayoutSize[] = [1, 2, 4, 8];

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

  return (
    <header className="topbar">
      {hasNavigator && (
        <button
          className={`layouts__btn ${navigatorOpen ? "is-active" : ""}`}
          onClick={onToggleNavigator}
          aria-pressed={navigatorOpen}
          title={withKeys(t("cmd.view.navigator"), "view.navigator")}
          aria-label={t("cmd.view.navigator")}
        >
          ◧
        </button>
      )}

      <div className="topbar__id">
        <h1 className="topbar__name">{workspace.name}</h1>
        <span className="topbar__path" title={workspace.cwd}>
          {workspace.cwd}
        </span>
      </div>

      <div className="layouts" role="group" aria-label={t("view.group")}>
        {(["terminals", "board", "memory"] as const).map((id) => (
          <button
            key={id}
            className={`layouts__btn layouts__btn--wide ${view === id ? "is-active" : ""}`}
            onClick={() => onView(id)}
          >
            {t(`view.${id}`)}
          </button>
        ))}
      </div>

      <div className="layouts">
        <button
          className={`layouts__btn layouts__btn--wide ${dockShown ? "is-active" : ""}`}
          onClick={onToggleDock}
          aria-pressed={dockShown}
          title={withKeys(t("board.dockHint"), "view.boardDock")}
        >
          ◧ {t("board.dock")}
        </button>
        {canDetach && (
          <button
            className="layouts__btn layouts__btn--wide"
            onClick={onDetach}
            title={t("window.detachHint")}
          >
            ⧉ {t("window.detach")}
          </button>
        )}
        {inOwnWindow && (
          <button
            className="layouts__btn layouts__btn--wide"
            onClick={onReattach}
            title={t("window.reattachHint")}
          >
            ⤓ {t("window.reattach")}
          </button>
        )}
      </div>

      {view === "terminals" && (
        <>
          <div className="layouts" role="group" aria-label={t("topbar.layout")}>
            {LAYOUTS.map((size) => (
              <button
                key={size}
                className={`layouts__btn ${panes.length === size ? "is-active" : ""}`}
                onClick={() => onPreset(size)}
                title={t("topbar.presetHint", { n: size })}
              >
                {size}
              </button>
            ))}
            {/* For the counts no preset matches, and for a grid that splits
                and moves have pulled out of shape. */}
            <button
              className="layouts__btn layouts__btn--wide"
              onClick={onTidy}
              disabled={panes.length < 2}
              title={withKeys(t("topbar.tidyHint"), "pane.tidy")}
            >
              ⊞ {t("topbar.tidy")}
            </button>
          </div>

          <button
            className="btn btn--ghost"
            onClick={onSplit}
            disabled={panes.length >= MAX_PANES}
            title={withKeys(t("topbar.addPaneHint"), "pane.split")}
          >
            <Icon name="plus" size={13} />
            {t("topbar.addPane")}
          </button>

          <button
            className="btn btn--ghost"
            onClick={onRestartAll}
            title={withKeys(t("topbar.restartAllHint"), "pane.restartAll")}
          >
            <Icon name="restart" size={13} />
            {t("topbar.restartAll")}
          </button>

          {confirmReset ? (
            <>
              <button className="btn btn--ghost" onClick={onCancelReset}>
                {t("create.cancel")}
              </button>
              <button
                className="btn btn--danger"
                onClick={onReset}
                onKeyDown={(event) => event.key === "Escape" && onCancelReset()}
                // Armed from the keyboard as often as from a click, so the
                // answer is one Enter away either way.
                autoFocus
              >
                {t("topbar.resetAllConfirm", { n: agentPaneCount })}
              </button>
            </>
          ) : (
            <button
              className="btn btn--ghost"
              onClick={onArmReset}
              disabled={!agentPaneCount}
              title={
                agentPaneCount
                  ? withKeys(t("topbar.resetAllHint"), "pane.resetAll")
                  : t("topbar.resetAllNone")
              }
            >
              <Icon name="reset" size={13} />
              {t("topbar.resetAll")}
            </button>
          )}
        </>
      )}

      <button
        className="layouts__btn"
        onClick={onOpenPalette}
        title={withKeys(t("cmd.view.palette"), "view.palette")}
        aria-label={t("cmd.view.palette")}
      >
        ⌘
      </button>
      <button
        className={`layouts__btn ${inspectorOpen ? "is-active" : ""}`}
        onClick={onToggleInspector}
        aria-pressed={inspectorOpen}
        title={withKeys(t("cmd.view.inspector"), "view.inspector")}
        aria-label={t("cmd.view.inspector")}
      >
        ◨
      </button>
    </header>
  );
}
