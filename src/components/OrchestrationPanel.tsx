import { Fragment, useId, useMemo, useState, type KeyboardEvent, type ReactNode } from "react";
import { Backdrop, ModalHead } from "./Backdrop";
import { Icon } from "./Icon";
import { useT } from "../i18n";
import { useStore } from "../store";
import { ORCHESTRATION_DEFAULTS, orchestrationOf, orchestratorOf, workersOf } from "../lib/orchestration";
import {
  draftOf,
  draftProblems,
  MODEL_SUGGESTIONS,
  normalizeDraft,
  ORCHESTRATION_LIMITS,
  sameSettings,
  storedOrchestration,
  type OrchestrationDraft,
} from "../lib/orchestrationForm";
import type { Workspace } from "../types";

interface Props {
  workspace: Workspace;
  /** Starts a Claude Code pane pinned as the orchestrator. */
  onCreateOrchestrator: () => void;
  onSaved: () => void;
  onClose: () => void;
}

type CountField = "maxWorkers" | "maxAttempts";
type ModelField = "orchestratorModel" | "workerModel" | "reviewerModel";

/**
 * « Orchestration »: how a workspace's orchestrator works — how many workers
 * at once, how many tries a task gets, whether a delivery lands by itself,
 * what checks it, where it lands, and on which models (lib/orchestration).
 *
 * A form with a draft rather than switches saved as they flip, like the MCP
 * servers' panel: half of it is typed, and a check command saved one letter
 * at a time would be run as whatever it said when a delivery came in. What
 * the panel shows is what the workspace runs with, defaults filled in, each
 * default named beside its field.
 */
export function OrchestrationPanel({ workspace, onCreateOrchestrator, onSaved, onClose }: Props) {
  const t = useT();
  const { updateWorkspace } = useStore();
  const [draft, setDraft] = useState<OrchestrationDraft>(() => draftOf(workspace));
  const uid = useId();
  const id = (field: string) => `${uid}-${field}`;

  const orchestrator = orchestratorOf(workspace);
  const workers = workersOf(workspace);
  const saved = orchestrationOf(workspace);
  const problems = draftProblems(draft);
  const next = useMemo(() => normalizeDraft(draft), [draft]);
  const blocked = Object.keys(problems).length > 0;
  const changed = !sameSettings(next, saved);
  const atDefaults = sameSettings(next, ORCHESTRATION_DEFAULTS);

  const set = <K extends keyof OrchestrationDraft>(field: K, value: OrchestrationDraft[K]) =>
    setDraft((current) => ({ ...current, [field]: value }));

  function save() {
    if (blocked || !changed) return;
    updateWorkspace(workspace.id, { orchestration: storedOrchestration(next) });
    onSaved();
    onClose();
  }

  /** Enter in a text field saves, as in the other dialogs. */
  const onFieldKey = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Enter") save();
  };

  const fallback = (value: string | number | null) =>
    t("orch.default", { value: value === null ? "—" : String(value) });

  function stepper(field: CountField, label: string, hint: string) {
    const { min, max } = ORCHESTRATION_LIMITS[field];
    const value = draft[field];
    return (
      <>
        <span className="form-grid__label" id={id(field)}>
          {label}
        </span>
        <div className="form-grid__field">
          <div className="form-grid__field--inline">
            <span className="stepper" role="group" aria-labelledby={id(field)}>
              <button
                type="button"
                className="stepper__btn"
                aria-label={t("orch.fewer", { label })}
                title={t("orch.fewer", { label })}
                onClick={() => set(field, Math.max(min, value - 1))}
                disabled={value <= min}
              >
                <Icon name="minus" size={12} />
              </button>
              <span className="stepper__value" aria-live="polite">
                {value}
              </span>
              <button
                type="button"
                className="stepper__btn"
                aria-label={t("orch.more", { label })}
                title={t("orch.more", { label })}
                onClick={() => set(field, Math.min(max, value + 1))}
                disabled={value >= max}
              >
                <Icon name="plus" size={12} />
              </button>
            </span>
            <span className="form-hint">{fallback(ORCHESTRATION_DEFAULTS[field])}</span>
          </div>
          <span className="form-hint">{hint}</span>
        </div>
      </>
    );
  }

  function textField(
    field: "checkCommand" | "branch" | ModelField,
    label: string,
    options: { placeholder: string; hint?: string; error?: string; models?: boolean },
  ) {
    const problem = problems[field] ? options.error : undefined;
    const described = [problem && id(`${field}-error`), options.hint && id(`${field}-hint`)]
      .filter(Boolean)
      .join(" ");
    const input = (
      <input
        id={id(field)}
        className={`text-input text-input--mono ${problem ? "is-invalid" : ""}`}
        value={draft[field]}
        placeholder={options.placeholder}
        spellCheck={false}
        autoComplete="off"
        list={options.models ? id("models") : undefined}
        aria-invalid={problem ? true : undefined}
        aria-describedby={described || undefined}
        onChange={(event) => set(field, event.target.value)}
        onKeyDown={onFieldKey}
      />
    );
    const hint = options.hint && (
      <span className="form-hint" id={id(`${field}-hint`)}>
        {options.hint}
      </span>
    );
    const error = problem && (
      <span className="form-hint orch-panel__error" id={id(`${field}-error`)} role="alert">
        <Icon name="alert" size={12} />
        {problem}
      </span>
    );
    return (
      <>
        <label className="form-grid__label" htmlFor={id(field)}>
          {label}
        </label>
        {/* A model is a word: its field is short, its default beside it. */}
        {options.models ? (
          <div className="form-grid__field">
            <div className="form-grid__field--inline orch-panel__model">
              {input}
              {hint}
            </div>
            {error}
          </div>
        ) : (
          <div className="form-grid__field">
            {input}
            {error}
            {hint}
          </div>
        )}
      </>
    );
  }

  const section = (title: string, children: ReactNode) => (
    <section className="orch-panel__section" aria-labelledby={id(title)}>
      <h3 className="orch-panel__heading" id={id(title)}>
        {title}
      </h3>
      <div className="form-grid form-grid--flush orch-panel__grid">{children}</div>
    </section>
  );

  return (
    <Backdrop onClose={onClose}>
      <div
        className="modal modal--wide modal--orch"
        role="dialog"
        aria-modal="true"
        aria-labelledby={id("title")}
      >
        <ModalHead
          id={id("title")}
          title={t("orch.title")}
          sub={`${workspace.name} · ${workspace.cwd}`}
          onClose={onClose}
        />

        <div className="modal__body orch-panel" data-modal-body>
          <p className="modal__intro">{t("orch.intro")}</p>

          <div className={`orch-panel__status ${orchestrator ? "is-on" : ""}`} role="status">
            <span className="orch-panel__status-icon" aria-hidden="true">
              <Icon name="orchestrator" size={16} />
            </span>
            <span className="orch-panel__status-text">
              {orchestrator ? (
                <>
                  <strong>{t("orch.statusPinned", { name: orchestrator.name })}</strong>
                  <span className="orch-panel__status-sub">
                    {workers.length
                      ? t("orch.statusWorkers", { n: workers.length })
                      : t("orch.statusNoWorker")}
                  </span>
                </>
              ) : (
                <>
                  <strong>{t("orch.statusNone")}</strong>
                  <span className="orch-panel__status-sub">{t("orch.statusNoneHint")}</span>
                </>
              )}
            </span>
            {!orchestrator && (
              <button type="button" className="btn" onClick={onCreateOrchestrator}>
                <Icon name="plus" size={12} />
                {t("cmd.workspace.newOrchestrator")}
              </button>
            )}
          </div>

          {section(
            t("orch.sectionWorkers"),
            <>
              {stepper("maxWorkers", t("orch.maxWorkers"), t("orch.maxWorkersHint"))}
              {stepper("maxAttempts", t("orch.maxAttempts"), t("orch.maxAttemptsHint"))}
            </>,
          )}

          {section(
            t("orch.sectionDeliveries"),
            <>
              <label className="form-grid__label" htmlFor={id("autoMerge")}>
                {t("orch.autoMerge")}
              </label>
              <div className="form-grid__field">
                <div className="form-grid__field--inline">
                  <span className="switch">
                    <input
                      id={id("autoMerge")}
                      type="checkbox"
                      role="switch"
                      checked={draft.autoMerge}
                      aria-describedby={id("autoMerge-hint")}
                      onChange={(event) => set("autoMerge", event.target.checked)}
                    />
                    <span className="switch__track" aria-hidden="true">
                      <span className="switch__knob" />
                    </span>
                  </span>
                </div>
                <span className="form-hint" id={id("autoMerge-hint")}>
                  {t("orch.autoMergeHint")}
                </span>
              </div>
              {textField("checkCommand", t("orch.checkCommand"), {
                placeholder: t("orch.checkPlaceholder"),
                hint: t("orch.checkHint"),
              })}
              {textField("branch", t("orch.branch"), {
                placeholder: t("orch.branchPlaceholder"),
                hint: t("orch.branchHint"),
                error: t("orch.branchInvalid"),
              })}
            </>,
          )}

          {section(
            t("orch.sectionModels"),
            <>
              {(
                [
                  ["orchestratorModel", t("orch.orchestratorModel")],
                  ["workerModel", t("orch.workerModel")],
                  ["reviewerModel", t("orch.reviewerModel")],
                ] as const
              ).map(([field, label]) => (
                <Fragment key={field}>
                  {textField(field, label, {
                    placeholder: ORCHESTRATION_DEFAULTS[field],
                    hint: fallback(ORCHESTRATION_DEFAULTS[field]),
                    error: t("orch.modelInvalid"),
                    models: true,
                  })}
                </Fragment>
              ))}
              <p className="form-hint form-grid__field--solo">{t("orch.modelsHint")}</p>
              <datalist id={id("models")}>
                {MODEL_SUGGESTIONS.map((name) => (
                  <option key={name} value={name} />
                ))}
              </datalist>
            </>,
          )}
        </div>

        <footer className="modal__foot">
          <p className="modal__note">{t("orch.applies")}</p>
          <button
            type="button"
            className="btn btn--ghost"
            title={t("orch.resetHint")}
            disabled={atDefaults}
            onClick={() => setDraft(draftOf({}))}
          >
            {t("orch.reset")}
          </button>
          <button type="button" className="btn btn--ghost" onClick={onClose}>
            {t("create.cancel")}
          </button>
          <button
            type="button"
            className="btn btn--primary"
            disabled={blocked || !changed}
            onClick={save}
          >
            {t("orch.save")}
          </button>
        </footer>
      </div>
    </Backdrop>
  );
}
