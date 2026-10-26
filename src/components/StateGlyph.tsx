import type { AgentActivity } from "../lib/agentState";

/**
 * An agent's state, drawn the same everywhere — navigator, pane header,
 * inspector, toolbar, palette. Colour and shape both carry it, so it reads
 * without telling green from amber: a pulsing dot while working, a diamond
 * while waiting for an answer, a solid dot when it is your turn, a ring when
 * idle. `starting` and `exited` are the shell's own states, before or after
 * there is an agent to report on.
 */
export type GlyphState = AgentActivity | "starting" | "exited";

export function StateGlyph({
  state,
  size = 8,
  label,
}: {
  state: GlyphState;
  /** Diameter in px at 100 % interface size. */
  size?: number;
  /** Spoken instead of nothing; omit where the state is also written out. */
  label?: string;
}) {
  return (
    <span
      className={`glyph glyph--${state}`}
      style={{ "--glyph-size": `${size}px` } as React.CSSProperties}
      role={label ? "img" : undefined}
      aria-label={label}
      aria-hidden={label ? undefined : true}
    />
  );
}
