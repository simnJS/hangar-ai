import type { Project } from "../api";
import { AgentCounts, BackBar, EmptyState, Link, PcChip, StateLabel, useNavigation } from "../components/Chrome";
import { Icon, Logo } from "../components/Icon";
import { WakeUp } from "../components/Wake";
import { t, tn } from "../i18n";
import { fleetOf, orchestratorPane, workersOf } from "../model";
import { useRemoteContext } from "../remote";

/** Projects: each one's needs, agents, board and plan at a glance. */
export function Projects() {
  const { state, pc, connection } = useRemoteContext();
  if (!state) return null;
  return (
    <div className="page">
      <div className="top">
        <h1 className="title">{t("projects.title")}</h1>
        <PcChip pc={pc} connection={connection} />
      </div>
      {state.projects.length === 0 ? (
        <EmptyState title={t("projects.empty")} text={t("projects.emptyHint")} mark={<Logo size={40} />} />
      ) : (
        <ul className="stack section">
          {state.projects.map((project) => (
            // Asleep, the card leads to the project and its foot wakes it: a
            // button cannot sit inside the link.
            <li key={project.id} className={project.asleep ? "project project--asleep" : undefined}>
              <Link
                to={{ name: "project", id: project.id }}
                className={project.asleep ? "project__link" : "project"}
                label={t("project.open", { name: project.name })}
              >
                <div className="project__head">
                  <span className="project__name">{project.name}</span>
                  <ProjectTag project={project} />
                  <Icon name="chevronRight" className="chevron" />
                </div>
                <Attention project={project} />
                <AgentCounts counts={fleetOf([project]).counts} />
                {/* An empty board says nothing in the list; the project's own screen shows it. */}
                {Object.values(project.board).some(Boolean) && <Board project={project} />}
                {project.planStatus && (
                  <p className="project__plan">
                    <b>{t("project.plan")}</b> {project.planStatus}
                  </p>
                )}
              </Link>
              {project.asleep && <WakeUp project={project} compact />}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function ProjectTag({ project }: { project: Project }) {
  if (project.asleep) {
    return (
      <span className="tag">
        <Icon name="moon" />
        {t("project.asleep")}
      </span>
    );
  }
  if (!project.open) return <span className="tag">{t("project.closed")}</span>;
  return null;
}

function Team({ project }: { project: Project }) {
  const workers = workersOf(project).length;
  const lead = project.orchestrator ? t("project.orchestrator", { name: project.orchestrator.name }) : t("project.noOrchestrator");
  return <p className="project__team">{workers > 0 ? `${lead}, ${tn("project.workers", workers)}` : lead}</p>;
}

function Attention({ project }: { project: Project }) {
  const parts = [
    project.needsYou.length ? tn("project.questions", project.needsYou.length) : null,
    project.readyToLand.length ? tn("project.lands", project.readyToLand.length) : null,
  ].filter(Boolean);
  if (!parts.length) return null;
  return (
    <p className="project__attention">
      <Icon name="flag" size={16} />
      {parts.join(", ")}
    </p>
  );
}

const COLUMNS = ["todo", "doing", "review", "done"] as const;

function Board({ project }: { project: Project }) {
  return (
    <dl className="board">
      {COLUMNS.map((column) => (
        <div key={column} className={project.board[column] ? "" : "is-zero"}>
          <dt>{t(`board.${column}`)}</dt>
          <dd>{project.board[column]}</dd>
        </div>
      ))}
    </dl>
  );
}

/** One project: what it asks of the person, its board, and its panes. */
export function ProjectScreen({ id }: { id: string }) {
  const { state } = useRemoteContext();
  const { navigate } = useNavigation();
  const project = state?.projects.find((candidate) => candidate.id === id);
  if (!state) return null;
  if (!project) {
    return (
      <div className="page">
        <BackBar to={{ name: "projects" }} label={t("projects.title")} />
        <EmptyState title={t("projects.empty")} text={t("projects.emptyHint")} />
      </div>
    );
  }
  const orchestrator = orchestratorPane(project);
  const answer = orchestrator ? () => navigate({ name: "pane", id: orchestrator.id }) : undefined;

  return (
    <div className="page">
      <BackBar to={{ name: "projects" }} label={t("projects.title")} />
      <div className="project__head">
        <h1 className="title">{project.name}</h1>
        <ProjectTag project={project} />
      </div>
      <Team project={project} />

      <WakeUp project={project} />

      {project.needsYou.length > 0 && (
        <section className="section">
          <div className="section__head">
            <h2 className="section__title">{t("forYou.questions")}</h2>
            <span className="section__count">{project.needsYou.length}</span>
          </div>
          <ul className="stack">
            {project.needsYou.map((task) => (
              <li key={task.id} className="card is-waiting" onClick={answer}>
                <p className="card__title">{task.title}</p>
                {answer && (
                  <div className="card__foot">
                    <button type="button" className="btn btn--primary" onClick={(event) => (event.stopPropagation(), answer())}>
                      {t("forYou.answer")}
                    </button>
                  </div>
                )}
              </li>
            ))}
          </ul>
        </section>
      )}

      {project.readyToLand.length > 0 && (
        <section className="section">
          <div className="section__head">
            <h2 className="section__title">{t("forYou.lands")}</h2>
            <span className="section__count">{project.readyToLand.length}</span>
          </div>
          <p className="section__hint">{t("forYou.landOnPc")}</p>
          <ul className="stack">
            {project.readyToLand.map((task) => (
              <li key={task.id} className="card is-yours">
                <p className="card__title">{task.title}</p>
              </li>
            ))}
          </ul>
        </section>
      )}

      <section className="section">
        <div className="section__head">
          <h2 className="section__title">{t("project.panes")}</h2>
          <span className="section__count">{project.panes.length}</span>
        </div>
        {project.panes.length === 0 ? (
          <p className="section__hint">{t("project.noPanes")}</p>
        ) : (
          <ul className="list">
            {project.panes.map((pane) => (
              <li key={pane.id}>
                <Link to={{ name: "pane", id: pane.id }} className="row">
                  <span className="row__main">
                    <span className="row__title">
                      {pane.name}
                      {pane.role && pane.role in ROLE && <span className="role">{t(ROLE[pane.role as keyof typeof ROLE])}</span>}
                    </span>
                    <span className="row__sub">
                      <StateLabel activity={pane.activity} />
                    </span>
                  </span>
                  <Icon name="chevronRight" className="chevron" />
                </Link>
              </li>
            ))}
          </ul>
        )}
      </section>
      <section className="section">
        <div className="section__head">
          <h2 className="section__title">{t("project.board")}</h2>
        </div>
        <div className="panel">
          <Board project={project} />
          {project.planStatus && (
            <p className="plan">
              <b>{t("project.plan")}</b>
              {project.planStatus}
            </p>
          )}
        </div>
      </section>

    </div>
  );
}

const ROLE = {
  orchestrator: "role.orchestrator",
  worker: "role.worker",
  manager: "role.manager",
} as const;
