import { useEffect, useState } from "react";
import { Backdrop } from "./Backdrop";
import { useT } from "../i18n";
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
  const t = useT();

  useEffect(() => {
    mcpTargets(cwd)
      .then((found) => {
        setTargets(found);
        // Pre-tick the tools that are installed but not wired up yet — and the
        // ones whose entry is broken, since reinstalling is the repair.
        setSelected(
          new Set(
            found
              .filter((t) => t.detected && (!t.configured || t.stale))
              .map((t) => t.id),
          ),
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
          <h2>{t("mcp.title")}</h2>
          <button className="icon-btn" onClick={onClose}>
            ×
          </button>
        </header>

        <p className="modal__sub" title={cwd}>
          {cwd}
        </p>

        <div className="modal__body modal__body--form">
          <p className="form-hint form-hint--block">{t("mcp.intro")}</p>

          <div className="targets">
            {targets.map((target) => (
              <label
                key={target.id}
                className={`target ${
                  target.configured && !target.stale ? "target--done" : ""
                }`}
              >
                <input
                  type="checkbox"
                  checked={selected.has(target.id)}
                  onChange={() => toggle(target.id)}
                />
                <span className="target__meta">
                  <span className="target__label">
                    {target.label}
                    {target.stale && (
                      <em className="target__badge target__badge--alert">
                        {t("mcp.stale")}
                      </em>
                    )}
                    {target.configured && !target.stale && (
                      <em className="target__badge">{t("mcp.configured")}</em>
                    )}
                    {!target.detected && !target.configured && (
                      <em className="target__badge target__badge--muted">
                        {t("mcp.notDetected")}
                      </em>
                    )}
                  </span>
                  <span className="target__path">{target.path}</span>
                  {target.stale && (
                    <span className="target__path target__path--alert">
                      {target.command
                        ? t("mcp.staleHint", { command: target.command })
                        : t("mcp.staleHintNoCommand")}
                    </span>
                  )}
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
                {t("mcp.playbook")}
                {playbookPresent && (
                  <em className="target__badge">{t("mcp.alreadyThere")}</em>
                )}
              </span>
              <span className="target__path">{t("mcp.playbookHint")}</span>
            </span>
          </label>

          {reports && (
            <div className="reports">
              {reports.map((report) => (
                <p key={report.id} className={report.ok ? "report--ok" : "report--fail"}>
                  {report.ok ? "✓" : "✕"} {report.id} — {report.message}
                </p>
              ))}
              <p className="form-hint">{t("mcp.restartHint")}</p>
            </div>
          )}

          {manual && (
            <section className="form-row">
              <span className="form-label">
                {t("mcp.manual")}
                <span className="form-hint">{t("mcp.manualHint")}</span>
              </span>
              <pre className="snippet">{manual.claude}</pre>
              <pre className="snippet">{manual.codex}</pre>
            </section>
          )}
        </div>

        <footer className="modal__foot modal__foot--actions">
          <button className="btn btn--ghost" onClick={onClose}>
            {t("mcp.close")}
          </button>
          <button
            className="btn btn--primary"
            onClick={install}
            disabled={busy || selected.size === 0}
          >
            {busy ? t("mcp.installing") : t("mcp.install", { n: selected.size })}
          </button>
        </footer>
      </div>
    </Backdrop>
  );
}
