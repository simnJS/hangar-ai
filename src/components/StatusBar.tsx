import type { RefObject } from "react";
import { Icon } from "./Icon";
import { useT } from "../i18n";
import type { Workspace } from "../types";

/**
 * The bar along the bottom of the workspace: the broadcast field while the
 * terminals are on screen, and what is true of the whole workspace.
 */
export interface StatusBarProps {
  workspace: Workspace;
  /** The broadcast field is only offered over the terminals. */
  showBroadcast: boolean;
  value: string;
  onChange: (value: string) => void;
  /** `whenFree` holds the line until each agent is done (see App.sendBroadcast). */
  onSend: (whenFree: boolean) => void;
  inputRef: RefObject<HTMLInputElement | null>;
  paneCount: number;
  /** Panes whose agent can hold a line until it is free. */
  bridgedCount: number;
}

export function StatusBar({
  showBroadcast,
  value,
  onChange,
  onSend,
  inputRef,
  paneCount,
  bridgedCount,
}: StatusBarProps) {
  const t = useT();
  if (!showBroadcast) return null;
  return (
    <footer className="broadcast">
      <span className="broadcast__icon">⇉</span>
      <input
        className="broadcast__input"
        ref={inputRef}
        placeholder={t("broadcast.placeholder", { n: paneCount })}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") onSend(e.altKey);
        }}
      />
      <button
        className="btn btn--ghost"
        onClick={() => onSend(true)}
        disabled={!value.trim()}
        title={t("broadcast.whenFreeHint", { n: bridgedCount })}
      >
        <Icon name="hourglass" size={12} />
        {t("broadcast.whenFree")}
      </button>
      <button className="btn btn--primary" onClick={() => onSend(false)} disabled={!value.trim()}>
        {t("broadcast.send")}
      </button>
    </footer>
  );
}
