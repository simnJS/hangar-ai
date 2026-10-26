import { useT } from "../i18n";
import { formatTokens, type ContextGauge } from "../lib/context";
import { gaugeLevel } from "./PaneStatus";

/** Radius of the ring on its 12-unit box, and the length of a full turn. */
const RADIUS = 4.6;
const TURN = 2 * Math.PI * RADIUS;

/**
 * How full the agent's context window is: a ring and a percentage, sized to
 * survive an 8-pane layout. The numbers behind them are in the tooltip. Amber
 * from 70 %, red from 90 %: the agent compacts on its own near the limit, and
 * these say how near that is.
 */
export function PaneGauge({ gauge, fallbackModel }: { gauge: ContextGauge; fallbackModel: string }) {
  const t = useT();
  const level = gaugeLevel(gauge.pct);
  const title = t("pane.context", {
    used: formatTokens(gauge.usedTokens),
    window: formatTokens(gauge.window),
    pct: gauge.pct,
    model: gauge.model ?? fallbackModel,
  });
  return (
    <span className={`pane__ctx pane__ctx--${level}`} title={title} role="img" aria-label={title}>
      <svg className="pane__ring" viewBox="0 0 12 12" aria-hidden="true" focusable="false">
        <circle className="pane__ring-track" cx="6" cy="6" r={RADIUS} />
        <circle
          className="pane__ring-value"
          cx="6"
          cy="6"
          r={RADIUS}
          strokeDasharray={`${((TURN * gauge.pct) / 100).toFixed(2)} ${TURN.toFixed(2)}`}
        />
      </svg>
      <span className="pane__ctxpct" aria-hidden="true">
        {t("pane.ctxPct", { pct: gauge.pct })}
      </span>
    </span>
  );
}
