import { useCallback, useEffect, useRef, useState } from "react";
import type { App } from "../api";
import { afterListing, appName, openFailure, type Viewer } from "../apps";
import { useNavigation } from "../components/Chrome";
import { Icon } from "../components/Icon";
import { usePolling } from "../hooks";
import { i18n, t } from "../i18n";
import { useRemoteContext } from "../remote";

/**
 * An agent's app on the phone, full screen: its dev server as the PC shows
 * it, hot reload included, in a frame under a slim bar — back, its name and
 * port, reload, and Safari for when a frame is not enough.
 *
 * The PC opens it through a slot of its own (`/m/api/app/open`), the app at
 * the root of an address of the tailnet. While it is on screen, the project's
 * apps are read every few seconds: the frame cannot say that its server
 * stopped, the PC's list can.
 */

/** While the app is on screen, its project's listing is read this often. */
const LISTING_MS = 5_000;

export interface AppViewProps {
  /** Its worker, or the person's own server. */
  name: string;
  /** Its project. */
  where: string;
  port: number;
  viewer: Viewer;
  /** Changes with each reload: the frame is drawn anew. */
  frame: number;
  onBack: () => void;
  onReload: () => void;
  onLoaded: () => void;
}

const urlOf = (viewer: Viewer) => (viewer.kind === "loading" || viewer.kind === "shown" ? viewer.url : null);

/** The viewer, from props alone: every state can be drawn without a PC. */
export function AppView({ name, where, port, viewer, frame, onBack, onReload, onLoaded }: AppViewProps) {
  const url = urlOf(viewer);
  return (
    <div className="viewer">
      <header className="viewer__bar">
        <button type="button" className="viewer__button" onClick={onBack} aria-label={t("app.back")}>
          <Icon name="chevronLeft" />
        </button>
        <div className="viewer__who">
          <h1 className="viewer__name">{name}</h1>
          <p className="viewer__where">
            {where ? `${where} · ` : ""}
            {t("apps.port", { port })}
          </p>
        </div>
        <button
          type="button"
          className="viewer__button"
          onClick={onReload}
          disabled={viewer.kind === "starting"}
          aria-label={t("app.reload")}
          title={t("app.reload")}
        >
          <Icon name="reload" />
        </button>
        {url ? (
          <a className="viewer__button" href={url} target="_blank" rel="noopener noreferrer" aria-label={t("app.safari")} title={t("app.safari")}>
            <Icon name="external" />
          </a>
        ) : (
          <button type="button" className="viewer__button" disabled aria-label={t("app.safari")} title={t("app.safari")}>
            <Icon name="external" />
          </button>
        )}
      </header>

      <div className="viewer__body">
        {url && (
          <iframe
            key={frame}
            className="viewer__frame"
            src={url}
            title={t("app.frame", { name, port })}
            allow="fullscreen; clipboard-write"
            onLoad={onLoaded}
          />
        )}
        <ViewerState viewer={viewer} port={port} onRetry={onReload} />
      </div>
    </div>
  );
}

/** Over the frame, or in its place: what the viewer is waiting for, or why there is no app. */
function ViewerState({ viewer, port, onRetry }: { viewer: Viewer; port: number; onRetry: () => void }) {
  switch (viewer.kind) {
    case "shown":
      return null;
    case "starting":
    case "loading":
      return (
        <div className={`viewer__state ${viewer.kind === "loading" ? "viewer__state--over" : ""}`} role="status">
          <span className="spinner" aria-hidden="true" />
          <p className="viewer__title">{viewer.kind === "starting" ? t("app.starting") : t("app.loading")}</p>
          {viewer.kind === "starting" && <p className="viewer__text">{t("app.startingHint")}</p>}
        </div>
      );
    case "stopped":
    case "refused":
    case "failed":
      return (
        <div className={`viewer__state viewer__state--${viewer.kind}`} role="alert">
          <Icon name={viewer.kind === "stopped" ? "stop" : "alert"} size={32} />
          <p className="viewer__title">
            {viewer.kind === "stopped" ? t("app.stopped") : viewer.kind === "refused" ? t("app.refused") : t("app.failed")}
          </p>
          <p className="viewer__text">{viewer.kind === "stopped" ? t("app.stoppedHint", { port }) : viewer.reason}</p>
          <button type="button" className={`btn ${viewer.kind === "stopped" ? "btn--primary" : ""}`} onClick={onRetry}>
            {t("error.retry")}
          </button>
        </div>
      );
  }
}

/** The screen: asks the PC to open the app, then watches that its server still listens. */
export function AppScreen({ workspace, port }: { workspace: string; port: number }) {
  const { client, state, apps } = useRemoteContext();
  const { back } = useNavigation();
  const [viewer, setViewer] = useState<Viewer>({ kind: "starting" });
  const [frame, setFrame] = useState(0);
  const [listed, setListed] = useState<App | null>(null);
  const viewerRef = useRef(viewer);
  viewerRef.current = viewer;
  const app = listed ?? apps?.find((entry) => entry.workspaceId === workspace && entry.port === port) ?? null;
  /** Seen listed: the PC no longer offering it means its server stopped. */
  const known = useRef(false);
  known.current ||= app !== null;
  const misses = useRef(0);
  /** The opening whose answer counts: a later one, or leaving, makes an earlier one's moot. */
  const attempt = useRef(0);

  const open = useCallback(async () => {
    const mine = ++attempt.current;
    misses.current = 0;
    setViewer({ kind: "starting" });
    try {
      const { url } = await client.openApp(workspace, port);
      if (mine !== attempt.current) return;
      setViewer({ kind: "loading", url });
      setFrame((count) => count + 1);
    } catch (error) {
      if (mine === attempt.current) setViewer(openFailure(error, port, i18n, known.current));
    }
  }, [client, workspace, port]);

  useEffect(() => {
    void open();
    return () => {
      attempt.current++;
    };
  }, [open]);

  // The frame cannot tell its server stopped; the PC's list of what listens can.
  usePolling(
    async () => {
      let found: App | null;
      try {
        found = (await client.apps(workspace)).apps.find((app) => app.port === port) ?? null;
      } catch {
        // Not asked, or not answered: nothing said about the app.
        return;
      }
      if (found) setListed(found);
      const next = afterListing(viewerRef.current, found !== null, misses.current);
      misses.current = next.misses;
      if (next.viewer !== viewerRef.current) setViewer(next.viewer);
    },
    LISTING_MS,
    viewer.kind === "loading" || viewer.kind === "shown",
  );

  const project = state?.projects.find((entry) => entry.id === workspace);

  return (
    <AppView
      name={app ? appName(app, i18n) : (project?.name ?? workspace)}
      where={app?.workspace ?? ""}
      port={port}
      viewer={viewer}
      frame={frame}
      onBack={() => back({ name: "project", id: workspace })}
      onReload={() => void open()}
      onLoaded={() => setViewer((current) => (current.kind === "loading" ? { kind: "shown", url: current.url } : current))}
    />
  );
}
