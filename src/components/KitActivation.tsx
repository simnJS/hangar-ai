import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useT, type Translator } from "../i18n";
import { ptyAlive, ptyWrite } from "../lib/ipc";
import {
  PERMISSION_LABEL,
  disableKit,
  enableKit,
  kitName,
  kitRequested,
  kitState,
  panesToRestart,
  permissionDiff,
  requestedPermissions,
  type KitState,
} from "../lib/kitActivation";
import { currentOs, runSetupCommand, type KitOs } from "../lib/kitSetup";
import {
  kitInspect,
  kitSetupMark,
  kitSetupStatus,
  kitsList,
  onKitsChanged,
  type KitInfo,
  type KitInspection,
  type KitSetupStatus,
} from "../lib/kits";
import { useStore } from "../store";
import type { Workspace, WorkspaceKit } from "../types";
import { Backdrop, ModalHead } from "./Backdrop";
import "./KitActivation.css";

/**
 * Enabling a kit on a workspace: the list of kits with a switch each, and the
 * activation view that says what a kit brings, what its tabs ask for and which
 * commands it wants run, before anything is stored. Refusing changes nothing.
 *
 * The view is drawn from props (`KitActivationView`); `KitActivation` reads the
 * kit from Rust; `WorkspaceKitsSection` is the list the workspace dialog shows.
 */

const OS_KEY = {
  windows: "kitsManage.osWindows",
  macos: "kitsManage.osMacos",
  linux: "kitsManage.osLinux",
} as const satisfies Record<KitOs, string>;

const message = (err: unknown) => (err instanceof Error ? err.message : String(err));

/** A setup step that was started in a pane, waiting for the person to say how it went. */
export interface StepRun {
  stepId: string;
  paneName: string;
}

export interface KitActivationViewProps {
  t: Translator;
  kit: KitInfo;
  inspection: KitInspection | null;
  steps: KitSetupStatus[] | null;
  /** What the workspace holds for this kit now; absent: not enabled. */
  entry: WorkspaceKit | undefined;
  workspaceName: string | null;
  /** The workspace exists, so a pane can be opened for a setup step. */
  canRun: boolean;
  os: KitOs;
  /** The step whose pane is being opened. */
  running: string | null;
  /** A step that ran and awaits "it worked". */
  ran: StepRun | null;
  error: string | null;
  onAccept: () => void;
  onClose: () => void;
  onRun: (step: KitSetupStatus) => void;
  onConfirm: (stepId: string) => void;
  onNotYet: () => void;
}

function Names({ title, names }: { title: string; names: string[] }) {
  if (names.length === 0) return null;
  return (
    <div className="kit-act__group">
      <h4 className="kit-act__label">
        {title} <span className="kit-act__count">{names.length}</span>
      </h4>
      <ul className="kit-act__chips">
        {names.map((name) => (
          <li key={name} className="kit-act__chip">
            {name}
          </li>
        ))}
      </ul>
    </div>
  );
}

export function KitActivationView(props: KitActivationViewProps) {
  const { t, kit, inspection, steps, entry, workspaceName, canRun, os, running, ran, error } = props;
  const manifest = kit.manifest;
  const requested = kitRequested(kit);
  const diff = permissionDiff(requested ?? [], entry?.granted ?? []);
  const needsAccept = !entry || diff.added.length > 0;
  const brings =
    inspection &&
    inspection.skills.length +
      inspection.commands.length +
      inspection.agents.length +
      inspection.mcpServers.length +
      inspection.tabs.length;

  return (
    <div className="kit-act">
      {workspaceName && (
        <p className="kit-act__ws">{t("kitsManage.actOnWorkspace", { name: workspaceName })}</p>
      )}
      {manifest && (
        <p className="kit-act__desc">
          <strong>{manifest.name}</strong> <span className="kit-act__ver">v{kit.installedVersion}</span>
          {manifest.description ? ` — ${manifest.description}` : ""}
        </p>
      )}

      <section className="kit-act__block" aria-labelledby="kit-act-brings">
        <h3 className="kit-act__heading" id="kit-act-brings">
          {t("kitsManage.actBrings")}
        </h3>
        {inspection === null ? (
          <p className="kit-act__note">{t("kitsManage.loading")}</p>
        ) : brings === 0 ? (
          <p className="kit-act__note">{t("kitsManage.actNothing")}</p>
        ) : (
          <>
            <Names title={t("kitsManage.actSkills")} names={inspection.skills} />
            <Names title={t("kitsManage.actCommands")} names={inspection.commands} />
            <Names title={t("kitsManage.actAgents")} names={inspection.agents} />
            <Names title={t("kitsManage.actMcp")} names={inspection.mcpServers} />
            <Names title={t("kitsManage.actTabs")} names={inspection.tabs.map((tab) => tab.title)} />
          </>
        )}
        <p className="kit-act__note">{t("kitsManage.actClaudeOnly")}</p>
      </section>

      <section className="kit-act__block" aria-labelledby="kit-act-perms">
        <h3 className="kit-act__heading" id="kit-act-perms">
          {t("kitsManage.actPerms")}
        </h3>
        {inspection === null ? null : requestedPermissions(inspection.tabs).length === 0 ? (
          <p className="kit-act__note">{t("kitsManage.actNoPerms")}</p>
        ) : (
          inspection.tabs
            .filter((tab) => tab.permissions.length > 0)
            .map((tab) => (
              <div key={tab.id} className="kit-act__group">
                <h4 className="kit-act__label">{t("kitsManage.actPermsFor", { tab: tab.title })}</h4>
                <ul className="kit-act__perms">
                  {tab.permissions.map((permission) => {
                    const isNew = diff.added.includes(permission);
                    return (
                      <li key={permission} className="kit-act__perm">
                        <span>{t(PERMISSION_LABEL[permission])}</span>
                        {entry &&
                          (isNew ? (
                            <span className="kit-act__tag kit-act__tag--new">
                              {t("kitsManage.actPermsNew")}
                            </span>
                          ) : (
                            <span className="kit-act__tag">{t("kitsManage.actPermsGranted")}</span>
                          ))}
                      </li>
                    );
                  })}
                </ul>
              </div>
            ))
        )}
      </section>

      <section className="kit-act__block" aria-labelledby="kit-act-setup">
        <h3 className="kit-act__heading" id="kit-act-setup">
          {t("kitsManage.actSetup")}
        </h3>
        {steps === null ? (
          <p className="kit-act__note">{t("kitsManage.loading")}</p>
        ) : steps.length === 0 ? (
          <p className="kit-act__note">{t("kitsManage.actNoSetup")}</p>
        ) : (
          <>
            <p className="kit-act__note">{t("kitsManage.actSetupHint")}</p>
            {steps.map((step) => (
              <div key={step.id} className="kit-step">
                <div className="kit-step__head">
                  <span className="kit-step__title">{step.title}</span>
                  <span className={`kit-act__tag ${step.done ? "kit-act__tag--ok" : ""}`}>
                    {step.done ? t("kitsManage.stepDone") : t("kitsManage.stepTodo")}
                  </span>
                </div>
                {step.description && <p className="kit-act__note">{step.description}</p>}
                <pre
                  className="kit-step__cmd"
                  tabIndex={0}
                  aria-label={t("kitsManage.actCommandOs", { os: t(OS_KEY[os]) })}
                >
                  {step.command}
                </pre>
                {ran?.stepId === step.id ? (
                  <div className="kit-step__ask">
                    <p className="kit-act__note">{t("kitsManage.stepOpened", { pane: ran.paneName })}</p>
                    <div className="kit-act__row">
                      <button type="button" className="btn btn--primary" onClick={() => props.onConfirm(step.id)}>
                        {t("kitsManage.stepConfirm")}
                      </button>
                      <button type="button" className="btn btn--ghost" onClick={props.onNotYet}>
                        {t("kitsManage.stepNotYet")}
                      </button>
                    </div>
                  </div>
                ) : (
                  !step.done && (
                    <div className="kit-act__row">
                      <button
                        type="button"
                        className="btn"
                        disabled={!canRun || running !== null || ran !== null}
                        onClick={() => props.onRun(step)}
                      >
                        {running === step.id ? t("kitsManage.stepRunning") : t("kitsManage.stepRun")}
                      </button>
                      {!canRun && <span className="kit-act__note">{t("kitsManage.stepRunNeedsWs")}</span>}
                    </div>
                  )
                )}
              </div>
            ))}
          </>
        )}
      </section>

      {error && (
        <p className="notice notice--error" role="alert">
          <span className="notice__text">{error}</span>
        </p>
      )}

      <footer className="kit-act__foot">
        {needsAccept ? (
          <>
            <button type="button" className="btn btn--ghost" onClick={props.onClose}>
              {t("kitsManage.actRefuse")}
            </button>
            <button
              type="button"
              className="btn btn--primary"
              disabled={!requested || inspection === null}
              onClick={props.onAccept}
            >
              {entry ? t("kitsManage.actAcceptPerms") : t("kitsManage.actAccept")}
            </button>
          </>
        ) : (
          <button type="button" className="btn" onClick={props.onClose}>
            {t("kitsManage.actClose")}
          </button>
        )}
      </footer>
    </div>
  );
}

interface ActivationProps {
  kit: KitInfo;
  entry: WorkspaceKit | undefined;
  /** The workspace the kit is enabled on, when it exists. */
  workspace: Workspace | null;
  /** Called with the permissions the person accepted. */
  onAccept: (granted: ReturnType<typeof requestedPermissions>) => void;
  onClose: () => void;
}

/** The activation view as a dialog, fed from Rust. */
export function KitActivation({ kit, entry, workspace, onAccept, onClose }: ActivationProps) {
  const t = useT();
  const { addPane, snapshot } = useStore();
  const [inspection, setInspection] = useState<KitInspection | null>(null);
  const [steps, setSteps] = useState<KitSetupStatus[] | null>(null);
  const [running, setRunning] = useState<string | null>(null);
  const [ran, setRan] = useState<StepRun | null>(null);
  const [error, setError] = useState<string | null>(null);
  const os = useMemo(() => currentOs(), []);
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  useEffect(() => {
    kitInspect(kit.id)
      .then((found) => alive.current && setInspection(found))
      .catch((err) => alive.current && setError(t("kitsManage.errorPrefix", { detail: message(err) })));
    kitSetupStatus(kit.id)
      .then((found) => alive.current && setSteps(found))
      .catch(() => alive.current && setSteps([]));
  }, [kit.id, t]);

  /**
   * Run: opens a shell pane in the kit's folder and types the command shown,
   * verbatim. Only ever reached from the Run button.
   */
  const run = useCallback(
    async (step: KitSetupStatus) => {
      if (!workspace) return;
      setError(null);
      setRunning(step.id);
      let paneName = "";
      const result = await runSetupCommand(step.command, {
        openPane: () => {
          const id = addPane(workspace.id, { agent: "shell", cwd: kit.dir });
          if (id) {
            const pane = snapshot()
              .workspaces.find((ws) => ws.id === workspace.id)
              ?.panes.find((p) => p.id === id);
            paneName = pane?.name ?? "";
          }
          return id;
        },
        alive: ptyAlive,
        write: ptyWrite,
        wait: (ms) => new Promise((resolve) => window.setTimeout(resolve, ms)),
      });
      if (!alive.current) return;
      setRunning(null);
      if (result.ok) setRan({ stepId: step.id, paneName: paneName || "…" });
      else if (result.reason === "no-pane") setError(t("kitsManage.stepNoPane"));
      else if (result.reason === "not-started") setError(t("kitsManage.stepPaneLost"));
      else setError(t("kitsManage.stepFailed", { detail: result.detail ?? "" }));
    },
    [workspace, addPane, snapshot, kit.dir, t],
  );

  /** Marks the step done: the person confirmed it, Hangar cannot read the exit code. */
  const confirm = useCallback(
    async (stepId: string) => {
      try {
        const next = await kitSetupMark(kit.id, stepId);
        if (!alive.current) return;
        setSteps(next);
        setRan(null);
      } catch (err) {
        if (alive.current) setError(t("kitsManage.errorPrefix", { detail: message(err) }));
      }
    },
    [kit.id, t],
  );

  // This dialog opens over another one (the workspace's): Escape closes this one
  // only, so it is caught before the dialog underneath hears it.
  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if (event.key !== "Escape") return;
      event.stopImmediatePropagation();
      onClose();
    }
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [onClose]);

  const name = kitName(kit);
  return (
    <Backdrop onClose={onClose} escape={false}>
      <div className="modal modal--wide" role="dialog" aria-modal="true" aria-labelledby="kit-act-title">
        <ModalHead
          id="kit-act-title"
          title={t(entry ? "kitsManage.actReviewTitle" : "kitsManage.actTitle", { name })}
          onClose={onClose}
        />
        <div className="modal__body" data-modal-body>
          <KitActivationView
            t={t}
            kit={kit}
            inspection={inspection}
            steps={steps}
            entry={entry}
            workspaceName={workspace?.name ?? null}
            canRun={workspace !== null}
            os={os}
            running={running}
            ran={ran}
            error={error}
            onAccept={() => onAccept(requestedPermissions(inspection?.tabs ?? []))}
            onClose={onClose}
            onRun={run}
            onConfirm={confirm}
            onNotYet={() => setRan(null)}
          />
        </div>
      </div>
    </Backdrop>
  );
}

const STATE_KEY = {
  on: "kitsManage.statusOn",
  off: "kitsManage.statusOff",
  review: "kitsManage.statusReview",
  broken: "kitsManage.statusBroken",
} as const satisfies Record<KitState, string>;

export interface KitsSectionProps {
  /** The kits enabled so far. */
  value: WorkspaceKit[];
  onChange: (next: WorkspaceKit[]) => void;
  /**
   * The workspace being edited. Absent while one is being created: there is no
   * pane to run a setup step in yet, and no running agent to restart.
   */
  workspace?: Workspace | null;
}

/** The "Kits" section of the workspace dialog: a switch per installed kit. */
export function WorkspaceKitsSection({ value, onChange, workspace = null }: KitsSectionProps) {
  const t = useT();
  const { respawnPane } = useStore();
  const [kits, setKits] = useState<KitInfo[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [opened, setOpened] = useState<string | null>(null);
  /** A kit just enabled on a workspace whose agents may be running. */
  const [restart, setRestart] = useState<string | null>(null);
  const [restarted, setRestarted] = useState(false);

  const refresh = useCallback(
    () =>
      kitsList()
        .then((list) => {
          setKits(list);
          setError(null);
        })
        .catch((err) => setError(message(err))),
    [],
  );

  useEffect(() => {
    void refresh();
    const stop = onKitsChanged(() => void refresh());
    return () => void stop.then((off) => off()).catch(() => undefined);
  }, [refresh]);

  const known = new Set((kits ?? []).map((kit) => kit.id));
  const gone = value.filter((entry) => kits !== null && !known.has(entry.id));
  const openKit = kits?.find((kit) => kit.id === opened) ?? null;

  function toggle(kit: KitInfo, on: boolean) {
    setRestarted(false);
    if (on) setOpened(kit.id);
    else onChange(disableKit(value, kit.id));
  }

  function accept(kit: KitInfo, granted: Parameters<typeof enableKit>[2]) {
    onChange(enableKit(value, kit.id, granted));
    setOpened(null);
    // Agents already running keep the kits they started with.
    if (workspace && panesToRestart(workspace).length > 0) setRestart(kitName(kit));
  }

  function restartPanes() {
    if (!workspace) return;
    // A respawn with no patch keeps each pane's session: the conversation resumes.
    for (const paneId of panesToRestart(workspace)) respawnPane(workspace.id, paneId, {});
    setRestart(null);
    setRestarted(true);
  }

  return (
    <div className="kit-sec">
      <p className="form-hint">{t("kitsManage.sectionHint")}</p>
      {error && (
        <p className="notice notice--error" role="alert">
          <span className="notice__text">{t("kitsManage.errorPrefix", { detail: error })}</span>
        </p>
      )}
      {kits !== null && kits.length === 0 && gone.length === 0 && (
        <p className="form-hint">{t("kitsManage.sectionNone")}</p>
      )}
      <ul className="kit-sec__list">
        {(kits ?? []).map((kit) => {
          const entry = value.find((item) => item.id === kit.id);
          const state = kit.error || !kit.manifest ? (entry ? "broken" : "off") : kitState(entry, kitRequested(kit));
          const usable = !kit.error && !!kit.manifest;
          const name = kitName(kit);
          return (
            <li key={kit.id} className="kit-sec__row">
              <label className="kit-sec__main">
                <span className="switch">
                  <input
                    type="checkbox"
                    role="switch"
                    checked={!!entry}
                    disabled={!usable && !entry}
                    aria-label={t("kitsManage.toggle", { name })}
                    onChange={(event) => toggle(kit, event.target.checked)}
                  />
                  <span className="switch__track" aria-hidden="true">
                    <span className="switch__knob" />
                  </span>
                </span>
                <span className="kit-sec__text">
                  <span className="kit-sec__name">{name}</span>
                  <span className="kit-sec__meta" title={kit.error ?? undefined}>
                    {kit.error ?? `v${kit.installedVersion}`}
                  </span>
                </span>
              </label>
              <span className={`kit-act__tag ${state === "review" ? "kit-act__tag--new" : state === "on" ? "kit-act__tag--ok" : ""}`}>
                {t(STATE_KEY[state])}
              </span>
              {usable && (state === "review" || state === "on") && (
                <button type="button" className="btn btn--tiny" onClick={() => setOpened(kit.id)}>
                  {state === "review" ? t("kitsManage.review") : t("kitsManage.details")}
                </button>
              )}
            </li>
          );
        })}
        {gone.map((entry) => (
          <li key={entry.id} className="kit-sec__row">
            <label className="kit-sec__main">
              <span className="switch">
                <input
                  type="checkbox"
                  role="switch"
                  checked
                  aria-label={t("kitsManage.toggle", { name: entry.id })}
                  onChange={() => onChange(disableKit(value, entry.id))}
                />
                <span className="switch__track" aria-hidden="true">
                  <span className="switch__knob" />
                </span>
              </span>
              <span className="kit-sec__text">
                <span className="kit-sec__name">{entry.id}</span>
                <span className="kit-sec__meta">{t("kitsManage.brokenName", { id: entry.id })}</span>
              </span>
            </label>
            <span className="kit-act__tag">{t("kitsManage.statusBroken")}</span>
          </li>
        ))}
      </ul>

      {!workspace && value.length > 0 && <p className="form-hint">{t("kitsManage.pendingNote")}</p>}

      {restart && workspace && (
        <div className="notice kit-sec__restart" role="status">
          <span className="notice__text">
            <strong>{t("kitsManage.restartTitle")}</strong>
            <br />
            {t("kitsManage.restartBody", { name: restart })} {t("kitsManage.restartResume")}
          </span>
          <button type="button" className="btn btn--tiny" onClick={restartPanes}>
            {t("kitsManage.restartAction", { n: panesToRestart(workspace).length })}
          </button>
          <button type="button" className="btn btn--tiny btn--ghost" onClick={() => setRestart(null)}>
            {t("kitsManage.restartLater")}
          </button>
        </div>
      )}
      {restarted && (
        <p className="form-hint" role="status">
          {t("kitsManage.restartDone")}
        </p>
      )}

      {openKit && (
        <KitActivation
          kit={openKit}
          entry={value.find((item) => item.id === openKit.id)}
          workspace={workspace}
          onAccept={(granted) => accept(openKit, granted)}
          onClose={() => setOpened(null)}
        />
      )}
    </div>
  );
}
