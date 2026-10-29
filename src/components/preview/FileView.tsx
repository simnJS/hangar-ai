import type { ComponentType } from "react";
import { HtmlView } from "./HtmlView";
import { ImageView } from "./ImageView";
import { MarkdownView } from "./MarkdownView";
import { MediaView } from "./MediaView";
import { useByteSize } from "./parts";
import { PdfView } from "./PdfView";
import { TextView } from "./TextView";
import { useFileWatch } from "./useFileWatch";
import { useT } from "../../i18n";
import type { OpenFile } from "../../lib/modelStore";
import { mimeType, viewerOf, type PreviewViewer } from "../../lib/preview";

export interface ViewerProps {
  entry: OpenFile;
  /** The panel is on screen: a sound stops when it is not. */
  shown: boolean;
}

/** Each viewer but the 3D one, which ModelPanel keeps mounted for its own reasons. */
const VIEWERS: Record<Exclude<PreviewViewer, "model">, ComponentType<ViewerProps>> = {
  image: ImageView,
  media: MediaView,
  code: TextView,
  markdown: MarkdownView,
  pdf: PdfView,
  html: HtmlView,
};

/**
 * The tab in front, when it is not a 3D model: its viewer (lib/preview ›
 * viewerOf), its bar and stage, and a line of what the file is. The viewer
 * is mounted afresh for each file and each revision of it: an agent showing
 * a file again, or rewriting it while it is on screen (./useFileWatch), has
 * it read from scratch.
 */
export function FileView({ entry, shown }: ViewerProps) {
  const t = useT();
  const missing = useFileWatch(entry, shown);
  const viewer = viewerOf(entry.kind);
  const View = viewer === "model" ? null : VIEWERS[viewer];
  return (
    <div className="preview-file">
      {missing && (
        <p className="preview-note" role="alert" title={missing}>
          {t("model.missing")}
        </p>
      )}
      {View && <View key={`${entry.id}:${entry.revision}`} entry={entry} shown={shown} />}
      <FileInfo entry={entry} />
    </div>
  );
}

function FileInfo({ entry }: { entry: OpenFile }) {
  const t = useT();
  const size = useByteSize();
  const extension = entry.path.match(/\.([^.\\/]+)$/)?.[1];
  return (
    <div className="model-info" aria-label={t(`preview.kind.${entry.kind}`)}>
      <span className="model-info__format">{extension ?? t(`preview.kind.${entry.kind}`)}</span>
      <span className="model-info__item">{t(`preview.kind.${entry.kind}`)}</span>
      <span className="model-info__item">
        <span className="model-info__value">{size(entry.size)}</span>
      </span>
      {entry.mime && <span className="model-info__item model-info__item--size">{mimeType(entry.mime)}</span>}
    </div>
  );
}
