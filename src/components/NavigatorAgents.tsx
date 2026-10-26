import { useLayoutEffect } from "react";
import { useT, type Translator } from "../i18n";
import {
  usePaneActivity,
  useWorkspaceActivity,
  type AgentActivity,
} from "../lib/agentState";
import { AGENTS, type Pane } from "../types";
import { StateGlyph } from "./StateGlyph";

/** The word a row ends with: short, because the glyph already says most of it. */
function shortState(t: Translator, activity: AgentActivity): string {
  switch (activity) {
    case "working":
      return t("pane.state.working");
    case "waiting":
      return t("pane.state.waitingShort");
    case "yours":
      return t("sidebar.stateYours");
    default:
      return t("pane.state.idle");
  }
}

/** The same state, spelled out for the tooltip. */
function longState(t: Translator, activity: AgentActivity): string {
  return t(`pane.state.${activity}`);
}

interface AgentRowProps {
  pane: Pane;
  /** `pane:<id>`, the key the keyboard walks the tree by. */
  rowKey: string;
  level: number;
  /** The focused pane of the workspace on screen. */
  selected: boolean;
  tabbable: boolean;
  onActivate: () => void;
}

/**
 * One agent under its workspace: its state, its name, what it runs, and the
 * state again in a word. Clicking it brings the pane forward, switching
 * workspace if it has to. A workspace in a window of its own still reports
 * here: that window relays its panes' states to this one (App).
 */
export function NavigatorAgentRow({
  pane,
  rowKey,
  level,
  selected,
  tabbable,
  onActivate,
}: AgentRowProps) {
  const t = useT();
  // A plain shell, or an agent that never said anything: nothing to report.
  const activity = usePaneActivity(pane.id)?.activity ?? "idle";
  const agent = AGENTS.find((a) => a.id === pane.agent)?.label ?? pane.agent;
  return (
    <div
      className={`nav-agent ${selected ? "nav-agent--selected" : ""}`}
      role="treeitem"
      aria-level={level}
      aria-current={selected ? "true" : undefined}
      tabIndex={tabbable ? 0 : -1}
      data-key={rowKey}
      title={`${pane.name} · ${agent} — ${longState(t, activity)}`}
      onClick={onActivate}
    >
      <StateGlyph state={activity} />
      <span className="nav-agent__name">{pane.name}</span>
      <span className="nav-agent__kind">{agent}</span>
      <span className={`nav-agent__state nav-state--${activity}`}>{shortState(t, activity)}</span>
    </div>
  );
}

/**
 * What a workspace's agents are up to, in one line under its path, readable
 * from any other workspace: their terminals keep running out of sight. Only
 * the state that matters most is written — an answer you owe first, then a
 * turn handed back, then work in progress — and the tooltip has them all.
 */
export function NavigatorActivityLine({ workspaceId, id }: { workspaceId: string; id: string }) {
  const { working, waiting, yours } = useWorkspaceActivity(workspaceId);
  const t = useT();
  if (!working && !waiting && !yours) return null;
  const parts = [
    waiting && t("sidebar.activityWaiting", { n: waiting }),
    yours && t("sidebar.activityYours", { n: yours }),
    working && t("sidebar.activityWorking", { n: working }),
  ].filter((part): part is string => Boolean(part));
  const state: AgentActivity = waiting ? "waiting" : yours ? "yours" : "working";
  return (
    <span
      id={id}
      className={`nav-ws__activity nav-state--${state}`}
      title={parts.join(" · ")}
      role="status"
    >
      <StateGlyph state={state} size={6} />
      <span className="nav-ws__activity-text">{parts[0]}</span>
    </span>
  );
}

/**
 * Renders nothing; tells the navigator when one pane's state changes. Only
 * mounted while the list is narrowed to the agents that want you, which is
 * the one view that depends on every pane's state at once.
 */
export function NavigatorPaneWatch({
  paneId,
  onChange,
}: {
  paneId: string;
  onChange: () => void;
}) {
  const activity = usePaneActivity(paneId)?.activity;
  // Before paint, so the list never shows a frame of the old answer.
  useLayoutEffect(onChange, [activity, onChange]);
  return null;
}
