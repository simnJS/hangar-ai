import { useEffect, useRef, useSyncExternalStore } from "react";
import { useLocale } from "../i18n";
import { notify } from "../lib/notify";
import { gitCloneConsent, type ConsentRequest } from "../lib/workspaceFromGit";
import { useStore } from "../store";
import { workspaceFromGitText, type WorkspaceFromGitKey } from "../strings/workspaceFromGit";
import "./GitCloneConsent.css";

/**
 * The question an agent's `workspace_create` waits on when the repository is
 * not the person's: what would be cloned, from whom, where, and that its
 * dependencies run its code here. Clone or Refuse; unanswered, the tool is
 * told nobody decided and nothing is cloned.
 *
 * It is a card beside the window, like the app's notices: it does not take
 * the keyboard from the terminal being typed in, so a Return meant for an
 * agent never lands on Clone. A system notification calls the person when the
 * window is not the one they are looking at.
 */
export function GitCloneConsent() {
  const { locale, state } = useConsentContext();
  const pending = useSyncExternalStore(gitCloneConsent.subscribe, gitCloneConsent.pending);
  const told = useRef(new Set<string>());
  const text = (key: WorkspaceFromGitKey, params?: Record<string, string>) =>
    workspaceFromGitText(locale, key, params);
  const { notifyOnIdle, notifyOnlyWhenAway } = state.settings;

  // Once per request, and only when the person is away from the window.
  useEffect(() => {
    for (const request of pending) {
      if (told.current.has(request.id)) continue;
      told.current.add(request.id);
      if (!notifyOnIdle || (notifyOnlyWhenAway && document.hasFocus())) continue;
      void notify(titleOf(request, text), request.url);
    }
    // `text` only depends on the locale, read when the request arrives.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pending, notifyOnIdle, notifyOnlyWhenAway]);

  const request = pending[0];
  if (!request) return null;

  return (
    <aside
      className="git-consent"
      role="alertdialog"
      aria-labelledby="git-consent-title"
      aria-describedby="git-consent-warning"
    >
      <h2 className="git-consent__title" id="git-consent-title">
        {titleOf(request, text)}
      </h2>
      <dl className="git-consent__facts">
        <dt>{text("gitConsent.repo")}</dt>
        <dd className="git-consent__mono">{request.url}</dd>
        <dt>{text("gitConsent.owner")}</dt>
        <dd>{request.owner}</dd>
        <dt>{text("gitConsent.destination")}</dt>
        <dd className="git-consent__mono">{request.destination}</dd>
      </dl>
      {request.reason && (
        <p className="git-consent__reason">
          {text("gitConsent.notYours", { reason: request.reason.replace(/\.+$/, "") })}
        </p>
      )}
      <p className="git-consent__warning" id="git-consent-warning">
        {text("gitConsent.warning")}
      </p>
      <div className="git-consent__actions">
        {pending.length > 1 && (
          <span className="git-consent__more">
            {text("gitConsent.more", { n: String(pending.length - 1) })}
          </span>
        )}
        <button
          type="button"
          className="btn btn--ghost"
          onClick={() => gitCloneConsent.answer(request.id, false)}
        >
          {text("gitConsent.refuse")}
        </button>
        <button
          type="button"
          className="btn btn--primary"
          onClick={() => gitCloneConsent.answer(request.id, true)}
        >
          {text("gitConsent.clone")}
        </button>
      </div>
    </aside>
  );
}

function titleOf(
  request: ConsentRequest,
  text: (key: WorkspaceFromGitKey, params?: Record<string, string>) => string,
): string {
  return request.by
    ? text("gitConsent.title", { by: request.by })
    : text("gitConsent.titleUnknown");
}

function useConsentContext() {
  const { state } = useStore();
  return { state, locale: useLocale() };
}
