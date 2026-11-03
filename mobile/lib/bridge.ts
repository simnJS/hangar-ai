/**
 * A plan window's usage, as the hangar-bridge mod reports it from inside a
 * pane's Claude Code (crates/hangar-core/src/bridge.rs and
 * crates/hangar-core/claude-mod/hangar-bridge), and as remote_stats passes it
 * on to the phone.
 *
 * The desktop's src/lib/bridge.ts also held the bridge's events and the
 * window's listeners for them (tag v1.12.1); the phone only reads this shape.
 */
export interface RateLimit {
  /** `five_hour`, `seven_day`, or a gateway's own window. */
  kind: string;
  percentUsed: number;
  resetsAt: string | null;
}
