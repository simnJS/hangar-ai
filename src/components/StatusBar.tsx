import { useEffect, useState, type RefObject } from "react";
import { Icon } from "./Icon";
import { useT } from "../i18n";
import { useStore } from "../store";
import { useWorkspaceActivity } from "../lib/agentState";
import { gitRepoInfo, type RepoInfo } from "../lib/git";
import { useShortcutLabel } from "../lib/useShortcuts";
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

/** Often enough to follow an agent switching branches, rarely enough to cost nothing. */
const GIT_REFRESH_MS = 20_000;

/**
 * The repository the workspace folder is in, or null when it is in none — or
 * when there is no git to ask, as in the browser demo, where the call fails.
 * Read again when the window comes back to the front: a branch changed in
 * another program is the likeliest thing to have moved meanwhile. A hidden
 * window — minimised, say — asks nothing at all: one git process every 20
 * seconds for a bar nobody can see is not nothing.
 */
function useRepo(cwd: string): RepoInfo | null {
  const [repo, setRepo] = useState<RepoInfo | null>(null);
  useEffect(() => {
    let alive = true;
    let lastRead = 0;
    setRepo(null);
    const read = () => {
      lastRead = Date.now();
      gitRepoInfo(cwd)
        .then((info) => {
          if (!alive) return;
          const next = info.installed && info.isRepo ? info : null;
          // Same branch, same head: keep the object, skip the render.
          setRepo((current) =>
            current &&
            next &&
            current.branch === next.branch &&
            current.head === next.head &&
            current.root === next.root
              ? current
              : next,
          );
        })
        .catch(() => alive && setRepo(null));
    };
    let timer = 0;
    const poll = () => {
      window.clearInterval(timer);
      timer = 0;
      if (document.hidden) return;
      read();
      timer = window.setInterval(read, GIT_REFRESH_MS);
    };
    poll();
    // Shown again: read at once, then on the usual beat. Coming back from
    // the taskbar is both shown and focused, and is worth one read, not two.
    const onFocus = () => {
      if (Date.now() - lastRead > 1000) read();
    };
    document.addEventListener("visibilitychange", poll);
    window.addEventListener("focus", onFocus);
    return () => {
      alive = false;
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", poll);
      window.removeEventListener("focus", onFocus);
    };
  }, [cwd]);
  return repo;
}

export function StatusBar({
  workspace,
  showBroadcast,
  value,
  onChange,
  onSend,
  inputRef,
  paneCount,
  bridgedCount,
}: StatusBarProps) {
  const t = useT();
  const { state } = useStore();
  const keys = useShortcutLabel();
  const repo = useRepo(workspace.cwd);
  const counts = useWorkspaceActivity(workspace.id);

  const broadcastKeys = keys("view.broadcast");
  const dictateKeys = state.settings.voiceEnabled ? keys("voice.dictate") : "";
  const empty = !value.trim();

  // Same order as the sidebar row: what needs you first.
  const summary = counts.waiting
    ? { kind: "waiting", text: t("sidebar.activityWaiting", { n: counts.waiting }) }
    : counts.yours
      ? { kind: "yours", text: t("sidebar.activityYours", { n: counts.yours }) }
      : counts.working
        ? { kind: "working", text: t("sidebar.activityWorking", { n: counts.working }) }
        : null;

  return (
    <footer className="status">
      {showBroadcast ? (
        <div className="status__broadcast">
          <span className="status__lead" aria-hidden="true">
            <Icon name="broadcast" size={16} />
          </span>
          <label className="status__field">
            <input
              className="status__input"
              ref={inputRef}
              placeholder={t("broadcast.placeholder", { n: paneCount })}
              aria-label={t("status.broadcast")}
              value={value}
              onChange={(e) => onChange(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") onSend(e.altKey);
              }}
            />
            {broadcastKeys && (
              <kbd className="status__kbd" title={t("cmd.view.broadcast")}>
                {broadcastKeys}
              </kbd>
            )}
          </label>
          <button
            type="button"
            className="status__btn"
            onClick={() => onSend(true)}
            disabled={empty}
            title={t("broadcast.whenFreeHint", { n: bridgedCount })}
          >
            <Icon name="clock" size={14} />
            <span className="status__btn-label">{t("broadcast.whenFree")}</span>
          </button>
          <button
            type="button"
            className="btn btn--primary status__send"
            onClick={() => onSend(false)}
            disabled={empty}
            title={t("status.sendHint")}
          >
            <Icon name="send" size={13} />
            {t("broadcast.send")}
          </button>
        </div>
      ) : (
        <div className="status__info">
          <span className="status__lead" aria-hidden="true">
            <Icon name="folder" size={15} />
          </span>
          <span className="status__path" title={workspace.cwd}>
            {/* See .status__path: cut from the start, read left to right. */}
            <span dir="ltr">{workspace.cwd}</span>
          </span>
          <span className={`status__summary ${summary ? `status__summary--${summary.kind}` : ""}`}>
            {summary ? summary.text : t("toolbar.activity.quiet")}
          </span>
        </div>
      )}

      <div className="status__items">
        {repo && (
          <span
            className="status__item"
            title={
              repo.branch
                ? t("status.branch", { branch: repo.branch, root: repo.root ?? workspace.cwd })
                : t("status.detached", { head: repo.head ?? "?", root: repo.root ?? workspace.cwd })
            }
          >
            <Icon name="branch" size={13} />
            <span className="status__text">{repo.branch ?? repo.head}</span>
            {repo.isLinkedWorktree && <span className="status__tag">{t("status.worktree")}</span>}
          </span>
        )}
        {dictateKeys && (
          <span className="status__item" title={t("status.dictate", { keys: dictateKeys })}>
            <Icon name="mic" size={13} />
            <span className="status__text">{dictateKeys}</span>
          </span>
        )}
        <span className="status__item status__item--panes" title={t("status.panesHint")}>
          <Icon name="layout4" size={13} />
          <span className="status__text">{t("status.panes", { n: paneCount })}</span>
        </span>
      </div>
    </footer>
  );
}
