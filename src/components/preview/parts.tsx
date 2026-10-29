import { useCallback, useEffect, useState, type ReactNode } from "react";
import { Icon, type IconName } from "../Icon";
import { Glyph, type GlyphName } from "./Glyph";
import { useLocale, useT } from "../../i18n";
import { openExternally, showInFolder } from "../../lib/preview";

/**
 * What every viewer of the preview draws the same way: a message in the
 * middle of the stage, a banner over it, a tool of the bar, a passing word
 * after an action, a size in bytes. The 3D view (ModelPanel) uses them too.
 */

/** Over the stage, centred: `icon` from the app's set, or `glyph` from the preview's. */
export function StageMessage({
  icon,
  glyph,
  tone,
  title,
  detail,
  action,
  onAction,
}: {
  icon?: IconName;
  glyph?: GlyphName;
  tone?: "danger" | "warning";
  title: string;
  detail?: string;
  action?: string;
  onAction?: () => void;
}) {
  return (
    <div className="model-stage__center" role={tone ? "alert" : "status"}>
      <div className={`model-message ${tone ? `model-message--${tone}` : ""}`}>
        {glyph ? (
          <Glyph name={glyph} size={20} className="model-message__icon" />
        ) : (
          <Icon name={icon ?? "alert"} size={20} className="model-message__icon" />
        )}
        <p className="model-message__title">{title}</p>
        {detail && <p className="model-message__detail">{detail}</p>}
        {action && (
          <button type="button" className="btn" onClick={onAction}>
            {action}
          </button>
        )}
      </div>
    </div>
  );
}

export function Banner({
  tone,
  text,
  detail,
  action,
  onAction,
}: {
  tone: "danger" | "warning";
  text: string;
  detail?: string | null;
  action?: string;
  onAction?: () => void;
}) {
  return (
    <div className={`model-banner model-banner--${tone}`} role="alert">
      <Icon name="alert" size={13} className="model-banner__icon" />
      <span className="model-banner__text" title={detail ?? undefined}>
        {text}
        {detail && <span className="model-banner__detail">{detail}</span>}
      </span>
      {action && (
        <button type="button" className="model-banner__action" onClick={onAction}>
          {action}
        </button>
      )}
    </div>
  );
}

/** A button of a viewer's bar, labelled by its tooltip; a toggle when `pressed` is given. */
export function Tool({
  label,
  pressed,
  disabled,
  onClick,
  children,
}: {
  label: string;
  pressed?: boolean;
  disabled?: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      className="model-panel__tool"
      aria-pressed={pressed}
      disabled={disabled}
      onClick={onClick}
      title={label}
      aria-label={label}
    >
      {children}
    </button>
  );
}

export interface Notice {
  text: string;
  error?: boolean;
}

/** A word after an action, gone after a moment — longer for an error, to be read. */
export function useNotice() {
  const [notice, setNotice] = useState<Notice | null>(null);
  useEffect(() => {
    if (!notice) return;
    const timer = window.setTimeout(() => setNotice(null), notice.error ? 6000 : 2200);
    return () => window.clearTimeout(timer);
  }, [notice]);
  return [notice, setNotice] as const;
}

export function NoticeText({ notice }: { notice: Notice | null }) {
  if (!notice) return null;
  return (
    <span className={`model-panel__notice ${notice.error ? "is-error" : ""}`} role="status">
      {!notice.error && <Icon name="check" size={12} />}
      {notice.text}
    </span>
  );
}

/** A size in bytes, in the unit that reads best: "812 bytes", "48.2 KB", "12 MB". */
export function useByteSize() {
  const t = useT();
  const locale = useLocale();
  return useCallback(
    (bytes: number) => {
      const number = (value: number) =>
        new Intl.NumberFormat(locale, { maximumFractionDigits: value < 10 ? 1 : 0 }).format(value);
      if (bytes < 1024) return t("preview.bytes", { size: number(bytes) });
      if (bytes < 1024 * 1024) return t("preview.kilobytes", { size: number(bytes / 1024) });
      return t("model.megabytes", { size: number(bytes / (1024 * 1024)) });
    },
    [t, locale],
  );
}

export const errorText = (error: unknown) => (error instanceof Error ? error.message : String(error));

/** A file on its way: its name over a bar that travels, as the 3D view's first load. */
export function Loading({ title }: { title: string }) {
  const t = useT();
  return (
    <div className="model-stage__center" role="status">
      <div className="model-loading">
        <span className="model-loading__title">{title}</span>
        <div className="model-progress is-indeterminate" aria-hidden="true">
          <span className="model-progress__fill" />
        </div>
        <span className="model-loading__phase">{t("preview.loading")}</span>
      </div>
    </div>
  );
}

/**
 * Hands the file to the app the system opens it with, and says what came of
 * it: opened, shown in its folder instead (lib/preview › openExternally), or
 * why neither could be done.
 */
export function useOpenExternally(path: string, onNotice: (notice: Notice) => void) {
  const t = useT();
  return () => {
    openExternally(path)
      .then((how) => onNotice({ text: t(how === "opened" ? "preview.opened" : "preview.revealed") }))
      .catch((error: unknown) =>
        onNotice({ text: t("preview.openFailed", { error: errorText(error) }), error: true }),
      );
  };
}

/** Shows the file selected in the system's file manager: the way out for a kind Hangar may not hand to an app. */
export function useShowInFolder(path: string, onNotice: (notice: Notice) => void) {
  const t = useT();
  return () => {
    showInFolder(path).catch((error: unknown) =>
      onNotice({ text: t("preview.openFailed", { error: errorText(error) }), error: true }),
    );
  };
}

/** The button that does it, labelled in words: it is the way out when the webview cannot show a file. */
export function OpenExternallyButton({ onClick }: { onClick: () => void }) {
  const t = useT();
  return (
    <button type="button" className="preview-action" onClick={onClick}>
      <Icon name="arrowUpRight" size={13} />
      {t("preview.openDefault")}
    </button>
  );
}
