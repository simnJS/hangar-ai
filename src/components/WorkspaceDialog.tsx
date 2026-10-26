import { useMemo, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Backdrop, ModalHead } from "./Backdrop";
import { Glyph } from "./settings/icons";
import { useT } from "../i18n";
import { findCodeWorkspace, readCodeWorkspace } from "../lib/ipc";
import { allPaneNames, pickPaneNames } from "../lib/paneNames";
import { useStore, type WorkspaceDraft } from "../store";
import {
  AGENTS,
  type AgentId,
  type LayoutSize,
  type ShellInfo,
  type WorkspaceRoot,
} from "../types";

const LAYOUTS: LayoutSize[] = [1, 2, 4, 8];

const basename = (path: string) =>
  path.replace(/[\\/]+$/, "").split(/[\\/]/).pop() || path;

/** `My project.code-workspace` names a workspace `My project`. */
const workspaceFileName = (path: string) =>
  basename(path).replace(/\.code-workspace$/i, "");

/** Grid shape per layout, mirrored from the CSS so the preview matches reality. */
const PREVIEW_COLUMNS: Record<LayoutSize, number> = { 1: 1, 2: 2, 4: 2, 8: 4 };

interface Props {
  availableAgents: string[];
  shells: ShellInfo[];
  onClose: () => void;
}

export function WorkspaceDialog({ availableAgents, shells, onClose }: Props) {
  const { addWorkspace, state } = useStore();

  const [name, setName] = useState("");
  const [cwd, setCwd] = useState("");
  /** Empty until a `.code-workspace` is loaded; `cwd` is one of these. */
  const [roots, setRoots] = useState<WorkspaceRoot[]>([]);
  /** A workspace file found next to the folder that was just picked. */
  const [suggestion, setSuggestion] = useState<string | null>(null);
  const [fileError, setFileError] = useState<string | null>(null);
  const [shellId, setShellId] = useState<string | null>(null);
  const [layout, setLayout] = useState<LayoutSize>(4);
  const names = useMemo(
    () => pickPaneNames(layout, allPaneNames(state.workspaces)),
    [layout, state.workspaces],
  );
  // One entry per pane: the source of truth the counters and preview share.
  const [assignments, setAssignments] = useState<AgentId[]>(() =>
    Array.from({ length: 4 }, () => "shell" as AgentId),
  );
  const t = useT();

  const counts = useMemo(() => {
    const tally = {} as Record<AgentId, number>;
    for (const agent of AGENTS) tally[agent.id] = 0;
    for (const assigned of assignments) tally[assigned] += 1;
    return tally;
  }, [assignments]);

  const freeSlots = counts.shell;

  function changeLayout(next: LayoutSize) {
    setLayout(next);
    setAssignments((prev) => {
      const resized = prev.slice(0, next);
      while (resized.length < next) resized.push("shell");
      return resized;
    });
  }

  function addAgent(agent: AgentId) {
    setAssignments((prev) => {
      const index = prev.indexOf("shell");
      if (index === -1) return prev;
      const next = [...prev];
      next[index] = agent;
      return next;
    });
  }

  function removeAgent(agent: AgentId) {
    setAssignments((prev) => {
      const index = prev.lastIndexOf(agent);
      if (index === -1) return prev;
      const next = [...prev];
      next[index] = "shell";
      return next;
    });
  }

  /** Clicking a preview cell cycles that single pane through the agents. */
  function cycleCell(index: number) {
    setAssignments((prev) => {
      const order = AGENTS.map((a) => a.id);
      const next = [...prev];
      next[index] = order[(order.indexOf(prev[index]) + 1) % order.length];
      return next;
    });
  }

  async function browse() {
    const selected = await open({
      directory: true,
      multiple: false,
      title: t("create.folderDialog"),
    });
    if (typeof selected !== "string") return;
    setCwd(selected);
    setRoots([]);
    setFileError(null);
    setSuggestion(null);
    // Only auto-fill the name while the user has not typed their own.
    setName((current) => (current.trim() === "" ? basename(selected) : current));
    // A folder holding a workspace file is usually meant to be opened through
    // it — usually, not always, so this is offered rather than applied.
    findCodeWorkspace(selected)
      .then(setSuggestion)
      .catch(() => undefined);
  }

  async function browseWorkspaceFile() {
    const selected = await open({
      multiple: false,
      title: t("create.fileDialog"),
      filters: [{ name: t("create.fileFilter"), extensions: ["code-workspace"] }],
    });
    if (typeof selected !== "string") return;
    await loadWorkspaceFile(selected);
  }

  /** Splits a `.code-workspace` into the folder to open in and the rest. */
  async function loadWorkspaceFile(path: string) {
    let found: WorkspaceRoot[];
    try {
      found = await readCodeWorkspace(path);
    } catch {
      setFileError(path);
      return;
    }
    setSuggestion(null);
    setFileError(null);
    setRoots(found);
    // Terminals need a folder that is really there, so a file whose first entry
    // is missing on this machine falls through to the first one that is not.
    const main = found.find((root) => root.exists);
    if (main) setCwd(main.path);
    setName((current) =>
      current.trim() === "" ? workspaceFileName(path) : current,
    );
  }

  function submit() {
    if (!cwd) return;
    const draft: WorkspaceDraft = {
      name: name.trim() || basename(cwd),
      cwd,
      // `cwd` is the root the terminals get; the others only ride along for the
      // agents that can read outside it. Missing folders are never passed on.
      extraRoots: roots
        .filter((root) => root.exists && root.path !== cwd)
        .map((root) => root.path),
      layout,
      agents: assignments,
      shellId,
    };
    addWorkspace(draft);
    onClose();
  }

  const columns = PREVIEW_COLUMNS[layout];
  const agentLabel = (id: AgentId) => AGENTS.find((a) => a.id === id)?.label ?? id;

  return (
    <Backdrop onClose={onClose}>
      <div
        className="modal modal--wide"
        role="dialog"
        aria-modal="true"
        aria-labelledby="ws-dialog-title"
      >
        <ModalHead id="ws-dialog-title" title={t("create.title")} onClose={onClose} />

        {/* A form, so Return creates the workspace the way a dialog's default
            button answers it; submit() still refuses without a folder. */}
        <form
          className="modal__form"
          onSubmit={(event) => {
            event.preventDefault();
            submit();
          }}
        >
          <div className="modal__body form-grid" data-modal-body>
            <label className="form-grid__label" htmlFor="ws-folder">
              {t("create.folder")}
            </label>
            <div className="form-grid__field">
              <div className="picker">
                <input
                  id="ws-folder"
                  className="picker__path"
                  value={cwd}
                  placeholder={t("create.folderEmpty")}
                  spellCheck={false}
                  onChange={(e) => setCwd(e.target.value)}
                />
                <button type="button" className="btn" onClick={browse}>
                  {t("create.browse")}
                </button>
                <button
                  type="button"
                  className="btn"
                  onClick={browseWorkspaceFile}
                  title={t("create.fileDialog")}
                >
                  {t("create.openFile")}
                </button>
              </div>

              {suggestion && (
                <div className="notice">
                  <span className="notice__text">
                    {t("create.fileFound", { name: basename(suggestion) })}
                  </span>
                  <button
                    type="button"
                    className="btn btn--tiny"
                    onClick={() => loadWorkspaceFile(suggestion)}
                  >
                    {t("create.fileUse")}
                  </button>
                  <button
                    type="button"
                    className="notice__close"
                    aria-label={t("modal.close")}
                    title={t("modal.close")}
                    onClick={() => setSuggestion(null)}
                  >
                    <Glyph name="close" />
                  </button>
                </div>
              )}

              {fileError && (
                <p className="notice notice--error">
                  <span className="notice__text">
                    {t("create.fileFailed", { name: basename(fileError) })}
                  </span>
                </p>
              )}

              {roots.length > 0 && (
                <>
                  <div className="roots">
                    {roots.map((root) => {
                      const isMain = root.path === cwd;
                      return (
                        <button
                          type="button"
                          key={root.path}
                          className={`root ${isMain ? "is-main" : ""} ${
                            root.exists ? "" : "is-missing"
                          }`}
                          aria-pressed={isMain}
                          onClick={() => setCwd(root.path)}
                          disabled={!root.exists}
                          title={root.path}
                        >
                          <span className="root__name">{root.name}</span>
                          <span className="root__path">{root.path}</span>
                          <span className="root__tag">
                            {!root.exists
                              ? t("create.rootMissing")
                              : isMain
                                ? t("create.rootMain")
                                : t("create.rootExtra")}
                          </span>
                        </button>
                      );
                    })}
                  </div>
                  <span className="form-hint">{t("create.rootsHint")}</span>
                </>
              )}
            </div>

            <label className="form-grid__label" htmlFor="ws-name">
              {t("create.name")}
            </label>
            <div className="form-grid__field">
              <input
                id="ws-name"
                className="text-input"
                value={name}
                placeholder={cwd ? basename(cwd) : t("create.namePlaceholder")}
                onChange={(e) => setName(e.target.value)}
              />
            </div>

            <label className="form-grid__label" htmlFor="ws-shell">
              {t("pane.shell")}
            </label>
            <div className="form-grid__field form-grid__field--inline">
              <span className="set-select">
                <select
                  id="ws-shell"
                  className="set-select__field"
                  value={shellId ?? ""}
                  onChange={(e) => setShellId(e.target.value || null)}
                >
                  <option value="">
                    {t("create.shellDefault", {
                      name: shells[0]?.label ?? t("create.shellNone"),
                    })}
                  </option>
                  {shells.map((shell) => (
                    <option key={shell.id} value={shell.id}>
                      {shell.label}
                    </option>
                  ))}
                </select>
              </span>
              <span className="form-hint">
                {t("create.shellDetected", { n: shells.length })}
              </span>
            </div>

            <span className="form-grid__label" id="ws-terminals">
              {t("create.terminals")}
            </span>
            <div className="form-grid__field">
              <div className="seg seg--numbers" role="group" aria-labelledby="ws-terminals">
                {LAYOUTS.map((size) => (
                  <button
                    type="button"
                    key={size}
                    className={`seg__btn ${layout === size ? "is-active" : ""}`}
                    aria-pressed={layout === size}
                    onClick={() => changeLayout(size)}
                  >
                    {size}
                  </button>
                ))}
              </div>
            </div>

            <span className="form-grid__label" id="ws-agents">
              {t("create.agents")}
            </span>
            <div className="form-grid__field">
              <div className="agent-counters" role="group" aria-labelledby="ws-agents">
                {AGENTS.filter((a) => a.id !== "shell").map((agent) => {
                  const missing = !availableAgents.includes(agent.id);
                  return (
                    <div key={agent.id} className={`counter ${missing ? "is-missing" : ""}`}>
                      <span className="counter__name">
                        {agent.label}
                        {missing && (
                          <em className="counter__missing">{t("create.notInstalled")}</em>
                        )}
                      </span>
                      <span className="stepper">
                        <button
                          type="button"
                          className="stepper__btn"
                          aria-label={t("create.less", { agent: agent.label })}
                          title={t("create.less", { agent: agent.label })}
                          onClick={() => removeAgent(agent.id)}
                          disabled={counts[agent.id] === 0}
                        >
                          <Glyph name="minus" />
                        </button>
                        <span className="stepper__value">{counts[agent.id]}</span>
                        <button
                          type="button"
                          className="stepper__btn"
                          aria-label={t("create.more", { agent: agent.label })}
                          title={t("create.more", { agent: agent.label })}
                          onClick={() => addAgent(agent.id)}
                          disabled={freeSlots === 0}
                        >
                          <Glyph name="plus" />
                        </button>
                      </span>
                    </div>
                  );
                })}
              </div>
              <span className="form-hint" aria-live="polite">
                {freeSlots > 0
                  ? t("create.freeSlots", { n: freeSlots })
                  : t("create.allAssigned")}
              </span>
            </div>

            <span className="form-grid__label">
              {t("create.layout")}
            </span>
            <div className="form-grid__field">
              <div
                className="preview"
                style={{ gridTemplateColumns: `repeat(${columns}, 1fr)` }}
              >
                {/* Pane names are unique across workspaces: these are the next
                    free ones, the same the store will hand out. */}
                {assignments.map((agent, index) => (
                  <button
                    type="button"
                    key={index}
                    className={`preview__cell ${agent === "shell" ? "is-shell" : ""}`}
                    onClick={() => cycleCell(index)}
                    title={agentLabel(agent)}
                  >
                    <span className="preview__head">
                      {/* The names the panes will actually be created with. */}
                      <span className="preview__name">{names[index] ?? index + 1}</span>
                      <span className="preview__agent">{agentLabel(agent)}</span>
                    </span>
                  </button>
                ))}
              </div>
              <span className="form-hint">{t("create.layoutHint")}</span>
            </div>
          </div>

          <footer className="modal__foot">
            <button type="button" className="btn btn--ghost" onClick={onClose}>
              {t("create.cancel")}
            </button>
            <button type="submit" className="btn btn--primary" disabled={!cwd}>
              {t("create.submit")}
            </button>
          </footer>
        </form>
      </div>
    </Backdrop>
  );
}
