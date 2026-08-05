import { useEffect, useState } from "react";
import { Backdrop } from "./Backdrop";
import { listSessions } from "../lib/ipc";
import { useT, type Translator } from "../i18n";
import type { AgentId, AgentSession } from "../types";

interface Props {
  agent: AgentId;
  cwd: string;
  currentId: string | null;
  onPick: (sessionId: string | null) => void;
  onClose: () => void;
}

const relative = (t: Translator, ms: number) => {
  const minutes = Math.round((Date.now() - ms) / 60000);
  if (minutes < 1) return t("sessions.justNow");
  if (minutes < 60) return t("sessions.minutesAgo", { n: minutes });
  const hours = Math.round(minutes / 60);
  if (hours < 24) return t("sessions.hoursAgo", { n: hours });
  return t("sessions.daysAgo", { n: Math.round(hours / 24) });
};

export function SessionPicker({ agent, cwd, currentId, onPick, onClose }: Props) {
  const [sessions, setSessions] = useState<AgentSession[] | null>(null);
  const t = useT();

  useEffect(() => {
    let cancelled = false;
    listSessions(agent, cwd)
      .then((found) => !cancelled && setSessions(found))
      .catch(() => !cancelled && setSessions([]));
    return () => {
      cancelled = true;
    };
  }, [agent, cwd]);

  return (
    <Backdrop onClose={onClose}>
      <div className="modal">
        <header className="modal__head">
          <h2>{t("sessions.title", { agent })}</h2>
          <button className="icon-btn" onClick={onClose}>
            ×
          </button>
        </header>

        <p className="modal__sub" title={cwd}>
          {cwd}
        </p>

        <div className="modal__body">
          <button
            className={`session ${currentId === null ? "is-active" : ""}`}
            onClick={() => onPick(null)}
          >
            <span className="session__label">{t("sessions.new")}</span>
            <span className="session__meta">{t("sessions.newHint")}</span>
          </button>

          {sessions === null && <p className="modal__empty">{t("sessions.loading")}</p>}

          {sessions?.length === 0 && (
            <p className="modal__empty">{t("sessions.empty")}</p>
          )}

          {sessions?.map((session) => (
            <button
              key={session.id}
              className={`session ${currentId === session.id ? "is-active" : ""}`}
              onClick={() => onPick(session.id)}
            >
              <span className="session__label">
                {session.label || <em>{t("sessions.untitled")}</em>}
              </span>
              <span className="session__meta">
                {relative(t, session.modified_ms)} · {session.id.slice(0, 8)}
              </span>
            </button>
          ))}
        </div>

        <footer className="modal__foot">
          <span className="modal__note">{t("sessions.note")}</span>
        </footer>
      </div>
    </Backdrop>
  );
}
