import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type { Translator } from "../../i18n";
import { voiceModelDownload, voiceModels } from "../../lib/ipc";
import type { VoiceDownload, VoiceModel } from "../../lib/voice";

/**
 * The local model, and how to get it.
 *
 * Worth its own block rather than a settings row: a model is half a gigabyte
 * that has to arrive before dictation can do anything, and the two questions
 * that follow from it — is it here yet, and how far along is it — are not
 * things a label and a control can answer.
 */

function formatBytes(bytes: number): string {
  const mb = bytes / (1024 * 1024);
  return mb >= 1024 ? `${(mb / 1024).toFixed(1)} GB` : `${Math.round(mb)} MB`;
}

export function VoiceCard({ t, selected }: { t: Translator; selected: string }) {
  const [models, setModels] = useState<VoiceModel[]>([]);
  const [progress, setProgress] = useState<VoiceDownload | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = () =>
    voiceModels()
      .then(setModels)
      .catch(() => setModels([]));

  useEffect(() => {
    refresh();
  }, []);

  useEffect(() => {
    let off: (() => void) | null = null;
    let cancelled = false;
    listen<VoiceDownload>("voice:download", (event) => {
      setProgress(event.payload.done ? null : event.payload);
      // A finished download changes what the rest of the page may offer, so the
      // catalogue is re-read rather than patched in place.
      if (event.payload.done) refresh();
    })
      .then((stop) => {
        if (cancelled) stop();
        else off = stop;
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
      off?.();
    };
  }, []);

  const model = models.find((entry) => entry.id === selected) ?? models[0] ?? null;
  if (!model) return null;

  const downloading = progress?.id === model.id;
  const percent = downloading
    ? Math.min(100, Math.round((progress.received / Math.max(1, progress.total)) * 100))
    : 0;

  const start = () => {
    setError(null);
    setProgress({ id: model.id, received: 0, total: model.approxBytes, done: false });
    voiceModelDownload(model.id)
      .catch((err) => setError(String(err)))
      .finally(() => {
        setProgress(null);
        refresh();
      });
  };

  return (
    <div className="vm">
      <div className="vm__head">
        <strong className="vm__name">{model.label}</strong>
        <span className={`vm__state ${model.installed ? "is-ready" : ""}`}>
          {model.installed ? t("settings.voiceInstalled") : formatBytes(model.approxBytes)}
        </span>
      </div>

      <p className="vm__meta">
        {t("settings.voiceModelHint", {
          // Shown as codes rather than names: twenty-five language names is a
          // paragraph, and the codes are what you would type in the language
          // field right below anyway.
          languages: model.languages.split(" ").join(", "),
          license: model.license,
        })}
      </p>

      {downloading ? (
        <>
          <div
            className="vm__bar"
            role="progressbar"
            aria-valuenow={percent}
            aria-valuemin={0}
            aria-valuemax={100}
          >
            <span className="vm__bar-fill" style={{ width: `${percent}%` }} />
          </div>
          <p className="vm__note">{t("settings.voiceDownloading", { percent })}</p>
        </>
      ) : (
        <div className="vm__actions">
          <button type="button" className="btn btn--primary" onClick={start}>
            {model.installed
              ? t("settings.voiceDownloadAgain")
              : t("settings.voiceDownload", { size: formatBytes(model.approxBytes) })}
          </button>
          {!model.installed && <span className="vm__note">{t("settings.voiceMissing")}</span>}
        </div>
      )}

      {error && <p className="vm__note vm__note--error">{error}</p>}
    </div>
  );
}
