import { NoticeText, OpenExternallyButton, StageMessage, useNotice, useOpenExternally } from "./parts";
import type { ViewerProps } from "./FileView";
import { useT } from "../../i18n";
import { pdfInline, previewUrl } from "../../lib/preview";

/**
 * A PDF, drawn by the webview's own viewer in a frame where it has one
 * (WebView2, WebKit on macOS); where it has none, a plain way out to the
 * system's PDF reader, which the bar offers everywhere. No sandbox on this
 * frame: the webview's PDF viewer does not run in one.
 */
export function PdfView({ entry }: ViewerProps) {
  const t = useT();
  const [notice, setNotice] = useNotice();
  const open = useOpenExternally(entry.path, setNotice);
  const inline = pdfInline(typeof navigator === "undefined" ? undefined : navigator);

  return (
    <>
      <div className="model-panel__bar" role="toolbar" aria-label={t("preview.tools", { kind: t("preview.kind.pdf") })}>
        <OpenExternallyButton onClick={open} />
        <span className="model-panel__spacer" />
        <NoticeText notice={notice} />
      </div>
      <div className="model-panel__stage preview-stage">
        {inline ? (
          <iframe className="preview-frame" src={previewUrl(entry.path, entry.revision)} title={entry.title} />
        ) : (
          <StageMessage
            glyph="pdf"
            title={t("preview.pdfUnsupported")}
            detail={t("preview.pdfUnsupportedHint")}
            action={t("preview.openDefault")}
            onAction={open}
          />
        )}
      </div>
    </>
  );
}
