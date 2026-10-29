import { useState } from "react";
import { Icon } from "../Icon";
import { CodeLines } from "./CodeLines";
import { Glyph } from "./Glyph";
import { NoticeText, Tool, useNotice } from "./parts";
import { CopyTool, TextStage, useWrap, WrapTool } from "./TextParts";
import { useTextHead } from "./useTextHead";
import type { ViewerProps } from "./FileView";
import { useT } from "../../i18n";
import { previewUrl } from "../../lib/preview";

/**
 * An HTML page an agent made, running — in a frame that is sandboxed, never
 * with `allow-same-origin`: its scripts run in an origin of their own, which
 * reaches neither Hangar's window nor its commands, nor the files beside it
 * through script. The scheme adds a CSP sandbox of its own (src-tauri). The
 * bar says so in words, and offers a reload and the page's source, which the
 * scheme serves as plain text when asked `?as=text`. A note says what such a
 * page cannot do: load its own scripts and styles from files beside it.
 */
export function HtmlView({ entry }: ViewerProps) {
  const t = useT();
  const [source, setSource] = useState(false);
  const [reloads, setReloads] = useState(0);
  const [wrap, toggleWrap] = useWrap();
  const [notice, setNotice] = useNotice();
  const head = useTextHead(source ? previewUrl(entry.path, entry.revision, true) : null);
  const text = source && head.status === "ready" ? head.text : null;

  const reload = () => {
    setReloads((n) => n + 1);
    if (source) head.retry();
  };

  return (
    <>
      <div className="model-panel__bar" role="toolbar" aria-label={t("preview.tools", { kind: t("preview.kind.html") })}>
        <Tool label={t("preview.reload")} onClick={reload}>
          <Icon name="restart" size={14} />
        </Tool>
        <Tool label={t("preview.sourceHint")} pressed={source} onClick={() => setSource((now) => !now)}>
          <Glyph name="code" size={14} />
        </Tool>
        {source && <WrapTool wrap={wrap} onToggle={toggleWrap} />}
        {source && <CopyTool text={text} onNotice={setNotice} />}
        <span className="model-panel__spacer" />
        <NoticeText notice={notice} />
        <span className="preview-isolated" title={t("preview.isolatedHint")}>
          <Icon name="lock" size={12} />
          {t("preview.isolated")}
        </span>
      </div>
      {source ? (
        <TextStage state={head} title={entry.title}>
          <CodeLines
            text={text ?? ""}
            language="xml"
            wrap={wrap}
            label={t("preview.codeLabel", { title: entry.title, language: "HTML" })}
          />
        </TextStage>
      ) : (
        <div className="model-panel__stage preview-stage">
          {/* Served as text with nosniff, its own .js and .css are refused (src-tauri/preview.rs). */}
          <p className="preview-note preview-note--quiet" role="note">
            {t("preview.inlineOnly")}
          </p>
          <iframe
            key={reloads}
            className="preview-frame preview-frame--page"
            src={previewUrl(entry.path, entry.revision)}
            title={entry.title}
            sandbox="allow-scripts allow-forms allow-popups"
            referrerPolicy="no-referrer"
          />
        </div>
      )}
    </>
  );
}
