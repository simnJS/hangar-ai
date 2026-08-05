/**
 * Turns a raw PTY stream into "this pane handed control back" events.
 *
 * No CLI agent exposes an "I am done" signal, so this watches the shape of the
 * output instead. Agents write continuously while they work — spinners, elapsed
 * timers, streamed tokens, tool output — and then go completely silent once the
 * prompt is yours again. A silence that follows a long enough burst is
 * therefore a hand-back.
 *
 * The burst threshold is what keeps the noise out: a shell prompt, an echoed
 * keystroke or a `git status` produce a few hundred milliseconds of output and
 * are ignored, while a real turn produces seconds of it.
 */

/** Shortest burst that counts as real work rather than terminal noise. */
const MIN_BUSY_MS = 2500;

/** How often the silence is checked. Cheap: only runs during a burst. */
const TICK_MS = 250;

/**
 * Two hand-backs closer than this are the same one seen twice — typically a
 * bell immediately followed by the prompt going quiet.
 */
const DEBOUNCE_MS = 1500;

export interface ActivityOptions {
  /** Read on each tick, so changing the setting applies without a restart. */
  idleMs: () => number;
  onSettle: () => void;
}

export interface ActivityWatcher {
  /** Call for every chunk of output; cost is constant per chunk. */
  push: () => void;
  /** A bell is an explicit hand-back: it settles now, without waiting. */
  ring: () => void;
  dispose: () => void;
}

export function createActivityWatcher({
  idleMs,
  onSettle,
}: ActivityOptions): ActivityWatcher {
  let ticker: number | null = null;
  let burstStartedAt = 0;
  let lastOutputAt = 0;
  let lastSettleAt = 0;

  function stop() {
    if (ticker !== null) {
      window.clearInterval(ticker);
      ticker = null;
    }
  }

  function settle() {
    stop();
    const now = Date.now();
    if (now - lastSettleAt < DEBOUNCE_MS) return;
    lastSettleAt = now;
    burstStartedAt = 0;
    onSettle();
  }

  return {
    push() {
      const now = Date.now();
      if (ticker === null) {
        burstStartedAt = now;
        ticker = window.setInterval(() => {
          if (Date.now() - lastOutputAt < idleMs()) return;
          // The burst is measured up to its last byte, not to now, so the
          // silence itself never counts as working time.
          const worked = lastOutputAt - burstStartedAt >= MIN_BUSY_MS;
          if (worked) settle();
          else stop();
        }, TICK_MS);
      }
      lastOutputAt = now;
    },

    ring: settle,

    dispose: stop,
  };
}
