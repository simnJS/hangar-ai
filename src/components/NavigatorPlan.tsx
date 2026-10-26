import { useT, useLocale } from "../i18n";
import { usePlanLimits } from "../lib/agentState";

/** Past these, a window is close enough to its limit to say so in colour. */
const WARN_AT = 70;
const HOT_AT = 90;

/**
 * The plan's rate-limit windows, as the last Claude Code session to report
 * saw them — one account behind every pane, so one block for all of them.
 * Nothing shows until a pane with the hangar-bridge mod has reported.
 */
export function NavigatorPlan() {
  const plan = usePlanLimits();
  const t = useT();
  const locale = useLocale();
  if (!plan) return null;

  const label = (kind: string) =>
    kind === "five_hour"
      ? t("sidebar.planFiveHour")
      : kind === "seven_day"
        ? t("sidebar.planWeek")
        : kind.replace(/_/g, " ");
  const percent = new Intl.NumberFormat(locale, { style: "percent", maximumFractionDigits: 0 });
  const time = new Intl.DateTimeFormat(locale, { hour: "2-digit", minute: "2-digit" });
  const dayAndTime = new Intl.DateTimeFormat(locale, {
    weekday: "short",
    hour: "2-digit",
    minute: "2-digit",
  });

  const resets = plan.limits
    .filter((limit) => limit.resetsAt)
    .map((limit) =>
      t("sidebar.planResets", {
        window: label(limit.kind),
        time: new Date(limit.resetsAt as string).toLocaleString(locale),
      }),
    );
  const title = [t("sidebar.planHint"), ...resets].join("\n");

  // The heading names the next window to reset: the one that frees up first,
  // which is what a pane about to hit its limit is waiting for.
  const now = Date.now();
  const next = plan.limits
    .map((limit) => (limit.resetsAt ? new Date(limit.resetsAt) : null))
    .filter((date): date is Date => date !== null && date.getTime() > now)
    .sort((a, b) => a.getTime() - b.getTime())[0];
  const nextLabel = next
    ? (next.toDateString() === new Date(now).toDateString() ? time : dayAndTime).format(next)
    : null;

  return (
    <div className="nav-plan" title={title}>
      <div className="nav-plan__head">
        <span className="nav-plan__label">{t("sidebar.plan")}</span>
        {nextLabel && (
          <span className="nav-plan__reset">{t("sidebar.planResetAt", { time: nextLabel })}</span>
        )}
      </div>
      {plan.limits.map((limit) => {
        const pct = Math.max(0, Math.min(100, Math.round(limit.percentUsed)));
        const level = pct >= HOT_AT ? "hot" : pct >= WARN_AT ? "warn" : "ok";
        const value = percent.format(pct / 100);
        return (
          <div
            key={limit.kind}
            className={`nav-meter nav-meter--${level}`}
            role="meter"
            aria-label={label(limit.kind)}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={pct}
            aria-valuetext={value}
          >
            <span className="nav-meter__label">{label(limit.kind)}</span>
            <span className="nav-meter__track">
              <span className="nav-meter__fill" style={{ width: `${pct}%` }} />
            </span>
            <span className="nav-meter__value">{value}</span>
          </div>
        );
      })}
    </div>
  );
}
