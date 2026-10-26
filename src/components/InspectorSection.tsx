import { useId, type ReactNode } from "react";

/**
 * The few glyphs the inspector needs that the app's icon set has no use for
 * elsewhere, drawn the same way (24-unit grid, one stroke, currentColor) so
 * they sit beside components/Icon without a seam. Decoration only: the
 * control around each one carries the label.
 */
export function InspectorGlyph({
  name,
  size = 12,
}: {
  name: "chevron" | "external" | "comment";
  size?: number;
}) {
  return (
    <svg
      className="inspector-glyph"
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2.2}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
    >
      {name === "chevron" ? (
        <path d="m6 9 6 6 6-6" />
      ) : name === "external" ? (
        <>
          <path d="M7 17 17 7" />
          <path d="M8 7h9v9" />
        </>
      ) : (
        <path d="M21 12a8 8 0 0 1-11.6 7.1L4 20.5l1.4-4.9A8 8 0 1 1 21 12z" />
      )}
    </svg>
  );
}

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
          <InspectorGlyph name="chevron" size={10} />
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
