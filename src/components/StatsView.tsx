import { useEffect, useMemo, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { Icon } from "./Icon";
import { ToolbarMenu, type MenuCloseReason } from "./ToolbarMenu";
import {
  Sparkline,
  StatsColumns,
  StatsColumnsTable,
  StatsHeatTable,
  StatsHeatmap,
  KINDS,
  kindColor,
} from "./StatsCharts";
import { useLocale, useT, type Translator } from "../i18n";
import { gitWorktreeList } from "../lib/git";
import { proseTime } from "../lib/inspectorFormat";
import { useUsageStats, type StatsAgent } from "../lib/stats";
import {
  PERIODS,
  change,
  formatChange,
  formatCompact,
  formatDuration,
  formatInteger,
  formatShare,
  formatUsd,
  measureOf,
  projectFor,
  summarize,
  workspaceRoots,
  type Measure,
  type Period,
  type Ranked,
  type SessionRow,
  type Summary,
} from "../lib/statsModel";
import { AGENTS, type Workspace } from "../types";

/**
 * The usage statistics: what the agents did, in tokens, time and the money
 * it would have cost at API prices, across every project or one.
 *
 * One row of controls scopes the whole page — the period, the project, and
 * whether the charts measure tokens or cost — so every number on screen is
 * about the same slice. What was picked lasts as long as the window does.
 */

const choices: { period: Period; measure: Measure; project: string | null } = {
  period: "30d",
  measure: "tokens",
  project: null,
};

/** How many projects show before the rest fold under "N more". */
const PROJECTS_SHOWN = 7;

/** A radio group drawn as a segmented control; the arrows walk it. */
function Choice<T extends string>({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: T;
  options: { id: T; label: string; hint: string }[];
  onChange: (value: T) => void;
}) {
  const onKey = (event: KeyboardEvent<HTMLDivElement>) => {
    const at = options.findIndex((option) => option.id === value);
    const step = event.key === "ArrowRight" || event.key === "ArrowDown" ? 1 : event.key === "ArrowLeft" || event.key === "ArrowUp" ? -1 : 0;
    let next = step ? (at + step + options.length) % options.length : -1;
    if (event.key === "Home") next = 0;
    if (event.key === "End") next = options.length - 1;
    if (next < 0) return;
    event.preventDefault();
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
          tabIndex={option.id === value ? 0 : -1}
          className="segmented__btn"
          title={option.hint}
          onClick={() => onChange(option.id)}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}

/** A card of the page: a title, a quiet hint, and what it holds. */
function Card({
  title,
  hint,
  actions,
  className = "",
  children,
}: {
  title: string;
  hint?: string;
  actions?: ReactNode;
  className?: string;
  children: ReactNode;
}) {
  return (
    <section className={`scard ${className}`} aria-label={title}>
      <header className="scard__head">
        <div className="scard__titles">
          <h3 className="scard__title">{title}</h3>
          {hint && <p className="scard__hint">{hint}</p>}
        </div>
        {actions && <div className="scard__actions">{actions}</div>}
      </header>
      {children}
    </section>
  );
}

/** Chart or table: the same values, two ways to read them. */
function TableToggle({ table, onToggle }: { table: boolean; onToggle: () => void }) {
  const t = useT();
  const label = t(table ? "stats.showChart" : "stats.showTable");
  return (
    <button type="button" className="tbtn scard__toggle" onClick={onToggle} title={label} aria-label={label} aria-pressed={table}>
      <Icon name={table ? "chart" : "table"} size={15} />
    </button>
  );
}

function Trend({ now, before, days }: { now: number; before: number | null; days: number }) {
  const t = useT();
  const locale = useLocale();
  if (before === null) return null;
  const ratio = change(now, before);
  if (ratio === null) {
    return <p className="kpi__trend">{now > 0 ? t("stats.trendNew", { n: days }) : ""}</p>;
  }
  // Rounded to nothing, it is no trend.
  const flat = Math.abs(ratio) < 0.005;
  return (
    <p className="kpi__trend">
      {!flat && (
        <span className="kpi__arrow" aria-hidden="true">
          {ratio > 0 ? "▲" : "▼"}
        </span>
      )}
      {!flat && <span className="stats-sr">{t(ratio > 0 ? "stats.trendUp" : "stats.trendDown")}</span>}
      {flat ? (
        t("stats.trendFlat", { n: days })
      ) : (
        <>
          <span className="kpi__change">{formatChange(ratio, locale)}</span> {t("stats.trendVs", { n: days })}
        </>
      )}
    </p>
  );
}

function Kpi({
  label,
  value,
  detail,
  trend,
  spark,
  note,
}: {
  label: string;
  value: string;
  detail?: string;
  trend?: ReactNode;
  spark: number[];
  note?: boolean;
}) {
  return (
    <article className="kpi">
      <div className="kpi__top">
        <h3 className="kpi__label">{label}</h3>
        <Sparkline values={spark} />
      </div>
      <p className="kpi__value">{value}</p>
      {trend}
      {detail && <p className={`kpi__detail ${note ? "kpi__detail--note" : ""}`}>{detail}</p>}
    </article>
  );
}

function valueOf(row: { tokens: number; cost: number }, measure: Measure, locale: string): string {
  return measure === "cost" ? formatUsd(row.cost, locale) : formatCompact(row.tokens, locale);
}

/** A ranking: one bar per row, its length against the first. */
function RankedList({
  rows,
  measure,
  selected,
  onPick,
  pickLabel,
  sub,
}: {
  rows: Ranked[];
  measure: Measure;
  selected?: string | null;
  onPick?: (key: string) => void;
  pickLabel?: (row: Ranked) => string;
  sub?: (row: Ranked) => string;
}) {
  const t = useT();
  const locale = useLocale();
  const max = Math.max(0, ...rows.map((row) => measureOf(row, measure)));
  return (
    <ul className={`rank ${selected ? "rank--picking" : ""}`}>
      {rows.map((row) => {
        const value = measureOf(row, measure);
        const width = max > 0 ? Math.max(value > 0 ? 0.6 : 0, (value / max) * 100) : 0;
        const body = (
          <>
            <span className="rank__text">
              <span className="rank__name">{row.label}</span>
              {(row.detail || sub) && (
                <span className="rank__detail" title={row.detail || undefined}>
                  {sub ? sub(row) : row.detail}
                </span>
              )}
            </span>
            <span className="rank__numbers">
              {row.unpriced && measure === "cost" ? (
                <span className="rank__badge">{t("stats.unpriced")}</span>
              ) : (
                <span className="rank__value">{valueOf(row, measure, locale)}</span>
              )}
              <span className="rank__share">{formatShare(row.share, locale)}</span>
            </span>
            <span className="rank__track" aria-hidden="true">
              <span className="rank__fill" style={{ width: `${width}%` }} />
            </span>
          </>
        );
        const on = selected === row.key;
        return (
          <li key={row.key} className={on ? "is-selected" : ""}>
            {onPick ? (
              <button
                type="button"
                className="rank__row rank__row--button"
                aria-pressed={on}
                title={pickLabel?.(row)}
                onClick={() => onPick(row.key)}
              >
                {body}
              </button>
            ) : (
              <div className="rank__row">{body}</div>
            )}
          </li>
        );
      })}
    </ul>
  );
}

function SessionList({
  rows,
  measure,
  agentLabel,
  onProject,
}: {
  rows: SessionRow[];
  measure: Measure;
  agentLabel: (agent: StatsAgent) => string;
  onProject: (key: string) => void;
}) {
  const t = useT();
  const locale = useLocale();
  const date = new Intl.DateTimeFormat(locale, { day: "numeric", month: "short" });
  if (!rows.length) return <p className="scard__empty">{t("stats.noData")}</p>;
  return (
    <ol className="slist">
      {rows.map((row, at) => (
        <li key={`${row.agent}:${row.id}`} className="slist__row">
          <span className="slist__rank" aria-hidden="true">
            {at + 1}
          </span>
          <span className="slist__main">
            <span className="slist__title" title={row.title || undefined}>
              {row.title || t("stats.untitled")}
            </span>
            <span className="slist__meta">
              <button
                type="button"
                className="slist__project"
                onClick={() => onProject(row.project.key)}
                title={t("stats.showOnly", { name: row.project.name })}
              >
                {row.project.name}
              </button>
              <span className="slist__sep" aria-hidden="true">
                ·
              </span>
              <span>{[agentLabel(row.agent), row.model].filter(Boolean).join(" · ")}</span>
              <span className="slist__sep" aria-hidden="true">
                ·
              </span>
              <span className="slist__date">{date.format(row.lastAt)}</span>
            </span>
          </span>
          <span className="slist__numbers">
            <span className="slist__value">{valueOf(row, measure, locale)}</span>
            <span className="slist__active">{t("stats.sessionActive", { value: formatDuration(row.activeMs, t) })}</span>
          </span>
        </li>
      ))}
    </ol>
  );
}

function Legend({ t }: { t: Translator }) {
  return (
    <ul className="viz-legend" aria-label={t("stats.dailyHint", { measure: "" }).trim()}>
      {[...KINDS].reverse().map((kind) => (
        <li key={kind}>
          <span className="viz-legend__swatch" style={{ background: kindColor(kind) }} />
          {t(`stats.kind.${kind}`)}
        </li>
      ))}
    </ul>
  );
}

/** A state of the whole page: reading, nothing to show, or a failure. */
function PageState({
  icon,
  title,
  body,
  busy,
  action,
}: {
  icon: "chart" | "alert";
  title: string;
  body: string;
  busy?: boolean;
  action?: ReactNode;
}) {
  return (
    <div className="stats__state" role={busy ? "status" : undefined}>
      <span className={`stats__state-icon ${busy ? "is-busy" : ""}`} aria-hidden="true">
        <Icon name={icon} size={22} />
      </span>
      <h3 className="stats__state-title">{title}</h3>
      <p className="stats__state-body">{body}</p>
      {action}
    </div>
  );
}

export function StatsView({ workspaces }: { workspaces: Workspace[] }) {
  const t = useT();
  const locale = useLocale();
  const { data, refreshing, error, checkedAt, refresh } = useUsageStats();
  const [period, setPeriodState] = useState<Period>(choices.period);
  const [measure, setMeasureState] = useState<Measure>(choices.measure);
  const [project, setProjectState] = useState<string | null>(choices.project);
  const [dailyTable, setDailyTable] = useState(false);
  const [heatTable, setHeatTable] = useState(false);
  const [allProjects, setAllProjects] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const pickerRef = useRef<HTMLButtonElement>(null);

  const setPeriod = (value: Period) => setPeriodState((choices.period = value));
  const setMeasure = (value: Measure) => setMeasureState((choices.measure = value));
  const setProject = (value: string | null) => setProjectState((choices.project = value));

  // Worktrees live beside their repository, under names of their own: git
  // knows which belong to which workspace.
  const [worktrees, setWorktrees] = useState<Record<string, string[]>>({});
  const repos = workspaces.map((ws) => `${ws.id}\n${ws.cwd}`).join("\n");
  useEffect(() => {
    let cancelled = false;
    Promise.all(
      workspaces.map((ws) =>
        gitWorktreeList(ws.cwd)
          .then((list) => [ws.id, list.map((tree) => tree.path)] as const)
          .catch(() => [ws.id, [] as string[]] as const),
      ),
    ).then((pairs) => {
      if (!cancelled) setWorktrees(Object.fromEntries(pairs));
    });
    return () => {
      cancelled = true;
    };
    // Only a change of folders is worth asking git again.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [repos]);

  const roots = useMemo(() => workspaceRoots(workspaces, worktrees), [workspaces, worktrees]);
  const projects = useMemo(
    () => (data ? data.sessions.map((session) => projectFor(session.cwd, roots)) : []),
    [data, roots],
  );
  const agentLabel = (agent: StatsAgent) => AGENTS.find((entry) => entry.id === agent)?.label ?? agent;

  const summary: Summary | null = useMemo(
    () =>
      data
        ? summarize(data, { period, project, measure, now: Date.now(), projects, agentLabel })
        : null,
    // agentLabel reads a constant table.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [data, period, project, measure, projects],
  );

  // A filter on a project the period no longer has keeps its name.
  const projectName =
    project === null
      ? null
      : (summary?.projects.find((row) => row.key === project)?.label ??
        projects.find((entry) => entry.key === project)?.name ??
        null);

  const closeMenu = (reason: MenuCloseReason) => {
    setMenuOpen(false);
    if (reason === "escape" || reason === "tab") pickerRef.current?.focus();
  };

  const status = refreshing ? (
    <span className="stats__status is-busy" role="status">
      <span className="stats__spinner" aria-hidden="true" />
      {t("stats.refreshing")}
    </span>
  ) : checkedAt ? (
    <span className="stats__status">{t("stats.upToDate", { time: proseTime(checkedAt, locale) })}</span>
  ) : data?.scan.cachedOnly ? (
    <span className="stats__status">{t("stats.fromCache")}</span>
  ) : null;

  const bar = (
    <header className="stats__bar">
      <h2 className="stats__title">{t("stats.title")}</h2>
      <Choice
        label={t("stats.period")}
        value={period}
        options={PERIODS.map((id) => ({ id, label: t(`stats.period.${id}`), hint: t(`stats.periodHint.${id}`) }))}
        onChange={setPeriod}
      />
      <div className="stats__project">
        <button
          ref={pickerRef}
          type="button"
          className={`stats__picker ${project ? "is-filtered" : ""} ${menuOpen ? "is-open" : ""}`}
          aria-haspopup="menu"
          aria-expanded={menuOpen}
          title={t("stats.projectFilter")}
          onClick={() => setMenuOpen((open) => !open)}
          disabled={!summary}
        >
          <Icon name="folder" size={14} />
          <span className="stats__picker-label">{projectName ?? t("stats.allProjects")}</span>
          <Icon name="chevronDown" size={12} />
        </button>
        {project && (
          <button
            type="button"
            className="tbtn stats__clear"
            onClick={() => setProject(null)}
            title={t("stats.clearProject")}
            aria-label={t("stats.clearProject")}
          >
            <Icon name="close" size={13} />
          </button>
        )}
      </div>
      <div className="stats__bar-end">
        {status}
        <Choice
          label={t("stats.measure")}
          value={measure}
          options={(["tokens", "cost"] as const).map((id) => ({
            id,
            label: t(`stats.measure.${id}`),
            hint: t(`stats.measureHint.${id}`),
          }))}
          onChange={setMeasure}
        />
        <button
          type="button"
          className="tbtn"
          onClick={refresh}
          disabled={refreshing}
          title={t("stats.refresh")}
          aria-label={t("stats.refresh")}
        >
          <Icon name="restart" size={15} />
        </button>
      </div>
      {menuOpen && summary && (
        <ToolbarMenu anchorRef={pickerRef} label={t("stats.projectMenu")} onClose={closeMenu}>
          <button
            type="button"
            role="menuitem"
            className="tmenu__item"
            aria-current={project === null}
            onClick={() => {
              setProject(null);
              setMenuOpen(false);
              pickerRef.current?.focus();
            }}
          >
            <span className="stats__check">{project === null && <Icon name="check" size={14} />}</span>
            <span className="tmenu__label">{t("stats.allProjects")}</span>
          </button>
          <div className="tmenu__sep" role="separator" />
          {summary.projects.map((row) => (
            <button
              key={row.key}
              type="button"
              role="menuitem"
              className="tmenu__item"
              aria-current={project === row.key}
              title={row.detail}
              onClick={() => {
                setProject(row.key);
                setMenuOpen(false);
                pickerRef.current?.focus();
              }}
            >
              <span className="stats__check">{project === row.key && <Icon name="check" size={14} />}</span>
              <span className="tmenu__label">{row.label}</span>
              <span className="tmenu__keys">{formatShare(row.share, locale)}</span>
            </button>
          ))}
        </ToolbarMenu>
      )}
    </header>
  );

  // Nothing to draw yet: the page says why, in the frame it will fill.
  if (!summary || !data) {
    return (
      <section className="stats" aria-label={t("stats.label")}>
        {bar}
        <div className="stats__scroll">
          {error ? (
            <PageState
              icon="alert"
              title={t("stats.errorTitle")}
              body={error}
              action={
                <button type="button" className="btn" onClick={refresh}>
                  {t("stats.retry")}
                </button>
              }
            />
          ) : (
            <PageState icon="chart" busy title={t("stats.loadingTitle")} body={t("stats.loadingBody")} />
          )}
        </div>
      </section>
    );
  }

  if (!data.buckets.length && !refreshing) {
    return (
      <section className="stats" aria-label={t("stats.label")}>
        {bar}
        <div className="stats__scroll">
          <PageState
            icon="chart"
            title={t("stats.emptyTitle")}
            body={t("stats.emptyBody")}
            action={
              // Where it looked: the first thing to check when a machine
              // with transcripts shows none.
              data.sources.length > 0 && (
                <ul className="stats__sources" aria-label={t("stats.sourcesLabel")}>
                  {data.sources.map((source) => (
                    <li key={source.root}>
                      <span className="stats__source-root">{source.root}</span>
                      <span className="stats__source-count">
                        {source.found ? t("stats.sourceFiles", { n: source.files }) : t("stats.sourceMissing")}
                      </span>
                    </li>
                  ))}
                </ul>
              )
            }
          />
        </div>
      </section>
    );
  }

  const { totals, previous, series, range } = summary;
  const days = range.days;
  const trend = (now: number, before: number | undefined) =>
    previous ? <Trend now={now} before={before ?? 0} days={days} /> : null;
  const since =
    period === "all" && summary.firstAt !== null
      ? new Intl.DateTimeFormat(locale, { day: "numeric", month: "long", year: "numeric" }).format(summary.firstAt)
      : null;
  const sinceLine = since ? <p className="kpi__trend">{t("stats.since", { date: since })}</p> : null;
  const cacheShare = totals.tokens > 0 ? totals.cacheRead / totals.tokens : 0;
  const measureName = t(`stats.measure.${measure}`);

  const visibleProjects =
    allProjects || summary.projects.length <= PROJECTS_SHOWN + 1
      ? summary.projects
      : summary.projects.filter((row, at) => at < PROJECTS_SHOWN || row.key === project);
  const hiddenProjects = summary.projects.length - visibleProjects.length;

  return (
    <section className="stats" aria-label={t("stats.label")}>
      {bar}
      <div className={`stats__scroll ${refreshing && data.scan.cachedOnly ? "is-stale" : ""}`}>
        <div className="stats__page">
          {error && (
            <p className="stats__error" role="alert">
              <Icon name="alert" size={14} />
              <span>
                {t("stats.errorTitle")} — {error}
              </span>
            </p>
          )}

          <div className="stats__kpis">
            <Kpi
              label={t("stats.kpiTokens")}
              value={formatCompact(totals.tokens, locale)}
              trend={trend(totals.tokens, previous?.tokens) ?? sinceLine}
              detail={t("stats.kpiTokensDetail", { share: formatShare(cacheShare, locale) })}
              spark={series.map((point) => point.tokens)}
            />
            <Kpi
              label={t("stats.kpiCost")}
              value={formatUsd(totals.cost, locale)}
              trend={trend(totals.cost, previous?.cost) ?? sinceLine}
              detail={t("stats.kpiCostNote")}
              note
              spark={series.map((point) => point.cost)}
            />
            <Kpi
              label={t("stats.kpiActive")}
              value={formatDuration(totals.activeMs, t)}
              trend={trend(totals.activeMs, previous?.activeMs) ?? sinceLine}
              detail={
                summary.activeDays > 0
                  ? t("stats.kpiActiveDetail", { value: formatDuration(totals.activeMs / summary.activeDays, t) })
                  : undefined
              }
              spark={series.map((point) => point.activeMs)}
            />
            <Kpi
              label={t("stats.kpiSessions")}
              value={formatInteger(totals.sessions, locale)}
              trend={trend(totals.sessions, previous?.sessions) ?? sinceLine}
              detail={t("stats.prompts", { n: totals.turns })}
              spark={series.map((point) => point.sessions)}
            />
          </div>

          <Card
            className="scard--wide"
            title={t(summary.granularity === "week" ? "stats.weeklyTitle" : "stats.dailyTitle")}
            hint={t("stats.dailyHint", { measure: measureName })}
            actions={
              <>
                <Legend t={t} />
                <TableToggle table={dailyTable} onToggle={() => setDailyTable((on) => !on)} />
              </>
            }
          >
            {dailyTable ? (
              <StatsColumnsTable series={series} granularity={summary.granularity} measure={measure} />
            ) : (
              <StatsColumns
                series={series}
                granularity={summary.granularity}
                measure={measure}
                label={`${t(summary.granularity === "week" ? "stats.weeklyTitle" : "stats.dailyTitle")} — ${t("stats.dailyHint", { measure: measureName })}`}
                empty={
                  <>
                    <strong>{t("stats.noData")}</strong>
                    <span>{t("stats.noDataHint")}</span>
                  </>
                }
              />
            )}
          </Card>

          {/* Two columns that stack on their own, so a long ranking never
              leaves a hole beside a short card. */}
          <div className="stats__grid">
            <div className="stats__col">
              <Card title={t("stats.projectsTitle")} hint={t("stats.projectsHint")}>
                {summary.projects.length ? (
                  <>
                    <RankedList
                      rows={visibleProjects}
                      measure={measure}
                      selected={project}
                      onPick={(key) => setProject(project === key ? null : key)}
                      pickLabel={(row) => (project === row.key ? t("stats.clearProject") : t("stats.showOnly", { name: row.label }))}
                      sub={(row) => `${t("stats.sessions", { n: row.sessions })} · ${formatDuration(row.activeMs, t)}`}
                    />
                    {(hiddenProjects > 0 || allProjects) && summary.projects.length > PROJECTS_SHOWN + 1 && (
                      <button type="button" className="scard__more" onClick={() => setAllProjects((on) => !on)}>
                        {allProjects ? t("stats.fewerProjects") : t("stats.moreProjects", { n: hiddenProjects })}
                      </button>
                    )}
                  </>
                ) : (
                  <p className="scard__empty">{t("stats.noData")}</p>
                )}
              </Card>

              <Card title={t("stats.agentsTitle")} className="scard--split">
                {summary.agents.length ? (
                  <RankedList
                    rows={summary.agents}
                    measure={measure}
                    sub={(row) => `${t("stats.sessions", { n: row.sessions })} · ${formatDuration(row.activeMs, t)}`}
                  />
                ) : (
                  <p className="scard__empty">{t("stats.noData")}</p>
                )}
                <h3 className="scard__title scard__title--sub">{t("stats.modelsTitle")}</h3>
                {summary.models.length ? (
                  <RankedList rows={summary.models} measure={measure} />
                ) : (
                  <p className="scard__empty">{t("stats.noData")}</p>
                )}
                {summary.unpricedTokens > 0 && (
                  <p className="scard__note">
                    {t("stats.unpricedNote", { tokens: formatCompact(summary.unpricedTokens, locale) })}
                  </p>
                )}
              </Card>

            </div>
            <div className="stats__col">
              <Card
                title={t("stats.heatTitle")}
                hint={t("stats.heatHint")}
                actions={<TableToggle table={heatTable} onToggle={() => setHeatTable((on) => !on)} />}
              >
                {heatTable ? (
                  <StatsHeatTable heat={summary.heat} />
                ) : (
                  <StatsHeatmap heat={summary.heat} max={summary.heatMax} label={`${t("stats.heatTitle")} — ${t("stats.heatHint")}`} />
                )}
              </Card>

              <Card title={t("stats.sessionsTitle")} hint={t("stats.sessionsHint", { measure: measureName.toLowerCase() })}>
                <SessionList rows={summary.sessions} measure={measure} agentLabel={agentLabel} onProject={setProject} />
              </Card>
            </div>
          </div>

          <footer className="stats__method">
            <h3 className="stats__method-title">
              <Icon name="info" size={13} />
              {t("stats.methodTitle")}
            </h3>
            <p>{t("stats.methodSources")}</p>
            <p>{t("stats.methodActive", { cap: formatDuration(data.activeGapCapMs, t) })}</p>
            <p>{t("stats.methodCost")}</p>
            {!data.scan.cachedOnly && (
              <p className="stats__method-scan">
                {t("stats.methodScan", { files: data.scan.files, read: data.scan.parsed, ms: data.scan.ms })}
              </p>
            )}
          </footer>
        </div>
      </div>
    </section>
  );
}
