import { memo, useCallback, useMemo, useRef } from "react";
import { Icon } from "./Icon";
import { FilesSection } from "./InspectorFiles";
import {
  ActivitySection,
  ContextSection,
  QueueSection,
  SessionSection,
  TaskSection,
  TerminalSection,
} from "./InspectorPanels";
import { StateGlyph } from "./StateGlyph";
import { useT } from "../i18n";
import { useStore } from "../store";
import { usePaneActivity, usePlanLimits, type AgentActivity } from "../lib/agentState";
import { prettyModel } from "../lib/inspectorFormat";
import { usePaneInsight } from "../lib/paneInsight";
import { taskForPane, useBoardTasks } from "../lib/paneTask";
import { resolveShell } from "../lib/shells";
import type { CommandId } from "../lib/shortcuts";
import { clampInspector, INSPECTOR_MAX, INSPECTOR_MIN } from "../lib/sidePanels";
import { getTerminal } from "../lib/terminalRegistry";
import { useDrawer } from "../lib/useDrawer";
import { useShortcutLabel, useShortcutTitle } from "../lib/useShortcuts";
import { AGENTS, DEFAULT_SETTINGS, type Pane, type ShellInfo, type Workspace } from "../types";

/**
 * The column on the right: everything about the focused pane — its session,
 * context and cost, the board task it is on, the prompts waiting for it, the
 * files it touched and what it did lately. Reads only: lib/paneInsight for
 * what the pane reported, lib/paneTask for the board.
 */
export interface InspectorProps {
  workspace: Workspace;
  /** Every workspace, to tell which board assignee is which pane. */
  workspaces: Workspace[];
  activeWorkspaceId: string | null;
  /** null when the workspace has no pane in focus. */
  pane: Pane | null;
  /** 1-based position in reading order, for the "focus pane N" key. */
  paneIndex: number | null;
  /** The shells the machine reported, to name a shell pane's. */
  shells?: ShellInfo[];
  /** In interface points, as saved. */
  width: number;
  /** The widest the room allows right now, in points (lib/sidePanels). */
  maxWidth: number;
  /**
   * No room beside the terminals: drawn over them, opened on demand and gone
   * again as soon as the user is done with it (lib/useDrawer).
   */
  drawer: boolean;
  onResize: (width: number) => void;
  /** Section ids folded away (Settings.inspectorCollapsed). */
  collapsed: string[];
  onToggleSection: (id: string) => void;
  onClose: () => void;
  onOpenBoard: () => void;
  onOpenSessions: (paneId: string) => void;
  /** Opens the choice of MCP servers for the workspace. */
  onOpenMcpServers?: () => void;
}

/**
 * Widths are in interface points: the column is drawn at `width × ui-scale`,
 * so a larger interface size keeps the same amount of text per line. Whether
 * it docks or opens as a drawer is App's call, from lib/sidePanels.
 */
const KEY_STEP = 16;

/** A function that keeps its identity across renders but always calls the latest. */
function useLatest<A extends unknown[]>(fn: (...args: A) => void): (...args: A) => void {
  const ref = useRef(fn);
  ref.current = fn;
  return useCallback((...args: A) => ref.current(...args), []);
}

function readScale(): number {
  const raw = getComputedStyle(document.documentElement).getPropertyValue("--ui-scale");
  return Number.parseFloat(raw) || 1;
}

export function Inspector(props: InspectorProps) {
  const { pane, width, maxWidth, drawer, onResize, onClose } = props;
  const t = useT();
  const slotRef = useRef<HTMLDivElement>(null);
  const panelRef = useRef<HTMLElement>(null);
  /** A drawer in a window narrower than the saved width still leaves a margin. */
  const shown = Math.min(clampInspector(width), maxWidth);

  function beginResize(event: React.PointerEvent<HTMLDivElement>) {
    if (event.button !== 0) return;
    const slot = slotRef.current;
    const panel = panelRef.current;
    if (!slot || !panel) return;
    event.preventDefault();

    const grip = event.currentTarget;
    const pointerId = event.pointerId;
    const scale = readScale();
    const max = Math.min(INSPECTOR_MAX, maxWidth);
    const startX = event.clientX;
    const startWidth = panel.getBoundingClientRect().width;
    let latest = shown;
    grip.setPointerCapture(pointerId);
    grip.classList.add("is-active");
    document.body.classList.add("is-resizing");

    // Painted straight onto the element while dragging; the settings hear
    // about it once, on release, instead of re-rendering on every move.
    const move = (ev: PointerEvent) => {
      if (ev.pointerId !== pointerId) return;
      const points = (startWidth + (startX - ev.clientX)) / scale;
      latest = Math.round(Math.max(INSPECTOR_MIN, Math.min(max, points)));
      slot.style.setProperty("--inspector-w", `${latest}px`);
    };

    const end = (ev?: PointerEvent) => {
      if (ev && ev.pointerId !== pointerId) return;
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", end);
      window.removeEventListener("pointercancel", end);
      window.removeEventListener("blur", lostFocus);
      if (grip.hasPointerCapture(pointerId)) grip.releasePointerCapture(pointerId);
      grip.classList.remove("is-active");
      document.body.classList.remove("is-resizing");
      if (latest !== shown) onResize(latest);
    };
    const lostFocus = () => end();

    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", end);
    window.addEventListener("pointercancel", end);
    window.addEventListener("blur", lostFocus);
  }

  function resizeBy(delta: number) {
    const max = Math.min(INSPECTOR_MAX, maxWidth);
    onResize(Math.round(Math.max(INSPECTOR_MIN, Math.min(max, shown + delta))));
  }

  const closeDrawer = useDrawer({
    panelRef,
    active: drawer,
    onClose: () => onClose(),
    paneId: pane?.id ?? null,
  });

  const close = useLatest(() => {
    // A drawer gives the keyboard back to whatever had it when it opened.
    if (drawer) return closeDrawer("button");
    onClose();
    // The keyboard goes back where it was working: the focused pane.
    getTerminal(pane?.id ?? null)?.focus();
  });

  return (
    <div
      ref={slotRef}
      className={`inspector ${drawer ? "inspector--drawer" : ""}`}
      style={{ "--inspector-w": `${shown}px` } as React.CSSProperties}
    >
      <aside
        ref={panelRef}
        className="inspector__panel"
        aria-label={t("inspector.label")}
        tabIndex={-1}
      >
        <div
          className="inspector__grip"
          role="separator"
          aria-orientation="vertical"
          aria-label={t("inspector.resize")}
          aria-valuenow={shown}
          aria-valuemin={INSPECTOR_MIN}
          aria-valuemax={Math.max(INSPECTOR_MIN, Math.min(INSPECTOR_MAX, maxWidth))}
          title={t("inspector.resize")}
          tabIndex={0}
          onPointerDown={beginResize}
          onDoubleClick={() => onResize(DEFAULT_SETTINGS.inspectorWidth)}
          onKeyDown={(event) => {
            // The column grows leftwards, into the work area.
            if (event.key === "ArrowLeft") resizeBy(KEY_STEP);
            else if (event.key === "ArrowRight") resizeBy(-KEY_STEP);
            else return;
            event.preventDefault();
          }}
        />
        {pane ? (
          <PaneInspector {...props} pane={pane} onClose={close} />
        ) : (
          <div className="insp-none">
            <button
              type="button"
              className="insp-icon-btn insp-none__close"
              aria-label={t("inspector.close")}
              title={t("inspector.close")}
              onClick={close}
            >
              <Icon name="close" size={12} />
            </button>
            <span className="insp-none__mark" aria-hidden="true" />
            <p className="insp-none__title">{t("inspector.noPane")}</p>
            <p className="insp-none__body">{t("inspector.noPaneBody")}</p>
          </div>
        )}
      </aside>
    </div>
  );
}

/** Everything about one pane: the header, then the sections. */
function PaneInspector({
  workspace,
  workspaces,
  activeWorkspaceId,
  pane,
  paneIndex,
  shells,
  collapsed,
  onToggleSection,
  onClose,
  onOpenBoard,
  onOpenSessions,
  onOpenMcpServers,
}: InspectorProps & { pane: Pane }) {
  const t = useT();
  const { state } = useStore();
  const insight = usePaneInsight(pane.id);
  const activity = usePaneActivity(pane.id);
  const limits = usePlanLimits();
  const tasks = useBoardTasks(workspace.cwd);

  // App passes fresh closures on every render; these keep the memoised
  // sections from re-rendering for that alone.
  const toggle = useLatest(onToggleSection);
  const openBoard = useLatest(onOpenBoard);
  const openSessions = useLatest(onOpenSessions);
  const openMcpServers = useLatest(() => onOpenMcpServers?.());

  const task = useMemo(
    () => taskForPane(tasks, pane.id, workspaces, activeWorkspaceId),
    [tasks, pane.id, workspaces, activeWorkspaceId],
  );
  const folded = useMemo(() => new Set(collapsed), [collapsed]);

  const agent = AGENTS.find((entry) => entry.id === pane.agent);
  const agentLabel = agent?.label ?? pane.agent;
  const isShell = pane.agent === "shell";
  const folder = pane.cwd ?? workspace.cwd;
  const shell = useMemo(
    () =>
      resolveShell(shells ?? [], pane.shellId, workspace.shellId, state.settings.shellId)
        ?.label ?? null,
    [shells, pane.shellId, workspace.shellId, state.settings.shellId],
  );
  const bridged = activity?.bridged ?? false;

  const activityEmpty =
    pane.agent === "claude"
      ? t(bridged ? "inspector.activityQuiet" : "inspector.activityEmpty")
      : t("inspector.activityOther", { agent: agentLabel });

  return (
    <>
      <InspectorHeader
        name={pane.name}
        state={activity?.activity ?? "idle"}
        agentLabel={agentLabel}
        isShell={isShell}
        version={insight.agentVersion}
        model={prettyModel(insight.model)}
        shell={shell}
        paneIndex={paneIndex}
        onClose={onClose}
      />
      <div className="inspector__scroll">
        {isShell ? (
          <TerminalSection
            shell={shell}
            folder={folder}
            collapsed={folded.has("terminal")}
            onToggle={toggle}
          />
        ) : (
          <SessionSection
            paneId={pane.id}
            sessionId={pane.sessionId}
            resumable={agent?.resumable ?? false}
            since={insight.sessionSince}
            model={insight.model}
            folder={folder}
            onOpenSessions={openSessions}
            mcp={pane.agent === "claude" ? insight.mcp : null}
            onOpenMcpServers={onOpenMcpServers ? openMcpServers : undefined}
            collapsed={folded.has("session")}
            onToggle={toggle}
          />
        )}
        {!isShell && (
          <ContextSection
            agentLabel={agentLabel}
            showPlan={pane.agent === "claude"}
            gauge={insight.context}
            costUsd={insight.costUsd}
            since={insight.sessionSince}
            limits={limits?.limits ?? null}
            collapsed={folded.has("context")}
            onToggle={toggle}
          />
        )}
        <TaskSection
          task={task}
          onOpenBoard={openBoard}
          collapsed={folded.has("task")}
          onToggle={toggle}
        />
        {/* Prompts are queued through the hangar-bridge mod, which only
            Claude Code loads; another agent only shows a queue it has. */}
        {(pane.agent === "claude" || insight.queue.length > 0) && (
          <QueueSection
            paneId={pane.id}
            queue={insight.queue}
            collapsed={folded.has("queue")}
            onToggle={toggle}
          />
        )}
        <FilesSection
          workspaceId={workspace.id}
          paneId={pane.id}
          folder={folder}
          edited={insight.editedFiles}
          turnStartedAt={insight.turnStartedAt}
          collapsed={folded.has("files")}
          onToggle={toggle}
        />
        {/* A shell has no one to report on it; it gets the section only once
            something did. */}
        {(!isShell || insight.events.length > 0) && (
          <ActivitySection
            events={insight.events}
            empty={activityEmpty}
            collapsed={folded.has("activity")}
            onToggle={toggle}
          />
        )}
      </div>
    </>
  );
}

const InspectorHeader = memo(function InspectorHeader({
  name,
  state,
  agentLabel,
  isShell,
  version,
  model,
  shell,
  paneIndex,
  onClose,
}: {
  name: string;
  state: AgentActivity;
  agentLabel: string;
  isShell: boolean;
  version: string | null;
  model: string | null;
  /** A shell pane's shell, named in place of an agent's version and model. */
  shell: string | null;
  paneIndex: number | null;
  onClose: () => void;
}) {
  const t = useT();
  const keys = useShortcutLabel();
  const withKeys = useShortcutTitle();
  const focusCommand =
    paneIndex !== null && paneIndex >= 1 && paneIndex <= 9
      ? (`pane.focus${paneIndex}` as CommandId)
      : null;
  const focusKeys = focusCommand ? keys(focusCommand) : "";
  const stateWord = t(`pane.state.${state}`);
  const monogram = (Array.from(name.trim())[0] ?? "?").toUpperCase();

  return (
    <header className="insp-head">
      <span className="insp-head__avatar" aria-hidden="true">
        {monogram}
        <span className="insp-head__badge">
          <StateGlyph state={state} size={8} />
        </span>
      </span>
      <h2 className="insp-head__title">
        <span className="insp-head__name">{name}</span>
        <span className={`insp-head__state insp-state--${state}`}>{stateWord}</span>
      </h2>
      <div className="insp-head__tools">
        {focusKeys && focusCommand && (
          <kbd
            className="insp-head__kbd"
            title={withKeys(t("cmd.pane.focusN", { n: paneIndex ?? 0 }), focusCommand)}
          >
            {focusKeys}
          </kbd>
        )}
        <button
          type="button"
          className="insp-icon-btn"
          aria-label={t("inspector.close")}
          title={withKeys(t("inspector.close"), "view.inspector")}
          onClick={onClose}
        >
          <Icon name="close" size={12} />
        </button>
      </div>
      <p className="insp-head__sub">
        {isShell ? (
          <>
            {agentLabel}
            {shell && <> · {shell}</>}
          </>
        ) : (
          <>
            {agentLabel}
            {version && <span className="insp-head__version"> {version}</span>}
            {model && <> · {model}</>}
          </>
        )}
      </p>
    </header>
  );
});
