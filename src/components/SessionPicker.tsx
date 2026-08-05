import { useEffect, useState } from "react";
import { Backdrop } from "./Backdrop";
import { listSessions } from "../lib/ipc";
import type { AgentId, AgentSession } from "../types";

interface Props {
  agent: AgentId;
  cwd: string;
  currentId: string | null;
  onPick: (sessionId: string | null) => void;
  onClose: () => void;
}

const relative = (ms: number) => {
  const delta = Date.now() - ms;
  const minutes = Math.round(delta / 60000);
  if (minutes < 1) return "à l'instant";
  if (minutes < 60) return `il y a ${minutes} min`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `il y a ${hours} h`;
  return `il y a ${Math.round(hours / 24)} j`;
};

export function SessionPicker({ agent, cwd, currentId, onPick, onClose }: Props) {
  const [sessions, setSessions] = useState<AgentSession[] | null>(null);

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
          <h2>Sessions {agent}</h2>
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
            <span className="session__label">Nouvelle session</span>
            <span className="session__meta">démarre l'agent sans reprise</span>
          </button>

          {sessions === null && <p className="modal__empty">Lecture des transcriptions…</p>}

          {sessions?.length === 0 && (
            <p className="modal__empty">Aucune session enregistrée pour ce dossier.</p>
          )}

          {sessions?.map((session) => (
            <button
              key={session.id}
              className={`session ${currentId === session.id ? "is-active" : ""}`}
              onClick={() => onPick(session.id)}
            >
              <span className="session__label">
                {session.label || <em>sans titre</em>}
              </span>
              <span className="session__meta">
                {relative(session.modified_ms)} · {session.id.slice(0, 8)}
              </span>
            </button>
          ))}
        </div>

        <footer className="modal__foot">
          <span className="modal__note">
            Le panneau redémarre pour appliquer la session choisie.
          </span>
        </footer>
      </div>
    </Backdrop>
  );
}
