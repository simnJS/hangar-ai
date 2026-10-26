import { useMemo, type CSSProperties, type ReactNode } from "react";
import { StateGlyph } from "./StateGlyph";
import { useT } from "../i18n";
import { useStore } from "../store";
import {
  getPaneActivity,
  usePaneActivity,
  useWorkspaceActivity,
  type AgentActivity,
} from "../lib/agentState";
import { usePaneInsight } from "../lib/paneInsight";
import { taskForPane, useBoardTasks } from "../lib/paneTask";
import { leafIds, normalizeTree } from "../lib/layout";
import { getTerminal } from "../lib/terminalRegistry";
import type { Pane, Workspace } from "../types";
import type { View } from "./Toolbar";

/**
 * The capsule in the middle of the toolbar: what the focused pane is doing, in
 * one line, and how many agents of the workspace are waiting on you.
 *
 * Read-only on purpose — the pane is the one that knows, through
 * lib/agentState and lib/paneInsight — except for the two counters, which take
 * you to the pane they count.
 */

/** The state words, coloured like the glyph they sit next to. */
const STATE_WORD = {
  working: "pane.state.working",
  waiting: "pane.state.waiting",
  yours: "pane.state.yours",
  idle: "pane.state.idle",
} as const;

export function ToolbarActivity({
  workspace,
  panes,
  focusedPaneId,
  view,
  onView,
  onFocusPane,
}: {
  workspace: Workspace;
  panes: Pane[];
  focusedPaneId: string | null;
  view: View;
  onView: (view: View) => void;
  onFocusPane: (paneId: string) => void;
}) {
  const t = useT();
  const { state } = useStore();
  const pane = panes.find((entry) => entry.id === focusedPaneId) ?? null;
  const activity = usePaneActivity(pane?.id ?? null)?.activity ?? "idle";
  const insight = usePaneInsight(pane?.id ?? null);
  const tasks = useBoardTasks(workspace.cwd);
  const task = pane ? taskForPane(tasks, pane.id, state.workspaces, workspace.id) : null;
  const counts = useWorkspaceActivity(workspace.id);

  /** Reading order, so the counters step through the panes as they sit on screen. */
  const order = useMemo(
    () => leafIds(normalizeTree(panes, workspace.tree)).filter(Boolean),
    [panes, workspace.tree],
  );

  /**
   * The next pane in `wanted` after the focused one, wrapping: the first click
   * lands on the first such pane, the next ones walk through the rest.
   */
  const nextIn = (wanted: AgentActivity): Pane | null => {
    const ids = order.filter((id) => getPaneActivity(id)?.activity === wanted);
    if (!ids.length) return null;
    const id = ids[(ids.indexOf(focusedPaneId ?? "") + 1) % ids.length];
    return panes.find((entry) => entry.id === id) ?? null;
  };

  const go = (target: Pane) => {
    if (view !== "terminals") onView("terminals");
    onFocusPane(target.id);
    // The pane focuses itself when its focus changes; when it already was the
    // focused one, nothing changes, and the keyboard would stay on this button.
    requestAnimationFrame(() => getTerminal(target.id)?.focus());
  };

  const pct = insight.context ? Math.round(Math.min(100, Math.max(0, insight.context.pct))) : null;

  const word = (state: AgentActivity) => (
    <span className={`activity__state activity__state--${state}`}>{t(STATE_WORD[state])}</span>
  );

  let message: ReactNode;
  if (!pane) {
    message = (
      <span className="activity__detail">
        {counts.working
          ? t("sidebar.activityWorking", { n: counts.working })
          : t("toolbar.activity.quiet")}
      </span>
    );
  } else {
    // What the state word alone leaves out: the task it is on or just finished,
    // what it is asking, or, for want of anything else, the last thing it said.
    // Only the latest event can be the question: an older one was answered.
    const latest = insight.events[0];
    let detail: string | null = null;
    /** A task title reads in full strength; the rest is commentary. */
    let strong = false;
    if (activity === "waiting") {
      // The state word already says it waits for you; what is left to say is
      // what for — the tool, or the other pane's file.
      if (latest?.kind === "permission") {
        const { tool, conflict } = latest.params;
        detail = conflict
          ? `· ${t("toolbar.activity.conflict", { name: conflict })}`
          : tool
            ? `· ${tool}`
            : null;
      }
    } else if (task && activity === "working") {
      detail = t("toolbar.activity.on", { title: task.title });
      strong = true;
    } else if (task && activity === "yours") {
      detail = `· ${t("toolbar.activity.task", { title: task.title })}`;
      strong = true;
    } else if (insight.lastAnswer) {
      detail = `· ${t("toolbar.activity.lastAnswer", { answer: insight.lastAnswer })}`;
    }
    message = (
      <>
        {word(activity)}
        {detail && (
          <>
            {" "}
            <span className={`activity__detail ${strong ? "activity__detail--strong" : ""}`}>
              {detail}
            </span>
          </>
        )}
      </>
    );
  }

  const waitingNext = counts.waiting ? nextIn("waiting") : null;
  const yoursNext = counts.yours ? nextIn("yours") : null;

  const badge = (
    kind: "waiting" | "yours",
    count: number,
    target: Pane | null,
    glyph: ReactNode,
  ) => {
    if (!count || !target) return null;
    const summary = t(kind === "waiting" ? "sidebar.activityWaiting" : "sidebar.activityYours", {
      n: count,
    });
    const label = t("toolbar.activity.goTo", { summary, name: target.name });
    return (
      <button
        type="button"
        className={`activity__badge activity__badge--${kind}`}
        onClick={() => go(target)}
        title={label}
        aria-label={label}
      >
        {glyph}
        <span className="activity__count">{count}</span>
      </button>
    );
  };

  // Each counter wears its state's glyph, as everywhere else that state shows.
  const waitingBadge = badge(
    "waiting",
    counts.waiting,
    waitingNext,
    <StateGlyph state="waiting" size={7} />,
  );
  const yoursBadge = badge("yours", counts.yours, yoursNext, <StateGlyph state="yours" size={7} />);

  return (
    <div className="activity" role="group" aria-label={t("toolbar.activity.label")}>
      <span className="activity__line">
        {pane ? (
          <span className="activity__who">
            <StateGlyph state={activity} size={7} />
            <span className="activity__name">{pane.name}</span>
          </span>
        ) : (
          <span className="activity__who">
            <span className="activity__name">{workspace.name}</span>
          </span>
        )}
        <span className="activity__message">{message}</span>
      </span>

      {pct !== null && (
        <span
          className="activity__context"
          title={t("toolbar.activity.contextHint", { pct })}
        >
          {t("toolbar.activity.context", { pct })}
        </span>
      )}

      {(waitingBadge || yoursBadge) && (
        <span className="activity__badges">
          {waitingBadge}
          {yoursBadge}
        </span>
      )}

      {pct !== null && (
        <span
          className="activity__progress"
          style={{ "--activity-progress": pct / 100 } as CSSProperties}
          aria-hidden="true"
        />
      )}
    </div>
  );
}
