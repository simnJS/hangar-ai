import { useId, type ReactNode } from "react";
import { Icon } from "./Icon";

/**
 * One foldable block of the inspector. The title row is a real button that
 * says whether it is open; whatever sits at its right end — a count, a total
 * — is part of the title, and an action ("Open", "View diff") sits beside the
 * button rather than in it, so each stays one control.
 */
export function InspectorSection({
  id,
  title,
  meta,
  action,
  collapsed,
  onToggle,
  children,
}: {
  id: string;
  title: string;
  /** Read with the title: a count or a total. */
  meta?: ReactNode;
  /** A link-like control at the far right of the title row. */
  action?: ReactNode;
  collapsed: boolean;
  onToggle: (id: string) => void;
  children: ReactNode;
}) {
  const bodyId = useId();
  return (
    <section className={`insp-section ${collapsed ? "insp-section--folded" : ""}`}>
      <div className="insp-section__head">
        <button
          type="button"
          className="insp-section__toggle"
          aria-expanded={!collapsed}
          aria-controls={collapsed ? undefined : bodyId}
          onClick={() => onToggle(id)}
        >
          <Icon name="chevronDown" size={10} />
          <span className="insp-section__title">{title}</span>
          {meta != null && meta !== false && <span className="insp-section__meta">{meta}</span>}
        </button>
        {action && !collapsed ? <div className="insp-section__action">{action}</div> : null}
      </div>
      {collapsed ? null : (
        <div className="insp-section__body" id={bodyId}>
          {children}
        </div>
      )}
    </section>
  );
}
