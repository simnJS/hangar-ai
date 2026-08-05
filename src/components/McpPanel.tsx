import { useEffect, useState } from "react";
import { Backdrop } from "./Backdrop";
import {
  agentInstructionsStatus,
  mcpInstall,
  mcpManualCommands,
  mcpTargets,
  writeAgentInstructions,
  type InstructionReport,
  type McpReport,
  type McpTarget,
} from "../lib/board";

interface Props {
  cwd: string;
  onClose: () => void;
}

export function McpPanel({ cwd, onClose }: Props) {
  const [targets, setTargets] = useState<McpTarget[]>([]);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [reports, setReports] = useState<McpReport[] | null>(null);
  const [manual, setManual] = useState<{ claude: string; codex: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const [writePlaybook, setWritePlaybook] = useState(true);
  const [playbookPresent, setPlaybookPresent] = useState(false);

  useEffect(() => {
    mcpTargets(cwd)
      .then((found) => {
        setTargets(found);
        // Pre-tick the tools that are installed but not wired up yet.
        setSelected(
          new Set(found.filter((t) => t.detected && !t.configured).map((t) => t.id)),
        );
      })
      .catch(() => setTargets([]));
    mcpManualCommands(cwd)
      .then(setManual)
      .catch(() => setManual(null));
    agentInstructionsStatus(cwd)
      .then(setPlaybookPresent)
      .catch(() => setPlaybookPresent(false));
  }, [cwd]);

  function toggle(id: string) {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  async function install() {
    setBusy(true);
    const result = await mcpInstall([...selected], cwd).catch((): McpReport[] => []);

    if (writePlaybook) {
      const written = await writeAgentInstructions(cwd).catch(
        (): InstructionReport[] => [],
      );
      result.push(
        ...written.map((entry) => ({
          id: entry.path.split(/[\\/]/).pop() ?? entry.path,
          ok: entry.ok,
          message: entry.message,
          path: entry.path,
        })),
      );
      setPlaybookPresent(true);
    }

    setReports(result);
    setBusy(false);
    mcpTargets(cwd).then(setTargets).catch(() => undefined);
  }

  return (
    <Backdrop onClose={onClose}>
      <div className="modal modal--wide">
        <header className="modal__head">
          <h2>Connecter les agents au tableau</h2>
          <button className="icon-btn" onClick={onClose}>
            ×
          </button>
        </header>

        <p className="modal__sub" title={cwd}>
          {cwd}
        </p>

        <div className="modal__body modal__body--form">
          <p className="form-hint form-hint--block">
            IaBench s'enregistre comme serveur MCP dans la configuration de chaque outil.
            Le port et le jeton ne sont jamais écrits dans ces fichiers : ils sont résolus
            au lancement, donc la configuration reste valable après un redémarrage.
          </p>

          <div className="targets">
            {targets.map((target) => (
              <label
                key={target.id}
                className={`target ${target.configured ? "target--done" : ""}`}
              >
                <input
                  type="checkbox"
                  checked={selected.has(target.id)}
                  onChange={() => toggle(target.id)}
                />
                <span className="target__meta">
                  <span className="target__label">
                    {target.label}
                    {target.configured && <em className="target__badge">déjà configuré</em>}
                    {!target.detected && !target.configured && (
                      <em className="target__badge target__badge--muted">non détecté</em>
                    )}
                  </span>
                  <span className="target__path">{target.path}</span>
                </span>
              </label>
            ))}
          </div>

          <label className={`target ${playbookPresent ? "target--done" : ""}`}>
            <input
              type="checkbox"
              checked={writePlaybook}
              onChange={(e) => setWritePlaybook(e.target.checked)}
            />
            <span className="target__meta">
              <span className="target__label">
                Écrire le mode d'emploi pour les agents
                {playbookPresent && <em className="target__badge">déjà présent</em>}
              </span>
              <span className="target__path">
                AGENTS.md + CLAUDE.md · ajoute .iabench/ au .gitignore
              </span>
            </span>
          </label>

          {reports && (
            <div className="reports">
              {reports.map((report) => (
                <p key={report.id} className={report.ok ? "report--ok" : "report--fail"}>
                  {report.ok ? "✓" : "✕"} {report.id} — {report.message}
                </p>
              ))}
              <p className="form-hint">
                Redémarre les agents concernés pour qu'ils chargent le serveur.
              </p>
            </div>
          )}

          {manual && (
            <section className="form-row">
              <span className="form-label">
                Ou à la main
                <span className="form-hint">si tu préfères passer par la CLI</span>
              </span>
              <pre className="snippet">{manual.claude}</pre>
              <pre className="snippet">{manual.codex}</pre>
            </section>
          )}
        </div>

        <footer className="modal__foot modal__foot--actions">
          <button className="btn btn--ghost" onClick={onClose}>
            Fermer
          </button>
          <button
            className="btn btn--primary"
            onClick={install}
            disabled={busy || selected.size === 0}
          >
            {busy ? "Installation…" : `Configurer (${selected.size})`}
          </button>
        </footer>
      </div>
    </Backdrop>
  );
}
