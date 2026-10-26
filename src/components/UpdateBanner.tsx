import { useEffect, useState } from "react";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { useT } from "../i18n";
import { Glyph } from "./settings/icons";

type Stage = "idle" | "available" | "downloading" | "ready" | "failed";

export function UpdateBanner() {
  const [update, setUpdate] = useState<Update | null>(null);
  const [stage, setStage] = useState<Stage>("idle");
  const [progress, setProgress] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const t = useT();

  useEffect(() => {
    let cancelled = false;
    // Silent on failure: no network, or running a dev build with no release
    // feed, should never surface an error to the user.
    check()
      .then((found) => {
        if (cancelled || !found) return;
        setUpdate(found);
        setStage("available");
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, []);

  async function install() {
    if (!update) return;
    setStage("downloading");
    setError(null);

    let total = 0;
    let received = 0;

    try {
      await update.downloadAndInstall((event) => {
        switch (event.event) {
          case "Started":
            total = event.data.contentLength ?? 0;
            break;
          case "Progress":
            received += event.data.chunkLength;
            if (total > 0) setProgress(Math.round((received / total) * 100));
            break;
          case "Finished":
            setProgress(100);
            break;
        }
      });
      setStage("ready");
    } catch (err) {
      setError(String(err));
      setStage("failed");
    }
  }

  if (stage === "idle" || !update) return null;

  // A capsule floating over the corner of the window: it never reflows the
  // panes when it appears, and it stays out of the way of the status bar.
  return (
    <div className={`update update--${stage}`} role="status">
      <Glyph
        name={stage === "failed" ? "alert" : stage === "ready" ? "check" : "download"}
        className="update__icon"
      />

      {stage === "available" && (
        <>
          <span className="update__text">
            {t("update.available", { version: update.version })}
          </span>
          <button type="button" className="btn btn--ghost" onClick={() => setStage("idle")}>
            {t("update.later")}
          </button>
          <button type="button" className="btn btn--primary" onClick={install}>
            {t("update.install")}
          </button>
        </>
      )}

      {stage === "downloading" && (
        <>
          <span className="update__text">
            {t("update.downloading", { progress: progress > 0 ? `${progress}%` : "" })}
          </span>
          <span
            className="update__bar"
            role="progressbar"
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={progress}
          >
            <i style={{ width: `${progress}%` }} />
          </span>
        </>
      )}

      {stage === "ready" && (
        <>
          <span className="update__text">{t("update.ready")}</span>
          <button type="button" className="btn btn--primary" onClick={() => relaunch()}>
            {t("update.restart")}
          </button>
        </>
      )}

      {stage === "failed" && (
        <>
          <span className="update__text update__text--error" title={error ?? ""}>
            {t("update.failed")}
          </span>
          <button type="button" className="btn btn--ghost" onClick={() => setStage("available")}>
            {t("update.retry")}
          </button>
        </>
      )}
    </div>
  );
}
