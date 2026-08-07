import { useMemo, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Backdrop } from "./Backdrop";
import { useT } from "../i18n";
import { findCodeWorkspace, readCodeWorkspace } from "../lib/ipc";
import { PANE_NAMES } from "../lib/paneNames";
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

const AGENT_SHORT: Record<AgentId, string> = {
  shell: "PS",
  claude: "CC",
  codex: "CX",
  gemini: "GM",
  opencode: "OC",
};

interface Props {
  availableAgents: string[];
  shells: ShellInfo[];
  onClose: () => void;
}

export function WorkspaceDialog({ availableAgents, shells, onClose }: Props) {
  const { addWorkspace } = useStore();

  const [name, setName] = useState("");
  const [cwd, setCwd] = useState("");
  /** Empty until a `.code-workspace` is loaded; `cwd` is one of these. */
  const [roots, setRoots] = useState<WorkspaceRoot[]>([]);
  /** A workspace file found next to the folder that was just picked. */
  const [suggestion, setSuggestion] = useState<string | null>(null);
  const [fileError, setFileError] = useState<string | null>(null);
  const [shellId, setShellId] = useState<string | null>(null);
  const [layout, setLayout] = useState<LayoutSize>(4);
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

  return (
    <Backdrop onClose={onClose}>
      <div className="modal modal--wide">
        <header className="modal__head">
          <h2>{t("create.title")}</h2>
          <button className="icon-btn" onClick={onClose}>
            ×
          </button>
        </header>

        <div className="modal__body modal__body--form">
          <section className="form-row">
            <label className="form-label" htmlFor="ws-folder">
              {t("create.folder")}
            </label>
            <div className="picker">
              <input
                id="ws-folder"
                className="picker__path"
                value={cwd}
                placeholder={t("create.folderEmpty")}
                onChange={(e) => setCwd(e.target.value)}
              />
              <button className="btn" onClick={browse}>
                {t("create.browse")}
              </button>
              <button
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
                  className="btn btn--tiny"
                  onClick={() => loadWorkspaceFile(suggestion)}
                >
                  {t("create.fileUse")}
                </button>
                <button className="icon-btn" onClick={() => setSuggestion(null)}>
                  ×
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
              <div className="roots">
                <span className="form-hint">{t("create.rootsHint")}</span>
                {roots.map((root) => {
                  const isMain = root.path === cwd;
                  return (
                    <button
                      key={root.path}
                      className={`root ${isMain ? "is-main" : ""} ${
                        root.exists ? "" : "is-missing"
                      }`}
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
            )}
          </section>

          <section className="form-row">
            <label className="form-label" htmlFor="ws-name">
              {t("create.name")}
            </label>
            <input
              id="ws-name"
              className="text-input"
              value={name}
              placeholder={cwd ? basename(cwd) : t("create.namePlaceholder")}
              onChange={(e) => setName(e.target.value)}
            />
          </section>

          <section className="form-row">
            <label className="form-label" htmlFor="ws-shell">
              {t("pane.shell")}
              <span className="form-hint">
                {t("create.shellDetected", { n: shells.length })}
              </span>
            </label>
            <select
              id="ws-shell"
              className="text-input"
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
          </section>

          <section className="form-row">
            <span className="form-label">{t("create.terminals")}</span>
            <div className="layouts layouts--lg">
              {LAYOUTS.map((size) => (
                <button
                  key={size}
                  className={`layouts__btn ${layout === size ? "is-active" : ""}`}
                  onClick={() => changeLayout(size)}
                >
                  {size}
                </button>
              ))}
            </div>
          </section>

          <section className="form-row">
            <span className="form-label">
              {t("create.agents")}
              <span className="form-hint">
                {freeSlots > 0
                  ? t("create.freeSlots", { n: freeSlots })
                  : t("create.allAssigned")}
              </span>
            </span>

            <div className="agent-counters">
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
                    <div className="counter__controls">
                      <button
                        className="counter__btn"
                        onClick={() => removeAgent(agent.id)}
                        disabled={counts[agent.id] === 0}
                      >
                        −
                      </button>
                      <span className="counter__value">{counts[agent.id]}</span>
                      <button
                        className="counter__btn"
                        onClick={() => addAgent(agent.id)}
                        disabled={freeSlots === 0}
                      >
                        +
                      </button>
                    </div>
                  </div>
                );
              })}
            </div>
          </section>

          <section className="form-row">
            <span className="form-label">
              {t("create.layout")}
              <span className="form-hint">{t("create.layoutHint")}</span>
            </span>
            <div
              className="preview"
              style={{ gridTemplateColumns: `repeat(${columns}, 1fr)` }}
            >
              {assignments.map((agent, index) => (
                <button
                  key={index}
                  className={`preview__cell preview__cell--${agent}`}
                  onClick={() => cycleCell(index)}
                  title={AGENTS.find((a) => a.id === agent)?.label}
                >
                  <span className="preview__tag">{AGENT_SHORT[agent]}</span>
                  {/* The names the panes will actually be created with. */}
                  <span className="preview__num">{PANE_NAMES[index] ?? index + 1}</span>
                </button>
              ))}
            </div>
          </section>
        </div>

        <footer className="modal__foot modal__foot--actions">
          <button className="btn btn--ghost" onClick={onClose}>
            {t("create.cancel")}
          </button>
          <button className="btn btn--primary" onClick={submit} disabled={!cwd}>
            {t("create.submit")}
          </button>
        </footer>
      </div>
    </Backdrop>
  );
}
