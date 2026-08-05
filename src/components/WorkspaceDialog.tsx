import { useMemo, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Backdrop } from "./Backdrop";
import { useStore, type WorkspaceDraft } from "../store";
import { AGENTS, type AgentId, type LayoutSize, type ShellInfo } from "../types";

const LAYOUTS: LayoutSize[] = [1, 2, 4, 8];

const basename = (path: string) =>
  path.replace(/[\\/]+$/, "").split(/[\\/]/).pop() || path;

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
  const [shellId, setShellId] = useState<string | null>(null);
  const [layout, setLayout] = useState<LayoutSize>(4);
  // One entry per pane: the source of truth the counters and preview share.
  const [assignments, setAssignments] = useState<AgentId[]>(() =>
    Array.from({ length: 4 }, () => "shell" as AgentId),
  );

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
      title: "Dossier du projet",
    });
    if (typeof selected !== "string") return;
    setCwd(selected);
    // Only auto-fill the name while the user has not typed their own.
    setName((current) => (current.trim() === "" ? basename(selected) : current));
  }

  function submit() {
    if (!cwd) return;
    const draft: WorkspaceDraft = {
      name: name.trim() || basename(cwd),
      cwd,
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
          <h2>Nouveau workspace</h2>
          <button className="icon-btn" onClick={onClose}>
            ×
          </button>
        </header>

        <div className="modal__body modal__body--form">
          <section className="form-row">
            <label className="form-label" htmlFor="ws-folder">
              Dossier du projet
            </label>
            <div className="picker">
              <input
                id="ws-folder"
                className="picker__path"
                value={cwd}
                placeholder="Aucun dossier sélectionné"
                onChange={(e) => setCwd(e.target.value)}
              />
              <button className="btn" onClick={browse}>
                Parcourir…
              </button>
            </div>
          </section>

          <section className="form-row">
            <label className="form-label" htmlFor="ws-name">
              Nom
            </label>
            <input
              id="ws-name"
              className="text-input"
              value={name}
              placeholder={cwd ? basename(cwd) : "Mon projet"}
              onChange={(e) => setName(e.target.value)}
            />
          </section>

          <section className="form-row">
            <label className="form-label" htmlFor="ws-shell">
              Shell
              <span className="form-hint">
                {shells.length} détecté{shells.length > 1 ? "s" : ""} sur ce PC
              </span>
            </label>
            <select
              id="ws-shell"
              className="text-input"
              value={shellId ?? ""}
              onChange={(e) => setShellId(e.target.value || null)}
            >
              <option value="">Par défaut ({shells[0]?.label ?? "aucun"})</option>
              {shells.map((shell) => (
                <option key={shell.id} value={shell.id}>
                  {shell.label}
                </option>
              ))}
            </select>
          </section>

          <section className="form-row">
            <span className="form-label">Nombre de terminaux</span>
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
              Répartition des agents
              <span className="form-hint">
                {freeSlots > 0
                  ? `${freeSlots} panneau${freeSlots > 1 ? "x" : ""} en shell simple`
                  : "tous les panneaux sont attribués"}
              </span>
            </span>

            <div className="agent-counters">
              {AGENTS.filter((a) => a.id !== "shell").map((agent) => {
                const missing = !availableAgents.includes(agent.id);
                return (
                  <div key={agent.id} className={`counter ${missing ? "is-missing" : ""}`}>
                    <span className="counter__name">
                      {agent.label}
                      {missing && <em className="counter__missing">non installé</em>}
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
              Disposition
              <span className="form-hint">clique une case pour changer son agent</span>
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
                  <span className="preview__num">{index + 1}</span>
                </button>
              ))}
            </div>
          </section>
        </div>

        <footer className="modal__foot modal__foot--actions">
          <button className="btn btn--ghost" onClick={onClose}>
            Annuler
          </button>
          <button className="btn btn--primary" onClick={submit} disabled={!cwd}>
            Créer le workspace
          </button>
        </footer>
      </div>
    </Backdrop>
  );
}
