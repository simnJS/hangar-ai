import { useEffect, useRef, useState } from "react";
import { Glyph } from "./Glyph";
import { NoticeText, StageMessage, Tool, useNotice, useShowInFolder } from "./parts";
import type { ViewerProps } from "./FileView";
import { useLocale, useT } from "../../i18n";
import { previewUrl } from "../../lib/preview";

/** From half speed to double: past that, speech is noise and frames are skipped. */
const SPEEDS = [0.5, 0.75, 1, 1.25, 1.5, 2];

/**
 * A video or a sound, in the webview's own player, with what it does not
 * offer itself: playing in a loop, and at another speed. A video fits the
 * panel. Playback stops when the panel leaves the screen — a sound playing
 * from a hidden panel would have nothing to stop it.
 */
export function MediaView({ entry, shown }: ViewerProps) {
  const t = useT();
  const locale = useLocale();
  const ref = useRef<HTMLVideoElement & HTMLAudioElement>(null);
  const [loop, setLoop] = useState(false);
  const [rate, setRate] = useState(1);
  const [failed, setFailed] = useState<string | null>(null);
  const [notice, setNotice] = useNotice();
  const reveal = useShowInFolder(entry.path, setNotice);
  const video = entry.kind === "video";

  // The default rate too: a new source would otherwise start back at 1×.
  useEffect(() => {
    const media = ref.current;
    if (!media) return;
    media.defaultPlaybackRate = rate;
    media.playbackRate = rate;
  }, [rate]);

  useEffect(() => {
    if (!shown) ref.current?.pause();
  }, [shown]);

  const speed = (value: number) =>
    `${new Intl.NumberFormat(locale, { maximumFractionDigits: 2 }).format(value)}×`;
  const src = previewUrl(entry.path, entry.revision);
  // The webview's reason, when it gives one, after the one a person can act on.
  const onError = () => setFailed(ref.current?.error?.message ?? "");

  return (
    <>
      <div
        className="model-panel__bar"
        role="toolbar"
        aria-label={t("preview.tools", { kind: t(video ? "preview.kind.video" : "preview.kind.audio") })}
      >
        <Tool label={t("preview.loop")} pressed={loop} disabled={failed !== null} onClick={() => setLoop((now) => !now)}>
          <Glyph name="loop" size={14} />
        </Tool>
        <select
          className="model-anim__clip preview-speed"
          value={rate}
          disabled={failed !== null}
          onChange={(event) => setRate(Number(event.target.value))}
          aria-label={t("preview.speed")}
          title={t("preview.speed")}
        >
          {SPEEDS.map((value) => (
            <option key={value} value={value}>
              {speed(value)}
            </option>
          ))}
        </select>
        <span className="model-panel__spacer" />
        <NoticeText notice={notice} />
      </div>
      <div className={`model-panel__stage preview-stage ${video ? "preview-stage--video" : ""}`}>
        {failed !== null ? (
          <StageMessage
            tone="warning"
            icon="alert"
            title={t("preview.mediaFailed")}
            detail={failed ? `${t("preview.mediaFailedHint")} (${failed})` : t("preview.mediaFailedHint")}
            action={t("preview.showInFolder")}
            onAction={reveal}
          />
        ) : video ? (
          <video
            ref={ref}
            className="preview-video"
            src={src}
            controls
            loop={loop}
            playsInline
            preload="metadata"
            onError={onError}
            aria-label={entry.title}
          />
        ) : (
          <div className="preview-audio">
            <Glyph name="audio" size={40} className="preview-audio__mark" />
            <span className="preview-audio__title" title={entry.path}>
              {entry.title}
            </span>
            <audio
              ref={ref}
              className="preview-audio__player"
              src={src}
              controls
              loop={loop}
              preload="metadata"
              onError={onError}
              aria-label={entry.title}
            />
          </div>
        )}
      </div>
    </>
  );
}
