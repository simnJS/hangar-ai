import { priorityKey, priorityLevel } from "../lib/boardFormat";
import { useT } from "../i18n";

/**
 * A task's priority as three rising bars and a word — Low, Medium, High —
 * the way a signal is read: how many bars are lit is the urgency. The number
 * agents set stays in the tooltip.
 */
export function BoardPriority({ priority }: { priority: number }) {
  const t = useT();
  const level = priorityLevel(priority);
  const word = t(priorityKey(priority));
  return (
    <span
      className={`prio prio--${level}`}
      title={t("board.priorityTitle", { n: priority, level: word })}
    >
      <span className="prio__bars" aria-hidden>
        <i />
        <i />
        <i />
      </span>
      {word}
    </span>
  );
}
