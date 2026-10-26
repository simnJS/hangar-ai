import { usePaneActivity } from "../lib/agentState";
import { useT } from "../i18n";
import { StateGlyph, type GlyphState } from "./StateGlyph";

/** The pane a card's assignee resolved to (lib/agentPane), as a card names it. */
export interface AssigneePane {
  id: string;
  name: string;
  /** The agent running there, "Claude Code"; dropped when the card is narrow. */
  agent: string;
}

interface Props {
  /** The name the agent signed the board with. */
  assignee: string;
  pane: AssigneePane | null;
  /** Finished work says nothing about what its agent is doing now. */
  done: boolean;
  /** Brings the pane forward; null when no pane is clearly theirs. */
  onFocus: (() => void) | null;
}

/**
 * Who holds a task, with the live state of their pane in front of the name:
 * a card in progress whose agent is waiting for an answer reads as such
 * without leaving the board. An assignee no pane answers to is drawn idle.
 */
export function BoardAssignee({ assignee, pane, done, onFocus }: Props) {
  const t = useT();
  const activity = usePaneActivity(pane?.id ?? null);
  const state: GlyphState = done || !activity ? "idle" : activity.activity;
  const name = pane?.name ?? assignee;

  const body = (
    <>
      <StateGlyph state={state} size={7} label={t(`pane.state.${state}`)} />
      <span className="card__who-name">{name}</span>
      {pane && <span className="card__who-agent">· {pane.agent}</span>}
    </>
  );

  if (!onFocus) {
    return (
      <span className="card__who" title={t("board.assigneeTitle", { name: assignee })}>
        {body}
      </span>
    );
  }

  return (
    <button
      type="button"
      className="card__who card__who--link"
      title={t("board.focusAgent", { name })}
      onClick={(event) => {
        // The card around it opens the task; this opens the pane instead.
        event.stopPropagation();
        onFocus();
      }}
      onKeyDown={(event) => event.stopPropagation()}
    >
      {body}
    </button>
  );
}
