import { Icon } from "./Icon";
import { useT } from "../i18n";
import { useBoardTasks } from "../lib/paneTask";
import type { Pane } from "../types";

/**
 * What a pane is in its workspace's orchestration, in its header, right
 * after its name: « Orchestrateur », or « Worker » and the title of the task
 * it is on, or « Chef de projet » on the HQ's manager. Nothing at all on
 * every other pane.
 *
 * A worker's task is read from the workspace's board, the one its worktree
 * shares (lib/paneTask), so a title edited on the board shows here too.
 */
export function OrchestrationBadge({ pane, cwd }: { pane: Pane; cwd: string }) {
  if (pane.role === "orchestrator") return <OrchestratorBadge />;
  if (pane.role === "manager") return <ManagerBadge />;
  if (pane.role === "worker") return <WorkerBadge taskId={pane.taskId ?? null} cwd={cwd} />;
  return null;
}

function OrchestratorBadge() {
  const t = useT();
  return (
    <span
      className="orch-badge orch-badge--lead"
      title={t("orch.badgeOrchestratorHint")}
    >
      <Icon name="orchestrator" size={11} />
      <span className="orch-badge__role">{t("orch.badgeOrchestrator")}</span>
    </span>
  );
}

function ManagerBadge() {
  const t = useT();
  return (
    <span className="orch-badge orch-badge--lead" title={t("orch.badgeManagerHint")}>
      <Icon name="orchestrator" size={11} />
      <span className="orch-badge__role">{t("orch.badgeManager")}</span>
    </span>
  );
}

function WorkerBadge({ taskId, cwd }: { taskId: string | null; cwd: string }) {
  const t = useT();
  const tasks = useBoardTasks(taskId ? cwd : null);
  const task = taskId ? (tasks?.find((entry) => entry.id === taskId) ?? null) : null;
  const hint = task ? t("orch.badgeWorkerHint", { title: task.title }) : t("orch.badgeWorkerNoTask");
  return (
    <span className="orch-badge orch-badge--worker" title={hint}>
      <span className="orch-badge__role">{t("orch.badgeWorker")}</span>
      {task && (
        <>
          <span className="orch-badge__sep" aria-hidden="true">
            ·
          </span>
          <span className="orch-badge__task">{task.title}</span>
        </>
      )}
    </span>
  );
}
