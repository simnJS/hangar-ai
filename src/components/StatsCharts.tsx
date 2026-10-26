import {
  useCallback,
  useId,
  useMemo,
  useState,
  type CSSProperties,
  type KeyboardEvent,
  type PointerEvent,
  type ReactNode,
} from "react";
import { useLocale, useT } from "../i18n";
import {
  formatDuration,
  formatInteger,
  formatTick,
  formatUsd,
  heatLevel,
  type Measure,
  type Point,
} from "../lib/statsModel";

/**
 * The statistics page's charts, drawn by hand in SVG: a stacked column chart
 * of the period, the week's heatmap and the KPI sparklines.
 *
 * Every chart reads by keyboard the way it reads by pointer: it takes the
 * focus as one stop, the arrow keys walk its columns or cells, the readout
 * that follows the pointer follows the keys too, and a live region says it.
 * Each also has a table twin, so no value hides behind a hover.
 */

/** The four kinds of token, from the baseline up: cheapest nearest it. */
export const KINDS = ["cacheRead", "cacheWrite", "input", "output"] as const;
export type Kind = (typeof KINDS)[number];

const COST_OF: Record<Kind, keyof Point> = {
  cacheRead: "costCacheRead",
  cacheWrite: "costCacheWrite",
  input: "costInput",
  output: "costOutput",
};

export const kindValue = (point: Point, kind: Kind, measure: Measure) =>
  (measure === "cost" ? point[COST_OF[kind]] : point[kind]) as number;

export const totalValue = (point: Point, measure: Measure) =>
  measure === "cost" ? point.cost : point.tokens;

/** The CSS colour of a kind: steps of one ramp, see themes.ts. */
export const kindColor = (kind: Kind) => `var(--viz-${KINDS.indexOf(kind) + 1})`;

/** A value of the measure, in full: a table cell, a readout. */
export function formatMeasure(value: number, measure: Measure, locale: string): string {
  return measure === "cost" ? formatUsd(value, locale, true) : formatInteger(value, locale);
}

/** The width of an element, kept current. */
function useWidth(): [(element: HTMLElement | null) => (() => void) | undefined, number] {
  const [width, setWidth] = useState(0);
  const ref = useCallback((element: HTMLElement | null) => {
    if (!element) return;
    setWidth(element.getBoundingClientRect().width);
    const observer = new ResizeObserver(([entry]) => setWidth(entry.contentRect.width));
    observer.observe(element);
    return () => observer.disconnect();
  }, []);
  return [ref, width];
}

/**
 * Round steps for an axis: 1, 2, 2.5 or 5 times a power of ten, so the
 * ticks read as numbers someone would have chosen.
 */
export function niceTicks(max: number, count = 4): number[] {
  if (!(max > 0)) return [0];
  const rough = max / count;
  const magnitude = 10 ** Math.floor(Math.log10(rough));
  const residual = rough / magnitude;
  const nice = residual <= 1 ? 1 : residual <= 2 ? 2 : residual <= 2.5 ? 2.5 : residual <= 5 ? 5 : 10;
  const step = nice * magnitude;
  const top = Math.ceil(max / step) * step;
  const ticks: number[] = [];
  for (let value = 0; value <= top + step / 2; value += step) ticks.push(value);
  return ticks;
}

/** A readout that follows the pointer or the keys: the value first, then what it is. */
function Readout({
  x,
  width,
  title,
  rows,
  place,
}: {
  x: number;
  width: number;
  title: string;
  rows: { key: string; label: string; value: string; color?: string; strong?: boolean }[];
  /** Where it stands vertically, when not at the top of the chart. */
  place?: CSSProperties;
}) {
  // Kept inside the chart: past the middle it opens to the left.
  const right = x > width / 2;
  return (
    <div
      className={`viz-tip ${right ? "viz-tip--left" : ""}`}
      style={{ left: `${x}px`, ...place }}
      aria-hidden="true"
    >
      <p className="viz-tip__title">{title}</p>
      {rows.map((row) => (
        <p key={row.key} className={`viz-tip__row ${row.strong ? "viz-tip__row--total" : ""}`}>
          {row.color ? (
            <span className="viz-tip__key" style={{ background: row.color }} />
          ) : (
            <span className="viz-tip__key viz-tip__key--none" />
          )}
          <span className="viz-tip__value">{row.value}</span>
          <span className="viz-tip__label">{row.label}</span>
        </p>
      ))}
    </div>
  );
}

// ---------------------------------------------------------------------------
// The period, column by column
// ---------------------------------------------------------------------------

const PLOT_HEIGHT = 184;
const AXIS_HEIGHT = 22;
const GUTTER = 52;
const GAP = 2;

export function StatsColumns({
  series,
  granularity,
  measure,
  label,
  empty,
}: {
  series: Point[];
  granularity: "day" | "week";
  measure: Measure;
  /** What the chart shows, for a screen reader. */
  label: string;
  /** Said over an empty chart. */
  empty: ReactNode;
}) {
  const t = useT();
  const locale = useLocale();
  const [ref, width] = useWidth();
  const [active, setActive] = useState<number | null>(null);
  const [keyboard, setKeyboard] = useState(false);
  const hintId = useId();
  // useId's characters are not all welcome in a url(#…) reference.
  const clipBase = `viz-col-${hintId.replace(/[^\w-]/g, "")}`;

  const dayLabel = useMemo(
    () => new Intl.DateTimeFormat(locale, { day: "numeric", month: "short" }),
    [locale],
  );
  const longLabel = useMemo(
    () => new Intl.DateTimeFormat(locale, { weekday: "long", day: "numeric", month: "long" }),
    [locale],
  );
  const shortDay = useMemo(
    () => new Intl.DateTimeFormat(locale, { weekday: "short", day: "numeric" }),
    [locale],
  );

  const max = Math.max(0, ...series.map((point) => totalValue(point, measure)));
  const ticks = niceTicks(max);
  const top = ticks[ticks.length - 1] || 1;
  const plotWidth = Math.max(0, width - GUTTER - 4);
  const slot = series.length ? plotWidth / series.length : 0;
  // Thin marks: a third of the slot is air, never wider than 24px.
  const barWidth = Math.max(1, Math.min(24, slot > 6 ? slot * 0.64 : slot - 1));
  const y = (value: number) => (value / top) * PLOT_HEIGHT;

  // Labels far enough apart to never touch, counted back from today so the
  // last column always has one.
  const labelEvery = Math.max(1, Math.ceil(series.length / Math.max(1, Math.floor(plotWidth / 68))));
  const titleOf = (point: Point) =>
    granularity === "week"
      ? t("stats.weekOf", { date: dayLabel.format(point.at) })
      : longLabel.format(point.at);

  const pick = (event: PointerEvent<SVGSVGElement>) => {
    if (!slot) return;
    const box = event.currentTarget.getBoundingClientRect();
    const index = Math.floor((event.clientX - box.left - GUTTER) / slot);
    setKeyboard(false);
    setActive(index >= 0 && index < series.length ? index : null);
  };

  const onKey = (event: KeyboardEvent<HTMLDivElement>) => {
    if (!series.length) return;
    const last = series.length - 1;
    const from = active ?? last;
    const next =
      event.key === "ArrowLeft"
        ? Math.max(0, from - 1)
        : event.key === "ArrowRight"
          ? Math.min(last, from + 1)
          : event.key === "Home"
            ? 0
            : event.key === "End"
              ? last
              : null;
    if (event.key === "Escape" && active !== null) {
      event.preventDefault();
      event.stopPropagation();
      setActive(null);
      return;
    }
    if (next === null) return;
    event.preventDefault();
    setKeyboard(true);
    setActive(next);
  };

  const shown = active !== null ? series[active] : null;
  const readoutRows = shown
    ? [
        ...[...KINDS].reverse().map((kind) => ({
          key: kind,
          label: t(`stats.kind.${kind}`),
          value: formatMeasure(kindValue(shown, kind, measure), measure, locale),
          color: kindColor(kind),
        })),
        {
          key: "total",
          label: t("stats.total"),
          value: formatMeasure(totalValue(shown, measure), measure, locale),
          strong: true,
        },
      ]
    : [];
  const spoken = shown
    ? `${titleOf(shown)}. ${readoutRows.map((row) => `${row.label} ${row.value}`).join(", ")}`
    : "";

  return (
    <div
      className="viz-columns"
      ref={ref}
      tabIndex={0}
      role="group"
      aria-label={label}
      aria-describedby={hintId}
      onKeyDown={onKey}
      onFocus={(event) => {
        // The keys show the readout; a click that focused the chart does not.
        if (event.currentTarget.matches(":focus-visible") && active === null) {
          setKeyboard(true);
          setActive(series.length - 1);
        }
      }}
      onBlur={() => {
        if (keyboard) setActive(null);
      }}
    >
      <span id={hintId} className="stats-sr">
        {t("stats.chartKeys")}
      </span>
      {width > 0 && (
        <svg
          className="viz-columns__svg"
          width={width}
          height={PLOT_HEIGHT + AXIS_HEIGHT + 8}
          onPointerMove={pick}
          onPointerLeave={() => !keyboard && setActive(null)}
          aria-hidden="true"
        >
          <g transform="translate(0 6)">
            {ticks.map((tick) => (
              <g key={tick} transform={`translate(0 ${PLOT_HEIGHT - y(tick)})`}>
                <line
                  className={tick === 0 ? "viz-axis" : "viz-grid"}
                  x1={GUTTER}
                  x2={GUTTER + plotWidth}
                />
                <text className="viz-tick" x={GUTTER - 8} dy="0.32em" textAnchor="end">
                  {formatTick(tick, measure, locale)}
                </text>
              </g>
            ))}
            {active !== null && slot > 0 && (
              <rect
                className="viz-columns__band"
                x={GUTTER + active * slot}
                y={0}
                width={slot}
                height={PLOT_HEIGHT}
                rx={3}
              />
            )}
            {series.map((point, index) => {
              const total = totalValue(point, measure);
              if (total <= 0) return null;
              const x = GUTTER + index * slot + (slot - barWidth) / 2;
              const height = y(total);
              const radius = Math.min(4, barWidth / 2);
              let base = PLOT_HEIGHT;
              const parts = KINDS.map((kind) => ({ kind, h: y(kindValue(point, kind, measure)) }))
                .filter((part) => part.h > 0);
              return (
                <g
                  key={point.at}
                  className={active !== null && active !== index ? "viz-columns__col is-dim" : "viz-columns__col"}
                >
                  {/* One rounded top per column, square at the baseline: the
                      clip reaches below it, where its own corners are lost. */}
                  <clipPath id={`${clipBase}-${index}`}>
                    <rect x={x} y={PLOT_HEIGHT - height} width={barWidth} height={height + radius} rx={radius} />
                  </clipPath>
                  <g clipPath={`url(#${clipBase}-${index})`}>
                    {parts.map((part, at) => {
                      // A 2px gap of the card between two kinds, taken from
                      // the lower one when it is tall enough to give it.
                      const gap = at < parts.length - 1 && part.h > GAP + 1 ? GAP : 0;
                      const rect = (
                        <rect
                          key={part.kind}
                          x={x}
                          y={base - part.h + gap}
                          width={barWidth}
                          height={Math.max(0, part.h - gap)}
                          fill={kindColor(part.kind)}
                        />
                      );
                      base -= part.h;
                      return rect;
                    })}
                  </g>
                </g>
              );
            })}
            {series.map((point, index) => {
              if ((series.length - 1 - index) % labelEvery !== 0) return null;
              const x = GUTTER + index * slot + slot / 2;
              const text =
                granularity === "day" && series.length <= 7
                  ? shortDay.format(point.at)
                  : dayLabel.format(point.at);
              return (
                <text
                  key={point.at}
                  className="viz-tick"
                  x={x}
                  y={PLOT_HEIGHT + 16}
                  textAnchor={index === series.length - 1 && series.length > 7 ? "end" : "middle"}
                >
                  {text}
                </text>
              );
            })}
          </g>
        </svg>
      )}
      {max <= 0 && <div className="viz-empty">{empty}</div>}
      {shown && active !== null && (
        <Readout
          x={GUTTER + active * slot + slot / 2}
          width={width}
          title={titleOf(shown)}
          rows={readoutRows}
        />
      )}
      <span className="stats-sr" aria-live="polite">
        {keyboard ? spoken : ""}
      </span>
    </div>
  );
}

/** The table twin of StatsColumns: every value, latest first. */
export function StatsColumnsTable({
  series,
  granularity,
  measure,
}: {
  series: Point[];
  granularity: "day" | "week";
  measure: Measure;
}) {
  const t = useT();
  const locale = useLocale();
  const date = new Intl.DateTimeFormat(locale, {
    weekday: granularity === "day" ? "short" : undefined,
    day: "numeric",
    month: "short",
    year: "numeric",
  });
  const kinds = [...KINDS].reverse();
  return (
    <div className="viz-table">
      <table>
        <thead>
          <tr>
            <th scope="col">{t(granularity === "week" ? "stats.colWeek" : "stats.colDay")}</th>
            {kinds.map((kind) => (
              <th key={kind} scope="col">
                {t(`stats.kind.${kind}`)}
              </th>
            ))}
            <th scope="col">{t("stats.total")}</th>
          </tr>
        </thead>
        <tbody>
          {[...series].reverse().map((point) => (
            <tr key={point.at}>
              <th scope="row">{date.format(point.at)}</th>
              {kinds.map((kind) => (
                <td key={kind}>{formatMeasure(kindValue(point, kind, measure), measure, locale)}</td>
              ))}
              <td>{formatMeasure(totalValue(point, measure), measure, locale)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

// ---------------------------------------------------------------------------
// The week, hour by hour
// ---------------------------------------------------------------------------

/** Monday to Sunday, in the language's short form. */
function weekdayNames(locale: string, style: "short" | "long"): string[] {
  const format = new Intl.DateTimeFormat(locale, { weekday: style });
  // 2024-01-01 was a Monday.
  return Array.from({ length: 7 }, (_, day) => format.format(new Date(2024, 0, 1 + day)));
}

export function StatsHeatmap({ heat, max, label }: { heat: number[][]; max: number; label: string }) {
  const t = useT();
  const locale = useLocale();
  const [active, setActive] = useState<[number, number] | null>(null);
  const [keyboard, setKeyboard] = useState(false);
  const [ref, width] = useWidth();
  const hintId = useId();
  const short = useMemo(() => weekdayNames(locale, "short"), [locale]);
  const long = useMemo(() => weekdayNames(locale, "long"), [locale]);
  const hour = (h: number) => t("stats.heatHour", { h });

  const onKey = (event: KeyboardEvent<HTMLDivElement>) => {
    const [day, h] = active ?? [0, new Date().getHours()];
    const moves: Record<string, [number, number]> = {
      ArrowUp: [Math.max(0, day - 1), h],
      ArrowDown: [Math.min(6, day + 1), h],
      ArrowLeft: [day, Math.max(0, h - 1)],
      ArrowRight: [day, Math.min(23, h + 1)],
      Home: [day, 0],
      End: [day, 23],
    };
    if (event.key === "Escape" && active) {
      event.preventDefault();
      event.stopPropagation();
      setActive(null);
      return;
    }
    const next = moves[event.key];
    if (!next) return;
    event.preventDefault();
    setKeyboard(true);
    setActive(next);
  };

  const cell = active ? heat[active[0]][active[1]] : 0;
  const spoken = active
    ? t("stats.heatCell", { day: long[active[0]], hour: hour(active[1]), value: formatDuration(cell, t) })
    : "";

  // The readout's x, in the grid's own coordinates.
  const labelWidth = 40;
  const cellWidth = width > labelWidth ? (width - labelWidth) / 24 : 0;

  return (
    <div className="viz-heat">
      <div
        className="viz-heat__grid"
        ref={ref}
        tabIndex={0}
        role="group"
        aria-label={label}
        aria-describedby={hintId}
        onKeyDown={onKey}
        onFocus={(event) => {
          if (event.currentTarget.matches(":focus-visible") && !active) {
            setKeyboard(true);
            setActive([(new Date().getDay() + 6) % 7, new Date().getHours()]);
          }
        }}
        onBlur={() => keyboard && setActive(null)}
        onPointerLeave={() => !keyboard && setActive(null)}
      >
        <span id={hintId} className="stats-sr">
          {t("stats.heatKeys")}
        </span>
        <div className="viz-heat__hours" aria-hidden="true">
          {[0, 6, 12, 18].map((h) => (
            <span key={h} style={{ gridColumn: `${h + 2} / span 6` }}>
              {hour(h)}
            </span>
          ))}
        </div>
        {heat.map((hours, day) => (
          <div key={day} className="viz-heat__row" aria-hidden="true">
            <span className="viz-heat__day">{short[day]}</span>
            {hours.map((value, h) => {
              const on = active?.[0] === day && active?.[1] === h;
              return (
                <span
                  key={h}
                  className={`viz-heat__cell ${on ? "is-active" : ""}`}
                  style={{ background: `var(--viz-heat-${heatLevel(value, max)})` }}
                  onPointerEnter={() => {
                    setKeyboard(false);
                    setActive([day, h]);
                  }}
                />
              );
            })}
          </div>
        ))}
        {active && cellWidth > 0 && (
          <Readout
            x={labelWidth + (active[1] + 0.5) * cellWidth}
            width={width}
            // Beside the hovered row, never over it: under it in the first
            // half of the week, above it in the second.
            place={
              active[0] < 4
                ? { top: `calc(var(--heat-head) + ${active[0] + 1} * (var(--heat-cell) + 2px) + 6px)` }
                : { top: "auto", bottom: `calc(${7 - active[0]} * (var(--heat-cell) + 2px) + 6px)` }
            }
            title={`${long[active[0]]}, ${hour(active[1])}`}
            rows={[{ key: "v", label: t("stats.kpiActive"), value: formatDuration(cell, t), color: `var(--viz-heat-${Math.max(1, heatLevel(cell, max))})` }]}
          />
        )}
        <span className="stats-sr" aria-live="polite">
          {keyboard ? spoken : ""}
        </span>
      </div>
      <div className="viz-heat__legend" aria-hidden="true">
        <span>{t("stats.heatLess")}</span>
        {[0, 1, 2, 3, 4, 5].map((level) => (
          <span key={level} className="viz-heat__swatch" style={{ background: `var(--viz-heat-${level})` }} />
        ))}
        <span>{t("stats.heatMore")}</span>
      </div>
    </div>
  );
}

/** The heatmap's table twin: a row per weekday, a column per hour. */
export function StatsHeatTable({ heat }: { heat: number[][] }) {
  const t = useT();
  const locale = useLocale();
  const long = weekdayNames(locale, "long");
  return (
    <div className="viz-table viz-table--wide">
      <table>
        <thead>
          <tr>
            <th scope="col">{t("stats.colDay")}</th>
            {Array.from({ length: 24 }, (_, h) => (
              <th key={h} scope="col">
                {t("stats.heatHour", { h })}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {heat.map((hours, day) => (
            <tr key={day}>
              <th scope="row">{long[day]}</th>
              {hours.map((value, h) => (
                <td key={h}>{value > 0 ? formatDuration(value, t) : "–"}</td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Sparklines
// ---------------------------------------------------------------------------

/** A trend at a glance: the line in a quiet grey, today's point in the accent. */
export function Sparkline({ values }: { values: number[] }) {
  const width = 88;
  const height = 26;
  if (values.length < 2) return null;
  const max = Math.max(...values);
  const step = width / (values.length - 1);
  const yOf = (value: number) => (max > 0 ? height - 3 - (value / max) * (height - 6) : height - 3);
  const points = values.map((value, index) => `${(index * step).toFixed(1)},${yOf(value).toFixed(1)}`);
  const lastX = (values.length - 1) * step;
  return (
    <svg className="viz-spark" width={width + 4} height={height} viewBox={`-2 0 ${width + 4} ${height}`} aria-hidden="true">
      <polyline points={points.join(" ")} fill="none" />
      <circle cx={lastX} cy={yOf(values[values.length - 1])} r={2.75} />
    </svg>
  );
}
