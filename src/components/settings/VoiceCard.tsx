import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type { Translator } from "../../i18n";
import {
  voiceKeyClear,
  voiceKeySet,
  voiceKeyStatus,
  voiceModelDownload,
  voiceModels,
} from "../../lib/ipc";
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
          languages: model.languages,
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

/**
 * The Groq key, which lives in the system keychain.
 *
 * A card rather than a text field: a field would have to hold the key to show
 * it, and the window is not given it back once saved. So it says whether one
 * is there, and offers to replace or clear it.
 */
export function VoiceKeyCard({
  t,
  legacyKey,
  dropLegacy,
}: {
  t: Translator;
  /** A plaintext key an older version left in state.json, still there only
      because the keychain refused it at launch. */
  legacyKey: string;
  dropLegacy: () => void;
}) {
  /** `null` until the keychain answers, and while it cannot be reached. */
  const [saved, setSaved] = useState<boolean | null>(null);
  const [unavailable, setUnavailable] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [replacing, setReplacing] = useState(false);
  const [draft, setDraft] = useState("");
  const [armed, setArmed] = useState(false);
  const [busy, setBusy] = useState(false);

  const refresh = () =>
    voiceKeyStatus()
      .then((present) => {
        setSaved(present);
        setUnavailable(null);
      })
      .catch((err) => {
        setSaved(null);
        setUnavailable(String(err));
      });

  useEffect(() => {
    refresh();
  }, []);

  async function act(action: () => Promise<void>) {
    setBusy(true);
    setError(null);
    try {
      await action();
      return true;
    } catch (err) {
      setError(String(err));
      return false;
    } finally {
      setBusy(false);
      refresh();
    }
  }

  async function save(key: string) {
    const ok = await act(() => voiceKeySet(key.trim()));
    if (!ok) return;
    setDraft("");
    setReplacing(false);
    // Whatever the keychain now holds supersedes the plaintext copy.
    if (legacyKey) dropLegacy();
  }

  async function clear() {
    setArmed(false);
    await act(voiceKeyClear);
  }

  const asking = saved === false || replacing;

  return (
    <div className="vm">
      <div className="vm__head">
        <span className={`vm__state ${saved ? "is-ready" : ""}`}>
          {unavailable
            ? t("settings.voiceKeyUnavailableShort")
            : saved === null
              ? ""
              : saved
                ? t("settings.voiceKeySaved")
                : t("settings.voiceKeyNone")}
        </span>
      </div>

      <p className="vm__meta">{t("settings.voiceKeyHint")}</p>

      {unavailable && (
        <p className="vm__note vm__note--error">
          {t("settings.voiceKeyUnavailable", { error: unavailable })}
        </p>
      )}

      {legacyKey !== "" && (
        <>
          <p className="vm__note vm__note--error">{t("settings.voiceKeyLegacy")}</p>
          <div className="vm__actions">
            {!unavailable && (
              <button
                type="button"
                className="btn btn--primary"
                disabled={busy}
                onClick={() => save(legacyKey)}
              >
                {t("settings.voiceKeyLegacyMove")}
              </button>
            )}
            <button type="button" className="btn btn--danger" onClick={dropLegacy}>
              {t("settings.voiceKeyLegacyDrop")}
            </button>
          </div>
        </>
      )}

      {!unavailable && asking && (
        <div className="vm__actions">
          <input
            type="password"
            className="set-input"
            style={{ flex: "1 1 220px", width: "auto" }}
            value={draft}
            placeholder={t("settings.voiceKeyPlaceholder")}
            aria-label={t("settings.voiceKey")}
            autoComplete="off"
            spellCheck={false}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && draft.trim()) save(draft);
            }}
          />
          <button
            type="button"
            className="btn btn--primary"
            disabled={busy || !draft.trim()}
            onClick={() => save(draft)}
          >
            {t("settings.voiceKeySave")}
          </button>
          {replacing && (
            <button
              type="button"
              className="btn btn--ghost"
              onClick={() => {
                setReplacing(false);
                setDraft("");
              }}
            >
              {t("create.cancel")}
            </button>
          )}
        </div>
      )}

      {!unavailable && saved && !replacing && (
        <div className="vm__actions">
          <button type="button" className="btn" onClick={() => setReplacing(true)}>
            {t("settings.voiceKeyReplace")}
          </button>
          {armed ? (
            <>
              <button type="button" className="btn btn--ghost" onClick={() => setArmed(false)}>
                {t("create.cancel")}
              </button>
              <button type="button" className="btn btn--danger" disabled={busy} onClick={clear}>
                {t("settings.voiceKeyClearConfirm")}
              </button>
            </>
          ) : (
            <button type="button" className="btn" onClick={() => setArmed(true)}>
              {t("settings.voiceKeyClear")}
            </button>
          )}
        </div>
      )}

      {error && <p className="vm__note vm__note--error">{error}</p>}
    </div>
  );
}
