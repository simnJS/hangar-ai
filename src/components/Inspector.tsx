import { memo, useCallback, useLayoutEffect, useMemo, useRef, useState } from "react";
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
import { getTerminal } from "../lib/terminalRegistry";
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
  width: number;
  onResize: (width: number) => void;
  /** Section ids folded away (Settings.inspectorCollapsed). */
  collapsed: string[];
  onToggleSection: (id: string) => void;
  onClose: () => void;
  onOpenBoard: () => void;
  onOpenSessions: (paneId: string) => void;
}

/**
 * Widths are in interface points: the column is drawn at `width × ui-scale`,
 * so a larger interface size keeps the same amount of text per line.
 */
const MIN_WIDTH = 240;
const MAX_WIDTH = 440;
const KEY_STEP = 16;
/**
 * What the work area keeps beside a docked inspector. With less room than
 * that, the inspector stops taking any and slides over the terminals as a
 * drawer instead — about a 1180px window with the navigator open.
 */
const TERMINALS_MIN = 640;
/** The strip of work area a drawer, however wide, always leaves uncovered. */
const DRAWER_MARGIN = 48;

const clampWidth = (width: number) => Math.round(Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, width)));

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
  const { pane, width, onResize, onClose } = props;
  const t = useT();
  const slotRef = useRef<HTMLDivElement>(null);
  const panelRef = useRef<HTMLElement>(null);
  const [drawer, setDrawer] = useState(false);
  const shown = clampWidth(width);

  // Docked or drawer, from the room the work area has left — measured, since
  // it depends on the window, the navigator and the interface size at once.
  // The panel's own width is the same in both modes, so the switch can never
  // feed back into the measurement.
  useLayoutEffect(() => {
    const slot = slotRef.current;
    const panel = panelRef.current;
    const area = slot?.parentElement;
    if (!slot || !panel || !area) return;
    let first = true;
    const measure = () => {
      const room = area.getBoundingClientRect().width;
      const own = panel.getBoundingClientRect().width;
      const next = room - own < TERMINALS_MIN;
      setDrawer(next);
      // Opened as a drawer, it takes the keyboard: Escape then closes it
      // and hands the keys back to the pane.
      if (first && next) panel.focus({ preventScroll: true });
      first = false;
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(area);
    observer.observe(panel);
    return () => observer.disconnect();
  }, []);

  /** The widest the column may get, in points, with the room there is now. */
  const maxFor = useCallback(
    (scale: number) => {
      const room = slotRef.current?.parentElement?.getBoundingClientRect().width ?? Infinity;
      return Math.max(MIN_WIDTH, (room - (drawer ? DRAWER_MARGIN : TERMINALS_MIN)) / scale);
    },
    [drawer],
  );

  function beginResize(event: React.PointerEvent<HTMLDivElement>) {
    if (event.button !== 0) return;
    const slot = slotRef.current;
    const panel = panelRef.current;
    if (!slot || !panel) return;
    event.preventDefault();

    const grip = event.currentTarget;
    const pointerId = event.pointerId;
    const scale = readScale();
    const max = Math.min(MAX_WIDTH, maxFor(scale));
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
      latest = Math.round(Math.max(MIN_WIDTH, Math.min(max, points)));
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
    const max = Math.min(MAX_WIDTH, maxFor(readScale()));
    onResize(Math.round(Math.max(MIN_WIDTH, Math.min(max, shown + delta))));
  }

  const close = useLatest(() => {
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
        onKeyDown={(event) => {
          if (!drawer || event.key !== "Escape" || event.defaultPrevented) return;
          event.preventDefault();
          event.stopPropagation();
          close();
        }}
      >
        <div
          className="inspector__grip"
          role="separator"
          aria-orientation="vertical"
          aria-label={t("inspector.resize")}
          aria-valuenow={shown}
          aria-valuemin={MIN_WIDTH}
          aria-valuemax={MAX_WIDTH}
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
