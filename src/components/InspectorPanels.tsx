import { memo, useEffect, useState } from "react";
import { BoardPriority } from "./BoardPriority";
import { Icon } from "./Icon";
import { InspectorSection } from "./InspectorSection";
import { StateGlyph } from "./StateGlyph";
import { useLocale, useT } from "../i18n";
import { formatTokens, type ContextGauge } from "../lib/context";
import { bridgeCancel, type QueuedPrompt } from "../lib/ipc";
import type { RateLimit } from "../lib/bridge";
import type { Task } from "../lib/board";
import { paneEventText, queuedBy, type PaneEvent, type PaneMcp } from "../lib/paneInsight";
import { elapsedLabel, shortTaskId } from "../lib/boardFormat";
import {
  clockTime,
  formatCost,
  formatPercent,
  formatSince,
  planUsage,
  resetWhen,
  spacedUnit,
  usageTone,
} from "../lib/inspectorFormat";

/**
 * The inspector's sections, one component each and each memoised on exactly
 * what it shows: the pane's insight is a store that changes on every bridge
 * event, and a new context reading has no business re-rendering the queue.
 */

/**
 * Ticks every half minute — so "1 h 12" is never a minute behind — and only
 * while something on screen counts time.
 */
function useMinuteClock(running: boolean): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!running) return;
    setNow(Date.now());
    const timer = window.setInterval(() => setNow(Date.now()), 30_000);
    return () => window.clearInterval(timer);
  }, [running]);
  return now;
}

interface Foldable {
  collapsed: boolean;
  onToggle: (id: string) => void;
}

/** A label/value row of the key-value grids. */
function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <>
      <dt>{label}</dt>
      <dd>{children}</dd>
    </>
  );
}

/**
 * A path in a cell too narrow for it loses its beginning, not its end: the
 * last folders are the ones that tell two checkouts apart. The marks keep
 * the separators where they belong inside the right-to-left box.
 */
function TailPath({ path, className = "" }: { path: string; className?: string }) {
  return (
    <span className={`insp-tail ${className}`} title={path}>
      {`\u200e${path}\u200e`}
    </span>
  );
}

export const SessionSection = memo(function SessionSection({
  paneId,
  sessionId,
  resumable,
  since,
  model,
  folder,
  onOpenSessions,
  mcp,
  onOpenMcpServers,
  collapsed,
  onToggle,
}: Foldable & {
  paneId: string;
  sessionId: string | null;
  resumable: boolean;
  since: number | null;
  model: string | null;
  folder: string;
  onOpenSessions: (paneId: string) => void;
  /** The MCP servers its Claude Code started with; null for other agents. */
  mcp: PaneMcp | null;
  onOpenMcpServers?: () => void;
}) {
  const t = useT();
  const locale = useLocale();
  const now = useMinuteClock(!collapsed && since !== null);
  return (
    <InspectorSection
      id="session"
      title={t("inspector.section.session")}
      collapsed={collapsed}
      onToggle={onToggle}
    >
      <dl className="insp-kv">
        {resumable && (
          <Row label={t("inspector.resume")}>
            <span className="insp-kv__split">
              {sessionId ? (
                <span className="insp-mono insp-kv__clip" title={sessionId}>
                  {sessionId}
                </span>
              ) : (
                <span className="insp-kv__faint insp-kv__clip">{t("inspector.resumeNew")}</span>
              )}
              <button
                type="button"
                className="insp-link"
                title={t("inspector.resumeChangeHint")}
                onClick={() => onOpenSessions(paneId)}
              >
                {t("inspector.resumeChange")}
              </button>
            </span>
          </Row>
        )}
        {since !== null && (
          <Row label={t("inspector.since")}>
            <span title={new Date(since).toLocaleString(locale)}>
              {formatSince(since, now, t, locale)}
            </span>
          </Row>
        )}
        {model && (
          <Row label={t("inspector.model")}>
            <span className="insp-mono" title={model}>
              {model}
            </span>
          </Row>
        )}
        {mcp && (
          <Row label={t("inspector.mcp")}>
            <span className="insp-kv__split">
              {mcp.mode === "custom" ? (
                <span className="insp-mono insp-kv__clip" title={mcp.servers.join(", ")}>
                  {mcp.servers.join(", ")}
                </span>
              ) : (
                <span className="insp-kv__faint insp-kv__clip">{t("inspector.mcpInherit")}</span>
              )}
              {onOpenMcpServers && (
                <button
                  type="button"
                  className="insp-link"
                  title={t("inspector.mcpChangeHint")}
                  onClick={onOpenMcpServers}
                >
                  {t("inspector.mcpChange")}
                </button>
              )}
            </span>
          </Row>
        )}
        <Row label={t("inspector.folder")}>
          <TailPath path={folder} className="insp-mono" />
        </Row>
      </dl>
    </InspectorSection>
  );
});

export const TerminalSection = memo(function TerminalSection({
  shell,
  folder,
  collapsed,
  onToggle,
}: Foldable & { shell: string | null; folder: string }) {
  const t = useT();
  return (
    <InspectorSection
      id="terminal"
      title={t("inspector.section.terminal")}
      collapsed={collapsed}
      onToggle={onToggle}
    >
      <dl className="insp-kv">
        {shell && <Row label={t("inspector.shell")}>{shell}</Row>}
        <Row label={t("inspector.folder")}>
          <TailPath path={folder} className="insp-mono" />
        </Row>
      </dl>
    </InspectorSection>
  );
});

/** The context gauge, as a ring: neutral, amber from 70 %, red from 90 %. */
function Ring({ pct, label }: { pct: number; label: string }) {
  const radius = 27;
  const length = 2 * Math.PI * radius;
  return (
    <div className={`insp-ring insp-ring--${usageTone(pct)}`} role="img" aria-label={label}>
      <svg viewBox="0 0 64 64" aria-hidden="true">
        <circle className="insp-ring__track" cx="32" cy="32" r={radius} />
        <circle
          className="insp-ring__value"
          cx="32"
          cy="32"
          r={radius}
          strokeDasharray={`${((length * pct) / 100).toFixed(2)} ${length.toFixed(2)}`}
        />
      </svg>
      <span className="insp-ring__label" aria-hidden="true">
        <span>
          {pct}
          <small>%</small>
        </span>
      </span>
    </div>
  );
}

function PlanMeter({
  label,
  window: name,
  limit,
}: {
  label: string;
  window: string;
  limit: RateLimit | null;
}) {
  const t = useT();
  const locale = useLocale();
  if (!limit) return null;
  const pct = limit.percentUsed;
  const resets = limit.resetsAt ? new Date(limit.resetsAt).getTime() : NaN;
  const hint = !Number.isNaN(resets)
    ? t("inspector.planHintResets", {
        window: name,
        pct,
        reset: resetWhen(resets, Date.now(), t, locale),
      })
    : t("inspector.planHint", { window: name, pct });
  return (
    <div className={`insp-meter insp-meter--${usageTone(pct)}`} title={hint}>
      <span>{label}</span>
      <span className="insp-meter__bar">
        <i style={{ width: `${pct}%` }} />
      </span>
      <span className="insp-meter__value">{formatPercent(pct, locale)}</span>
    </div>
  );
}

export const ContextSection = memo(function ContextSection({
  agentLabel,
  showPlan,
  gauge,
  costUsd,
  since,
  limits,
  collapsed,
  onToggle,
}: Foldable & {
  agentLabel: string;
  /** The plan is Claude's: other agents never show its meters. */
  showPlan: boolean;
  gauge: ContextGauge | null;
  costUsd: number | null;
  since: number | null;
  limits: RateLimit[] | null;
}) {
  const t = useT();
  const locale = useLocale();
  const now = useMinuteClock(!collapsed && costUsd !== null && since !== null);
  const fiveHour = showPlan ? planUsage(limits, "five_hour") : null;
  const week = showPlan ? planUsage(limits, "seven_day") : null;
  const hasPlan = Boolean(fiveHour || week);
  // Cost and plan get their tiles once either is known; an agent that
  // reports neither gets no row of dashes.
  const stats = costUsd !== null || hasPlan;

  return (
    <InspectorSection
      id="context"
      title={t("inspector.section.context")}
      collapsed={collapsed}
      onToggle={onToggle}
    >
      {gauge ? (
        <div className="insp-ctx">
          <Ring pct={gauge.pct} label={t("inspector.contextRing", { pct: gauge.pct })} />
          <div className="insp-ctx__info">
            <b>{t("inspector.contextUsed", { pct: gauge.pct })}</b>
            <span className="insp-num">
              {t("inspector.contextTokens", {
                used: spacedUnit(formatTokens(gauge.usedTokens), locale),
                window: spacedUnit(formatTokens(gauge.window), locale),
              })}
            </span>
            <span className="insp-ctx__hint">
              {t(gauge.pct >= 90 ? "inspector.contextHintSoon" : "inspector.contextHint")}
            </span>
          </div>
        </div>
      ) : (
        <p className="insp-empty">{t("inspector.contextEmpty")}</p>
      )}
      {stats && (
        <div className="insp-stats">
          <div className="insp-stat">
            <span className="insp-stat__label">{t("inspector.cost")}</span>
            {costUsd !== null ? (
              <b className="insp-stat__value insp-num">
                {formatCost(costUsd, locale)}
                {since !== null && (
                  <small>
                    {t("inspector.costOver", { duration: elapsedLabel(now - since, t) })}
                  </small>
                )}
              </b>
            ) : (
              <b
                className="insp-stat__value insp-stat__value--none"
                title={t("inspector.costUnknown", { agent: agentLabel })}
              >
                —
              </b>
            )}
          </div>
          {showPlan && (
            <div className="insp-stat">
              <span className="insp-stat__label">{t("inspector.plan")}</span>
              {hasPlan ? (
                <>
                  <PlanMeter
                    label={t("inspector.planFiveHour")}
                    window={t("inspector.planWindowFiveHour")}
                    limit={fiveHour}
                  />
                  <PlanMeter
                    label={t("inspector.planWeek")}
                    window={t("inspector.planWindowWeek")}
                    limit={week}
                  />
                </>
              ) : (
                <b
                  className="insp-stat__value insp-stat__value--none"
                  title={t("inspector.planUnknown")}
                >
                  —
                </b>
              )}
            </div>
          )}
        </div>
      )}
    </InspectorSection>
  );
});

export const TaskSection = memo(function TaskSection({
  task,
  onOpenBoard,
  collapsed,
  onToggle,
}: Foldable & { task: Task | null; onOpenBoard: () => void }) {
  const t = useT();
  const locale = useLocale();
  const since = task?.column === "doing" ? (task.doing_since ?? null) : null;
  const now = useMinuteClock(!collapsed && since !== null);
  const open = (
    <button
      type="button"
      className="insp-link"
      title={t("inspector.taskOpenHint")}
      onClick={onOpenBoard}
    >
      {t("inspector.taskOpen")}
      <Icon name="arrowUpRight" size={10} />
    </button>
  );

  return (
    <InspectorSection
      id="task"
      title={t("inspector.section.task")}
      action={task ? open : null}
      collapsed={collapsed}
      onToggle={onToggle}
    >
      {task ? (
        // The whole card answers a click; the "Open" button beside the title
        // is the same action for the keyboard, so the card is not a second
        // tab stop.
        <div className="insp-task" onClick={onOpenBoard}>
          <div className="insp-task__top">
            <span className="insp-mono" title={t("inspector.taskId", { id: task.id })}>
              #{shortTaskId(task.id)}
            </span>
            <BoardPriority priority={task.priority} />
          </div>
          <p className="insp-task__title">{task.title}</p>
          <div className="insp-task__foot">
            <span className="insp-task__state">
              <StateGlyph state={task.column === "review" ? "yours" : "working"} size={7} />
              {task.column === "review" ? (
                t("inspector.taskReview")
              ) : since !== null ? (
                <span
                  title={t("board.doingSince", { date: new Date(since).toLocaleString(locale) })}
                >
                  {t("inspector.taskDoing", { duration: elapsedLabel(now - since, t) })}
                </span>
              ) : (
                t("inspector.taskDoingNow")
              )}
            </span>
            {task.comments.length > 0 && (
              <span
                className="insp-task__comments"
                title={t("inspector.taskComments", { n: task.comments.length })}
              >
                <Icon name="bubble" size={11} />
                {task.comments.length}
              </span>
            )}
          </div>
        </div>
      ) : (
        <p className="insp-empty">{t("inspector.taskNone")}</p>
      )}
    </InspectorSection>
  );
});

/** Who queued a prompt the person did not: the session reads it labelled. */
function QueuedFrom({ from }: { from?: string }) {
  const t = useT();
  const by = queuedBy(from);
  if (!by) return null;
  return (
    <span className="insp-queue__from" title={t("inspector.queueFrom", { name: by })}>
      {by}
    </span>
  );
}

export const QueueSection = memo(function QueueSection({
  paneId,
  queue,
  collapsed,
  onToggle,
}: Foldable & { paneId: string; queue: QueuedPrompt[] }) {
  const t = useT();
  return (
    <InspectorSection
      id="queue"
      title={t("inspector.section.queue")}
      meta={queue.length > 0 ? <span className="insp-num">{queue.length}</span> : null}
      collapsed={collapsed}
      onToggle={onToggle}
    >
      {queue.length ? (
        <ol className="insp-queue">
          {queue.map((entry, i) => (
            <li key={entry.id} className="insp-queue__item" title={entry.text}>
              <span className="insp-queue__n insp-num" aria-hidden="true">
                {i + 1}
              </span>
              <span className="insp-queue__text">{entry.text}</span>
              <QueuedFrom from={entry.from} />
              <button
                type="button"
                className="insp-icon-btn"
                aria-label={t("inspector.queueCancel")}
                title={t("inspector.queueCancel")}
                // The queue lives in the bridge; the pane hears the change
                // and the list follows, so there is nothing to undo here.
                onClick={() => void bridgeCancel(paneId, entry.id).catch(() => undefined)}
              >
                <Icon name="close" size={10} />
              </button>
            </li>
          ))}
        </ol>
      ) : (
        <p className="insp-empty">{t("inspector.queueEmpty")}</p>
      )}
    </InspectorSection>
  );
});

/** How many events the section keeps: a glance back, not a log. */
const ACTIVITY_SHOWN = 8;

export const ActivitySection = memo(function ActivitySection({
  events,
  empty,
  collapsed,
  onToggle,
}: Foldable & { events: PaneEvent[]; empty: string }) {
  const t = useT();
  const locale = useLocale();
  const shown = events.slice(0, ACTIVITY_SHOWN);
  return (
    <InspectorSection
      id="activity"
      title={t("inspector.section.activity")}
      collapsed={collapsed}
      onToggle={onToggle}
    >
      {shown.length ? (
        <ol className="insp-activity">
          {shown.map((event, i) => (
            <li key={`${event.at}-${i}`} className="insp-event">
              <time
                dateTime={new Date(event.at).toISOString()}
                title={new Date(event.at).toLocaleString(locale)}
              >
                {clockTime(event.at, locale)}
              </time>
              <i className={`insp-event__dot insp-event__dot--${event.kind}`} aria-hidden="true" />
              <span className="insp-event__text">{paneEventText(event, t)}</span>
            </li>
          ))}
        </ol>
      ) : (
        <p className="insp-empty insp-empty--note">{empty}</p>
      )}
    </InspectorSection>
  );
});
