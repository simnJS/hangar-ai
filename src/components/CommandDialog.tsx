import { useState } from "react";
import { Backdrop, ModalHead } from "./Backdrop";
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
      <div className="modal" role="dialog" aria-modal="true" aria-labelledby="cmd-title">
        <ModalHead
          id="cmd-title"
          title={t(editing ? "commands.titleEdit" : "commands.title")}
          onClose={onClose}
        />

        <div className="modal__body form-grid" data-modal-body>
          <label className="form-grid__label" htmlFor="cmd-command">
            {t("commands.command")}
          </label>
          <div className="form-grid__field">
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
          </div>

          <label className="form-grid__label" htmlFor="cmd-label">
            {t("commands.label")}
          </label>
          <div className="form-grid__field">
            <input
              id="cmd-label"
              className="text-input"
              value={label}
              placeholder={trimmed || t("commands.labelPlaceholder")}
              onChange={(e) => setLabel(e.target.value)}
              onKeyDown={onKeyDown}
            />
            <span className="form-hint">{t("commands.labelHint")}</span>
          </div>

          <label className="form-grid__label" htmlFor="cmd-autorun">
            {t("commands.autoRun")}
          </label>
          <div className="form-grid__field form-grid__field--inline">
            <span className="switch">
              <input
                id="cmd-autorun"
                type="checkbox"
                role="switch"
                checked={autoRun}
                onChange={(e) => setAutoRun(e.target.checked)}
              />
              <span className="switch__track" aria-hidden="true">
                <span className="switch__knob" />
              </span>
            </span>
            <span className="form-hint">{t("commands.autoRunHint")}</span>
          </div>

          <label className="form-grid__label" htmlFor="cmd-broadcast">
            {t("commands.broadcast")}
          </label>
          <div className="form-grid__field form-grid__field--inline">
            <span className="switch">
              <input
                id="cmd-broadcast"
                type="checkbox"
                role="switch"
                checked={broadcast}
                onChange={(e) => setBroadcast(e.target.checked)}
              />
              <span className="switch__track" aria-hidden="true">
                <span className="switch__knob" />
              </span>
            </span>
            <span className="form-hint">{t("commands.broadcastHint")}</span>
          </div>

          <span className="form-grid__label" id="cmd-scope">
            {t("commands.scope")}
          </span>
          <div className="form-grid__field">
            <div className="seg" role="group" aria-labelledby="cmd-scope">
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
            <span className="form-hint">{t("commands.scopeHint")}</span>
          </div>
        </div>

        <footer className="modal__foot">
          <button type="button" className="btn btn--ghost" onClick={onClose}>
            {t("create.cancel")}
          </button>
          <button
            type="button"
            className="btn btn--primary"
            onClick={submit}
            disabled={!trimmed}
          >
            {t("commands.save")}
          </button>
        </footer>
      </div>
    </Backdrop>
  );
}
