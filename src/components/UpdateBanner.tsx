import { useEffect, useState } from "react";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

type Stage = "idle" | "available" | "downloading" | "ready" | "failed";

export function UpdateBanner() {
  const [update, setUpdate] = useState<Update | null>(null);
  const [stage, setStage] = useState<Stage>("idle");
  const [progress, setProgress] = useState(0);
  const [error, setError] = useState<string | null>(null);

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
            Version <strong>{update.version}</strong> disponible
          </span>
          <button className="btn btn--ghost" onClick={() => setStage("idle")}>
            Plus tard
          </button>
          <button className="btn btn--primary" onClick={install}>
            Mettre à jour
          </button>
        </>
      )}

      {stage === "downloading" && (
        <span className="update__text">
          Téléchargement… {progress > 0 ? `${progress}%` : ""}
        </span>
      )}

      {stage === "ready" && (
        <>
          <span className="update__text">Mise à jour installée</span>
          <button className="btn btn--primary" onClick={() => relaunch()}>
            Redémarrer
          </button>
        </>
      )}

      {stage === "failed" && (
        <>
          <span className="update__text update__text--error" title={error ?? ""}>
            Échec de la mise à jour
          </span>
          <button className="btn btn--ghost" onClick={() => setStage("available")}>
            Réessayer
          </button>
        </>
      )}
    </div>
  );
}
