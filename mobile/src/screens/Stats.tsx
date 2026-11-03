import { useCallback, useEffect, useRef, useState, type KeyboardEvent, type PointerEvent } from "react";
import type { Stats as StatsAnswer, StatsMeasure, StatsPeriod } from "../api";
import { EmptyState, PcChip } from "../components/Chrome";
import { Icon } from "../components/Icon";
import { errorText } from "../errors";
import { i18n, t } from "../i18n";
import { startPolling } from "../poll";
import { useRemoteContext } from "../remote";
import {
  MEASURES,
  PERIODS,
  chartOf,
  columnAt,
  kpis,
  planView,
  rankRows,
  readout,
  unpricedNote,
  when,
  type Chart,
  type Kpi,
  type PlanView,
  type RankRow,
} from "../stats";
import { load, loadJson, save } from "../storage";

/** The usage is counted again this often while the tab is open: it moves by the minute, not the second. */
const REFRESH_MS = 60_000;

const pick = <T extends string>(value: string | null, options: readonly T[], fallback: T): T =>
  options.find((option) => option === value) ?? fallback;

/**
 * Stats: what the agents used, as the PC's statistics page counts it. The
 * plan's windows first, the one number a phone is opened for; then the
 * period and the measure, which scope everything under them; the totals,
 * the chart a day, and the projects, agents and models that used the most.
 */
export function Stats({ now }: { now: number }) {
  const { client, pc, connection } = useRemoteContext();
  const [period, setPeriod] = useState<StatsPeriod>(() => pick(load("statsPeriod"), PERIODS, "30d"));
  const [measure, setMeasure] = useState<StatsMeasure>(() => pick(load("statsMeasure"), MEASURES, "tokens"));
  // What the phone last knew, shown at once and kept on screen while the next answer comes.
  const [stats, setStats] = useState<StatsAnswer | null>(() => loadJson<StatsAnswer>("stats"));
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const asked = useRef({ period, measure });
  asked.current = { period, measure };

  const refresh = useCallback(async () => {
    const current = () => asked.current.period === period && asked.current.measure === measure;
    setLoading(true);
    try {
      const answer = await client.stats(period, measure);
      if (!current()) return;
      setStats(answer);
      setError(null);
      save("stats", JSON.stringify(answer));
    } catch (failure) {
      if (current()) setError(errorText(failure));
    } finally {
      if (current()) setLoading(false);
    }
  }, [client, period, measure]);

  // Again at once for another period or measure, then every minute while seen.
  useEffect(() => startPolling(refresh, REFRESH_MS), [refresh]);

  const choose = <T extends string>(key: string, set: (value: T) => void) => (value: T) => {
    save(key, value);
    set(value);
  };

  const plan = stats ? planView(stats.plan, now, i18n) : null;
  const nothing = stats !== null && stats.firstAt === null;
  const unpriced = stats ? unpricedNote(stats, i18n) : null;

  return (
    <div className="page">
      <div className="top">
        <h1 className="title">{t("stats.title")}</h1>
        <PcChip pc={pc} connection={connection} />
      </div>
      {stats && (
        <p className="subtitle" aria-live="polite">
          {stats.cachedOnly ? t("stats.cachedOnly") : t("stats.updated", { time: when(stats.readAt, now, i18n) })}
        </p>
      )}

      {plan && <PlanLimits plan={plan} />}

      {stats === null ? (
        error ? (
          <EmptyState title={error}>
            <button type="button" className="btn" onClick={() => void refresh()}>
              {t("error.retry")}
            </button>
          </EmptyState>
        ) : (
          <p className="calm" role="status">
            {t("stats.loading")}
          </p>
        )
      ) : nothing ? (
        <EmptyState title={t("stats.empty")} text={t("stats.emptyHint")} mark={<Icon name="chart" size={40} />} />
      ) : (
        <>
          <div className="stats__controls">
            <Choice
              label={t("stats.period")}
              value={period}
              options={PERIODS.map((id) => ({ id, label: t(`stats.period.${id}`), hint: t(`stats.periodHint.${id}`) }))}
              onChange={choose<StatsPeriod>("statsPeriod", setPeriod)}
            />
            <Choice
              label={t("stats.measure")}
              value={measure}
              options={MEASURES.map((id) => ({ id, label: t(`stats.measure.${id}`) }))}
              onChange={choose<StatsMeasure>("statsMeasure", setMeasure)}
            />
          </div>
          {error && (
            <p className="stats__error" role="alert">
              <Icon name="alert" size={16} />
              {error}
            </p>
          )}
          <div className={`stats__body ${loading && (stats.period !== period || stats.measure !== measure) ? "is-stale" : ""}`}>
            <Totals items={kpis(stats, i18n)} />
            <Daily stats={stats} now={now} />
            <Ranking title={t("stats.projects")} rows={rankRows(stats.projects, stats.measure, i18n)} />
            <Ranking title={t("stats.agents")} rows={rankRows(stats.agents, stats.measure, i18n)} />
            <Ranking title={t("stats.models")} rows={rankRows(stats.models, stats.measure, i18n, { sessions: false })} />
            {unpriced && <p className="stats__note">{unpriced}</p>}
          </div>
        </>
      )}
    </div>
  );
}

/** A radio group drawn as one segmented control, each option a 44 px target. */
function Choice<T extends string>({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: T;
  options: { id: T; label: string; hint?: string }[];
  onChange: (value: T) => void;
}) {
  const onKey = (event: KeyboardEvent<HTMLDivElement>) => {
    const step = event.key === "ArrowRight" || event.key === "ArrowDown" ? 1 : event.key === "ArrowLeft" || event.key === "ArrowUp" ? -1 : 0;
    if (!step) return;
    event.preventDefault();
    const next = (options.findIndex((option) => option.id === value) + step + options.length) % options.length;
    onChange(options[next].id);
    event.currentTarget.querySelectorAll<HTMLElement>('[role="radio"]')[next]?.focus();
  };
  return (
    <div className="segmented stats__choice" role="radiogroup" aria-label={label} onKeyDown={onKey}>
      {options.map((option) => (
        <button
          key={option.id}
          type="button"
          role="radio"
          aria-checked={option.id === value}
          aria-label={option.hint}
          tabIndex={option.id === value ? 0 : -1}
          className="segmented__option"
          onClick={() => onChange(option.id)}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}

function PlanLimits({ plan }: { plan: PlanView }) {
  return (
    <section className="section" aria-labelledby="stats-plan">
      <div className="section__head">
        <h2 className="section__title" id="stats-plan">
          {t("stats.plan")}
        </h2>
      </div>
      <div className="panel limits">
        {plan.bars.map((bar) => (
          <div
            key={bar.kind}
            className={`limit limit--${bar.tone} ${bar.past ? "limit--past" : ""}`}
            role="meter"
            aria-label={bar.label}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={bar.percent}
            aria-valuetext={[bar.used, bar.left, bar.reset].filter(Boolean).join(", ")}
          >
            <div className="limit__head">
              <span className="limit__label">{bar.label}</span>
              {bar.left && <span className="limit__left">{bar.left}</span>}
            </div>
            <span className="limit__track" aria-hidden="true">
              <span className="limit__fill" style={{ width: `${bar.percent}%` }} />
            </span>
            <div className="limit__foot">
              {bar.used && <span>{bar.used}</span>}
              {bar.reset && <span className="limit__reset">{bar.reset}</span>}
            </div>
          </div>
        ))}
        <p className="limits__reported">{plan.reported}</p>
      </div>
    </section>
  );
}

function Totals({ items }: { items: Kpi[] }) {
  return (
    <ul className="kpis">
      {items.map((item) => (
        <li key={item.key} className="kpi">
          <span className="kpi__label">{item.label}</span>
          <span className="kpi__value">{item.value}</span>
          {item.trend && (
            <span className="kpi__trend">
              {item.trend.direction && (
                <>
                  <span className="kpi__arrow" aria-hidden="true">
                    {item.trend.direction === "up" ? "▲" : "▼"}
                  </span>
                  <span className="sr-only">{t(item.trend.direction === "up" ? "stats.trend.up" : "stats.trend.down")}</span>
                </>
              )}
              {item.trend.text}
            </span>
          )}
          {item.note && <span className="kpi__note">{item.note}</span>}
        </li>
      ))}
    </ul>
  );
}

/** The chart a day (a week past four months), or the same values as a list. */
function Daily({ stats, now }: { stats: StatsAnswer; now: number }) {
  const [list, setList] = useState(false);
  const chart = chartOf(stats, i18n);
  return (
    <section className="section" aria-labelledby="stats-daily">
      <div className="section__head">
        <h2 className="section__title" id="stats-daily">
          {chart.title}
        </h2>
        <button type="button" className="btn btn--quiet stats__view" aria-pressed={list} onClick={() => setList((on) => !on)}>
          {t(list ? "stats.view.chart" : "stats.view.list")}
        </button>
      </div>
      <div className="panel">
        {list ? <DayList stats={stats} now={now} /> : <Columns stats={stats} chart={chart} now={now} />}
      </div>
    </section>
  );
}

function Columns({ stats, chart, now }: { stats: StatsAnswer; chart: Chart; now: number }) {
  const [picked, setPicked] = useState<number | null>(null);
  const index = picked !== null && picked < chart.columns.length ? picked : chart.initial;
  const read = readout(stats, index, now, i18n);
  const count = chart.columns.length;

  // A finger on the plot reads the column under it, and sliding along reads the next.
  const follow = (event: PointerEvent<HTMLDivElement>) => {
    const box = event.currentTarget.getBoundingClientRect();
    const at = columnAt(event.clientX - box.left, box.width, count);
    if (at >= 0) setPicked(at);
  };
  const onKey = (event: KeyboardEvent<HTMLDivElement>) => {
    const step = event.key === "ArrowRight" ? 1 : event.key === "ArrowLeft" ? -1 : 0;
    if (!step) return;
    event.preventDefault();
    setPicked(Math.min(count - 1, Math.max(0, index + step)));
  };

  if (chart.empty) return <p className="vchart__empty">{t("stats.chart.none")}</p>;
  return (
    <figure className="vchart">
      {read && (
        <figcaption className="vchart__readout" aria-live="polite">
          <span className="vchart__value">{read.value}</span>
          <span className="vchart__date">{read.date}</span>
        </figcaption>
      )}
      <div className="vchart__frame">
        <div className="vchart__ticks" aria-hidden="true">
          {chart.ticks.map((tick) => (
            <span key={tick.at} className="vchart__tick" style={{ bottom: `${tick.at * 100}%` }}>
              <span className="vchart__tick-label">{tick.label}</span>
            </span>
          ))}
        </div>
        <div
          className={`vchart__plot ${count > 45 ? "vchart__plot--dense" : ""}`}
          role="img"
          aria-label={chart.summary}
          tabIndex={0}
          onPointerDown={follow}
          onPointerMove={follow}
          onKeyDown={onKey}
        >
          {chart.columns.map((column, at) => (
            <span key={column.at} className={`vchart__col ${at === index ? "is-on" : ""}`}>
              <span
                className={`vchart__bar ${column.value > 0 ? "has-value" : ""}`}
                style={{ height: `${Math.min(1, column.height) * 100}%` }}
              />
            </span>
          ))}
        </div>
      </div>
      <div className="vchart__axis" aria-hidden="true">
        {chart.axis.map(({ index: at, label }) => (
          <span
            key={at}
            className={`vchart__label ${count > 7 && at === 0 ? "is-first" : ""} ${count > 7 && at === count - 1 ? "is-last" : ""}`}
            style={{ left: `${((at + 0.5) / count) * 100}%` }}
          >
            {label}
          </span>
        ))}
      </div>
    </figure>
  );
}

/** The chart's values, newest first: what a screen reader, or anyone, can read line by line. */
function DayList({ stats, now }: { stats: StatsAnswer; now: number }) {
  const rows = stats.series.map((_, at) => readout(stats, at, now, i18n)!).reverse();
  return (
    <ol className="vlist">
      {rows.map((row, at) => (
        <li key={at} className="vlist__row">
          <span className="vlist__date">{row.date}</span>
          <span className="vlist__value">{row.value}</span>
        </li>
      ))}
    </ol>
  );
}

function Ranking({ title, rows }: { title: string; rows: RankRow[] }) {
  if (rows.length === 0) return null;
  return (
    <section className="section" aria-label={title}>
      <div className="section__head">
        <h2 className="section__title">{title}</h2>
      </div>
      <ul className="list rank">
        {rows.map((row) => (
          <li key={row.key} className={`rank__row ${row.other ? "rank__row--other" : ""}`}>
            <div className="rank__line">
              <span className="rank__name">{row.label}</span>
              <span className="rank__value">{row.value}</span>
            </div>
            <div className="rank__line rank__line--sub">
              <span className="rank__sub">{row.sub}</span>
              <span className="rank__share">{row.share}</span>
            </div>
            <span className="rank__track" aria-hidden="true">
              <span className="rank__fill" style={{ width: `${row.width}%` }} />
            </span>
          </li>
        ))}
      </ul>
    </section>
  );
}
