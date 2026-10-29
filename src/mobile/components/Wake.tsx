import type { Project } from "../api";
import { errorText } from "../errors";
import { t } from "../i18n";
import { useRemoteContext } from "../remote";
import { wakeOf } from "../wake";
import { Icon } from "./Icon";

/**
 * A project asleep on the PC: why its agents do not answer, and the one
 * action that brings them back. « Réveil en cours… » from the tap until the
 * PC shows it awake, then nothing: its panes work as usual.
 *
 * `compact`: the foot of its card in the Projects list, where its name and
 * « En veille » already say why.
 */
export function WakeUp({ project, compact = false }: { project: Project; compact?: boolean }) {
  const { wakes, wake } = useRemoteContext();
  const attempt = wakes[project.id];
  // Read at each render: the state, polled every 2 s, renders it again.
  const status = wakeOf(project, attempt, Date.now());
  if (status.state === "awake") return null;

  const variant = compact ? "asleep asleep--compact" : "asleep";
  if (status.state === "waking") {
    return (
      <div className={`${variant} asleep--waking`} role="status">
        <span className="spinner" aria-hidden="true" />
        <div className="asleep__body">
          <p className="asleep__title">{t("wake.waking")}</p>
          {!compact && <p>{t("wake.wakingHint")}</p>}
        </div>
      </div>
    );
  }

  const failure =
    status.failed === "call" ? errorText(attempt?.error) : status.failed === "timeout" ? t("wake.timeout") : null;
  return (
    <div className={variant}>
      {!compact && <Icon name="moon" />}
      <div className="asleep__body">
        {!compact && <p>{t("project.asleepHint")}</p>}
        {failure && (
          <p className="asleep__error" role="alert">
            {failure}
          </p>
        )}
        <button
          type="button"
          className="btn btn--primary asleep__wake"
          onClick={() => void wake(project.id)}
          aria-label={t("wake.label", { name: project.name })}
        >
          {t("wake.button")}
        </button>
      </div>
    </div>
  );
}
