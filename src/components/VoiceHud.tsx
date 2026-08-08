import { useEffect, useRef, useState } from "react";
import { Logo } from "./Logo";
import { useT } from "../i18n";
import type { VoicePhase } from "../lib/voice";

/**
 * What dictation looks like while it is happening.
 *
 * A pill rather than a sentence. Push-to-talk lasts a few seconds and the
 * finger is already on the key, so instructions are not what is wanted on
 * screen — the one question is whether it can hear you, and a row of bars
 * moving with your voice answers it faster than any wording could. The text is
 * still there for screen readers, and as the tooltip.
 */

/** Bars in the meter. Enough to read as a waveform, few enough to stay a pill. */
const BARS = 9;

/**
 * How often the meter scrolls. Deliberately not driven by the level changing:
 * a steady voice reports the same peak twice in a row and React would skip the
 * render, freezing the trace exactly while someone is holding a note.
 */
const TICK_MS = 70;

export function VoiceHud({ phase, level }: { phase: VoicePhase; level: number }) {
  const [bars, setBars] = useState<number[]>(() => Array(BARS).fill(0));
  const t = useT();

  // Read by the timer, which is installed once per recording and must not be
  // torn down and rebuilt on every level that arrives.
  const levelRef = useRef(level);
  levelRef.current = level;

  useEffect(() => {
    if (phase !== "recording") {
      setBars(Array(BARS).fill(0));
      return;
    }
    const timer = window.setInterval(() => {
      // Newest on the right, so the trace reads the way a waveform is written.
      setBars((current) => [...current.slice(1), levelRef.current]);
    }, TICK_MS);
    return () => window.clearInterval(timer);
  }, [phase]);

  const label = phase === "transcribing" ? t("voice.transcribing") : t("voice.recording");

  return (
    <div className="voice-pill" role="status" aria-label={label} title={label}>
      <Logo size={15} />
      <span className="voice-pill__sep" aria-hidden="true" />
      <span
        className={`voice-pill__bars ${phase === "transcribing" ? "is-working" : ""}`}
        aria-hidden="true"
      >
        {bars.map((value, index) => (
          <i
            key={index}
            // Square-rooted: a peak meter on a linear scale spends its life on
            // the floor. The floor is a quarter of the height rather than a
            // sliver, because a 2px bar on a 3px width is a dot — and a row of
            // dots reads as a dotted line, not as an idle meter.
            style={{ height: `${24 + Math.min(1, Math.sqrt(value)) * 76}%` }}
          />
        ))}
      </span>
    </div>
  );
}
