import { useEffect, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { Backdrop, ModalHead } from "./Backdrop";
import { Glyph } from "./settings/icons";
import { listSessions } from "../lib/ipc";
import { arrowStep, stepIndex } from "../lib/menuNav";
import { useT, type Translator } from "../i18n";
import { AGENTS, type AgentId, type AgentSession } from "../types";

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

/** Up and Down step through the list, so a session is a few keys away. */
function onListKey(event: KeyboardEvent<HTMLDivElement>) {
  const step = arrowStep(event.key);
  if (!step) return;
  const rows = [...event.currentTarget.querySelectorAll<HTMLButtonElement>(".session")];
  const next = stepIndex(rows.indexOf(document.activeElement as HTMLButtonElement), rows.length, step, false);
  if (next === null) return;
  event.preventDefault();
  rows[next].focus();
}

export function SessionPicker({ agent, cwd, currentId, onPick, onClose }: Props) {
  const [sessions, setSessions] = useState<AgentSession[] | null>(null);
  const listRef = useRef<HTMLDivElement>(null);
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

  // Once the list is in, the keyboard starts on the session the pane is on
  // rather than on "New session": Return by reflex then keeps the conversation
  // instead of dropping it. Only while focus has not moved on its own.
  useEffect(() => {
    const list = listRef.current;
    if (!sessions || !list) return;
    const first = list.querySelector<HTMLButtonElement>(".session");
    const current = list.querySelector<HTMLButtonElement>(".session.is-active");
    if (current && document.activeElement === first) current.focus();
  }, [sessions]);

  const agentLabel = AGENTS.find((entry) => entry.id === agent)?.label ?? agent;

  /** The current session wears a check, so the list says where you are now. */
  const row = (id: string | null, label: ReactNode, meta: string) => {
    // The new-session row explains itself in a sentence; the others carry
    // metadata, set in the monospace of ids.
    const sentence = id === null;
    const current = currentId === id;
    return (
      <button
        key={id ?? "new"}
        type="button"
        className={`session ${current ? "is-active" : ""}`}
        aria-current={current ? "true" : undefined}
        onClick={() => onPick(id)}
      >
        <span className="session__check" aria-hidden="true">
          {current && <Glyph name="check" />}
        </span>
        <span className="session__label">{label}</span>
        <span className={`session__meta ${sentence ? "session__meta--text" : ""}`}>{meta}</span>
      </button>
    );
  };

  return (
    <Backdrop onClose={onClose}>
      <div
        className="modal modal--list"
        role="dialog"
        aria-modal="true"
        aria-labelledby="sessions-title"
      >
        <ModalHead
          id="sessions-title"
          title={t("sessions.title", { agent: agentLabel })}
          sub={cwd}
          onClose={onClose}
        />

        <div
          className="modal__body modal__body--list"
          data-modal-body
          ref={listRef}
          onKeyDown={onListKey}
        >
          {row(null, t("sessions.new"), t("sessions.newHint"))}

          {sessions && sessions.length > 0 && <hr className="modal__sep" />}

          {sessions === null && <p className="modal__empty">{t("sessions.loading")}</p>}

          {sessions?.length === 0 && (
            <p className="modal__empty">{t("sessions.empty")}</p>
          )}

          {sessions?.map((session) =>
            row(
              session.id,
              session.label || <em>{t("sessions.untitled")}</em>,
              `${relative(t, session.modified_ms)} · ${session.id.slice(0, 8)}`,
            ),
          )}
        </div>

        <footer className="modal__foot">
          <span className="modal__note">{t("sessions.note")}</span>
          <button type="button" className="btn btn--ghost" onClick={onClose}>
            {t("create.cancel")}
          </button>
        </footer>
      </div>
    </Backdrop>
  );
}
