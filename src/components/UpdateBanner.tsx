import { useEffect, useState } from "react";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { useT } from "../i18n";

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

  return (
    <div className="update">
      {stage === "available" && (
        <>
          <span className="update__text">
            {t("update.available", { version: update.version })}
          </span>
          <button className="btn btn--ghost" onClick={() => setStage("idle")}>
            {t("update.later")}
          </button>
          <button className="btn btn--primary" onClick={install}>
            {t("update.install")}
          </button>
        </>
      )}

      {stage === "downloading" && (
        <span className="update__text">
          {t("update.downloading", { progress: progress > 0 ? `${progress}%` : "" })}
        </span>
      )}

      {stage === "ready" && (
        <>
          <span className="update__text">{t("update.ready")}</span>
          <button className="btn btn--primary" onClick={() => relaunch()}>
            {t("update.restart")}
          </button>
        </>
      )}

      {stage === "failed" && (
        <>
          <span className="update__text update__text--error" title={error ?? ""}>
            {t("update.failed")}
          </span>
          <button className="btn btn--ghost" onClick={() => setStage("available")}>
            {t("update.retry")}
          </button>
        </>
      )}
    </div>
  );
}
