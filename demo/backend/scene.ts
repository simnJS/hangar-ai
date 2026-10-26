/**
 * What the demo is in the middle of when it opens: agents at work in
 * workspaces off screen, so the navigator has something to report from the
 * first second, and a click on it lands on a pane that really is waiting.
 *
 * Each job goes out the way an agent hands one to another pane: pane_send
 * over the control channel, queued until that pane's Claude Code is free.
 * The app starts the workspace's panes in the background to deliver it
 * (lib/control), exactly as it does for a real agent.
 */
import * as control from "./control";
import { markAsleep } from "../../src/lib/hibernateStore";

const JOBS: { pane: string; text: string }[] = [
  // Stops to ask before running the suite: waits for you.
  { pane: "Eve", text: "Fix the coupon applied twice at checkout" },
  // Minutes of backfilling: still at work when you look.
  { pane: "Max", text: "Migrate the invoice amounts to integer cents" },
  // Done in a few seconds, with nobody looking: your turn.
  { pane: "Leo", text: "Tighten the copy of the pricing page" },
];

/** Once the app has loaded its state; the control listener comes with it. */
export function playOpeningScene() {
  // Left alone long enough to be put to sleep, as lib/hibernate would.
  markAsleep("ws-docs", { at: Date.now() - 42 * 60_000, agents: 1 });
  for (const job of JOBS) {
    control.ask("pane_send", { ...job, when_idle: true }).catch(() => undefined);
  }
}
