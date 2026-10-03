import { useState } from "react";
import { Backdrop } from "./Backdrop";
import { useT } from "../i18n";
import type { CommandScope, SavedCommandDraft } from "../store";
import type { SavedCommand } from "../types";

/**
 * Writing a saved command down, without a trip through the settings.
 *
 * Typed by hand rather than picked out of the scrollback: reading the last
 * line back from xterm is possible, but it also picks up the prompt, the
 * agent's own output and whatever the shell redrew, so what it saves is
 * frequently not what was run.
 */

interface Props {
  /** The command being edited and where it is stored; null adds a new one. */
  editing: { command: SavedCommand; scope: CommandScope } | null;
  onSubmit: (draft: SavedCommandDraft, scope: CommandScope) => void;
  onClose: () => void;
}

export function CommandDialog({ editing, onSubmit, onClose }: Props) {
  const [label, setLabel] = useState(editing?.command.label ?? "");
  const [command, setCommand] = useState(editing?.command.command ?? "");
  // Saving a command is asking for it to be run; the ones you finish by hand
  // are the exception.
  const [autoRun, setAutoRun] = useState(editing?.command.autoRun ?? true);
  const [broadcast, setBroadcast] = useState(editing?.command.broadcast ?? false);
  const [scope, setScope] = useState<CommandScope>(editing?.scope ?? "workspace");
  const t = useT();

  const trimmed = command.trim();

  function submit() {
    if (!trimmed) return;
    // An empty label leaves the command itself to name the entry, which for a
    // short one reads better than anything a placeholder could suggest.
    onSubmit({ label: label.trim(), command: trimmed, autoRun, broadcast }, scope);
  }

  /** Enter anywhere in the form saves it: there are two fields and a button. */
  const onKeyDown = (event: React.KeyboardEvent) => {
    if (event.key === "Enter") submit();
  };

  return (
    <Backdrop onClose={onClose}>
      <div className="modal">
        <header className="modal__head">
          <h2>{t(editing ? "commands.titleEdit" : "commands.title")}</h2>
          <button className="icon-btn" onClick={onClose}>
            ×
          </button>
        </header>

        <div className="modal__body modal__body--form">
          <section className="form-row">
            <label className="form-label" htmlFor="cmd-command">
              {t("commands.command")}
            </label>
            <input
              id="cmd-command"
              className="text-input text-input--mono"
              value={command}
              autoFocus
              spellCheck={false}
              placeholder={t("commands.commandPlaceholder")}
              onChange={(e) => setCommand(e.target.value)}
              onKeyDown={onKeyDown}
            />
          </section>

          <section className="form-row">
            <label className="form-label" htmlFor="cmd-label">
              {t("commands.label")}
              <span className="form-hint">{t("commands.labelHint")}</span>
            </label>
            <input
              id="cmd-label"
              className="text-input"
              value={label}
              placeholder={trimmed || t("commands.labelPlaceholder")}
              onChange={(e) => setLabel(e.target.value)}
              onKeyDown={onKeyDown}
            />
          </section>

          <section className="form-row form-row--toggle">
            <span className="form-label">
              {t("commands.autoRun")}
              <span className="form-hint">{t("commands.autoRunHint")}</span>
            </span>
            <span className="switch">
              <input
                type="checkbox"
                checked={autoRun}
                aria-label={t("commands.autoRun")}
                onChange={(e) => setAutoRun(e.target.checked)}
              />
              <span className="switch__track" aria-hidden="true">
                <span className="switch__knob" />
              </span>
            </span>
          </section>

          <section className="form-row form-row--toggle">
            <span className="form-label">
              {t("commands.broadcast")}
              <span className="form-hint">{t("commands.broadcastHint")}</span>
            </span>
            <span className="switch">
              <input
                type="checkbox"
                checked={broadcast}
                aria-label={t("commands.broadcast")}
                onChange={(e) => setBroadcast(e.target.checked)}
              />
              <span className="switch__track" aria-hidden="true">
                <span className="switch__knob" />
              </span>
            </span>
          </section>

          <section className="form-row">
            <span className="form-label">
              {t("commands.scope")}
              <span className="form-hint">{t("commands.scopeHint")}</span>
            </span>
            <div className="seg">
              <button
                type="button"
                className={`seg__btn ${scope === "workspace" ? "is-active" : ""}`}
                aria-pressed={scope === "workspace"}
                onClick={() => setScope("workspace")}
              >
                {t("commands.scopeWorkspace")}
              </button>
              <button
                type="button"
                className={`seg__btn ${scope === "global" ? "is-active" : ""}`}
                aria-pressed={scope === "global"}
                onClick={() => setScope("global")}
              >
                {t("commands.scopeGlobal")}
              </button>
            </div>
          </section>
        </div>

        <footer className="modal__foot modal__foot--actions">
          <button className="btn btn--ghost" onClick={onClose}>
            {t("create.cancel")}
          </button>
          <button className="btn btn--primary" onClick={submit} disabled={!trimmed}>
            {t("commands.save")}
          </button>
        </footer>
      </div>
    </Backdrop>
  );
}
