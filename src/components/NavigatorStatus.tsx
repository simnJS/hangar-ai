import { useT, type Translator } from "../i18n";
import { getPaneActivity, useAgentsIn, type AgentActivity } from "../lib/agentState";
import { getTerminal } from "../lib/terminalRegistry";
import type { Pane, Workspace } from "../types";
import { readingOrder } from "./NavigatorModel";
import { StateGlyph } from "./StateGlyph";

/** The states a workspace's summary counts, the one that needs you first. */
export const BUSY = ["waiting", "yours", "working"] as const;
type Busy = (typeof BUSY)[number];

/** "Eve waits for your answer", "Eve and Max are working". */
const WHO = {
  waiting: "sidebar.whoWaiting",
  yours: "sidebar.whoYours",
  working: "sidebar.whoWorking",
} as const;

/** "Ava", "Ava and Ben", "Ava, Ben and Cleo". */
function listNames(t: Translator, names: string[]): string {
  if (names.length < 2) return names[0] ?? "";
  return t("sidebar.namesLast", {
    list: names.slice(0, -1).join(", "),
    last: names[names.length - 1],
  });
}

/** Who is in `state` among these panes, as a sentence. */
export const whoIs = (t: Translator, state: Busy, panes: Pane[]) =>
  t(WHO[state], { n: panes.length, names: listNames(t, panes.map((pane) => pane.name)) });

/**
 * The agents of these workspaces in `state` now, in the order they sit on
 * screen: workspace by workspace, each one's panes in reading order. Read
 * when asked, for a menu or a click, rather than kept up to date.
 */
export function agentsIn(workspaces: Workspace[], state: AgentActivity): Pane[] {
  return workspaces
    .flatMap(readingOrder)
    .filter((pane) => getPaneActivity(pane.id)?.activity === state);
}

/**
 * Brings an agent's pane forward and gives it the keyboard: when it already
 * was the pane in front, nothing changes, and the focus would stay where the
 * click or the menu left it.
 */
export function bringForward(
  onFocusPane: (workspaceId: string, paneId: string) => void,
  workspaceId: string,
  paneId: string,
) {
  onFocusPane(workspaceId, paneId);
  requestAnimationFrame(() => getTerminal(paneId)?.focus());
}

interface Props {
  /** One workspace for its row; every one filed in a folded folder for its heading. */
  workspaces: Workspace[];
  /** Ties the spoken summary to its row (aria-describedby). */
  id: string;
  activeId: string | null;
  /** The pane each workspace has in front, so that a second click moves on. */
  focusByWorkspace: Record<string, string>;
  /** Brings a pane forward, switching workspace if it has to. */
  onFocusPane: (workspaceId: string, paneId: string) => void;
}

/**
 * What the agents of a workspace are doing, in as little room as it takes:
 * how many wait for an answer, are done and wait for you, or are at work,
 * each with the glyph that state wears everywhere else. Nothing when they are
 * idle. Pointing at one says who; clicking it brings that agent's pane
 * forward, and the next click the next one, as the toolbar's counters do. The
 * keyboard reaches the same agents through the row's menu, and a screen
 * reader hears the summary with the row.
 */
export function NavigatorStatus({
  workspaces,
  id,
  activeId,
  focusByWorkspace,
  onFocusPane,
}: Props) {
  const t = useT();
  const roster = useAgentsIn(workspaces.map((ws) => ws.id));
  const order = workspaces.flatMap(readingOrder);
  const panesIn = (state: Busy) => order.filter((pane) => roster[state].includes(pane.id));

  const go = (panes: Pane[]) => {
    const current = activeId ? focusByWorkspace[activeId] : undefined;
    const target = panes[(panes.findIndex((pane) => pane.id === current) + 1) % panes.length];
    const ws = workspaces.find((entry) => entry.panes.some((pane) => pane.id === target.id));
    if (ws) bringForward(onFocusPane, ws.id, target.id);
  };

  const shown = BUSY.map((state) => [state, panesIn(state)] as const).filter(
    ([, panes]) => panes.length > 0,
  );

  return (
    <>
      {shown.length > 0 && (
        <span className="nav-status" aria-hidden="true">
          {shown.map(([state, panes]) => (
            <span
              key={state}
              className={`nav-status__item nav-status__item--${state} nav-ctl`}
              title={whoIs(t, state, panes)}
              onClick={(event) => {
                // Not the row's own click, which would only open the workspace.
                event.stopPropagation();
                go(panes);
              }}
            >
              <StateGlyph state={state} size={7} />
              <span className="nav-status__count">{panes.length}</span>
            </span>
          ))}
        </span>
      )}
      {/* Spoken with the row, and when it changes, as the line it replaces was. */}
      <span className="nav-sr" id={id} role="status">
        {shown.map(([state, panes]) => whoIs(t, state, panes)).join(", ")}
      </span>
    </>
  );
}
