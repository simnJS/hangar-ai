import { useEffect, useRef, useState, type MouseEvent } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { CodeLines } from "./CodeLines";
import { Glyph } from "./Glyph";
import { errorText, Loading, NoticeText, Tool, useNotice } from "./parts";
import { CopyTool, TextStage, useWrap, WrapTool } from "./TextParts";
import { useTextHead } from "./useTextHead";
import type { ViewerProps } from "./FileView";
import { useT } from "../../i18n";
import { modelUrl } from "../../lib/modelFiles";
import { markdownLink, previewUrl } from "../../lib/preview";

/**
 * A Markdown file, rendered (./markdown: no raw HTML, images from beside the
 * file, code coloured), or its source on demand. A link never takes the
 * app's window anywhere: a heading of the page scrolls to it, a web address
 * opens in the browser, a file beside it does nothing (lib/preview ›
 * markdownLink).
 */
export function MarkdownView({ entry }: ViewerProps) {
  const t = useT();
  const head = useTextHead(previewUrl(entry.path, entry.revision));
  const [source, setSource] = useState(false);
  const [wrap, toggleWrap] = useWrap();
  const [notice, setNotice] = useNotice();
  const text = head.status === "ready" ? head.text : null;
  const html = useRendered(source ? null : text, modelUrl(entry.path));
  const pageRef = useRef<HTMLDivElement>(null);

  function follow(event: MouseEvent<HTMLDivElement>) {
    const anchor = (event.target as Element).closest("a");
    if (!anchor) return;
    event.preventDefault();
    if (event.type !== "click") return;
    const href = anchor.getAttribute("href") ?? "";
    const link = markdownLink(href);
    if (link.kind === "anchor") {
      pageRef.current?.querySelector(`[id="${CSS.escape(link.id)}"]`)?.scrollIntoView({ block: "start" });
    } else if (link.kind === "external") {
      openUrl(link.url).catch((error: unknown) =>
        setNotice({ text: t("preview.openFailed", { error: errorText(error) }), error: true }),
      );
    } else {
      setNotice({ text: t("preview.linkInert", { href }), error: true });
    }
  }

  return (
    <>
      <div className="model-panel__bar" role="toolbar" aria-label={t("preview.tools", { kind: t("preview.kind.markdown") })}>
        <Tool label={t("preview.sourceHint")} pressed={source} onClick={() => setSource((now) => !now)}>
          <Glyph name="code" size={14} />
        </Tool>
        {source && <WrapTool wrap={wrap} onToggle={toggleWrap} />}
        <CopyTool text={text} onNotice={setNotice} />
        <span className="model-panel__spacer" />
        <NoticeText notice={notice} />
      </div>
      <TextStage state={head} title={entry.title}>
        {source ? (
          <CodeLines
            text={text ?? ""}
            language="markdown"
            wrap={wrap}
            label={t("preview.codeLabel", { title: entry.title, language: t("preview.kind.markdown") })}
          />
        ) : html === null ? (
          <Loading title={entry.title} />
        ) : (
          <div
            ref={pageRef}
            className="preview-markdown"
            tabIndex={0}
            role="document"
            aria-label={entry.title}
            onClick={follow}
            onAuxClick={follow}
            // ./markdown renders with html: false — the file's own markup is text.
            dangerouslySetInnerHTML={{ __html: html }}
          />
        )}
      </TextStage>
    </>
  );
}

/** `text` rendered once the parser has arrived; null until then, or for no text. */
function useRendered(text: string | null, fileUrl: string): string | null {
  const [result, setResult] = useState<{ text: string; html: string } | null>(null);
  useEffect(() => {
    if (text === null) return;
    let live = true;
    import("./markdown")
      .then(({ renderMarkdown }) => {
        if (live) setResult({ text, html: renderMarkdown(text, fileUrl) });
      })
      .catch(() => undefined);
    return () => {
      live = false;
    };
  }, [text, fileUrl]);
  return result && result.text === text ? result.html : null;
}
