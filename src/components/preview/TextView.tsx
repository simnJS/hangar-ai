import { CodeLines } from "./CodeLines";
import { NoticeText, useNotice } from "./parts";
import { CopyTool, TextStage, useWrap, WrapTool } from "./TextParts";
import { useTextHead } from "./useTextHead";
import type { ViewerProps } from "./FileView";
import { useT } from "../../i18n";
import { languageOf, prettyJson, previewUrl } from "../../lib/preview";

/**
 * A text or code file: its head, coloured by its extension, with numbered
 * lines; a wrap toggle and a copy button. JSON written on one line is laid
 * out for reading (lib/preview › prettyJson), and says so.
 */
export function TextView({ entry }: ViewerProps) {
  const t = useT();
  const head = useTextHead(previewUrl(entry.path, entry.revision));
  const language = languageOf(entry.path);
  const [wrap, toggleWrap] = useWrap();
  const [notice, setNotice] = useNotice();
  const ready = head.status === "ready";
  const pretty = ready && !head.truncated && language === "json" ? prettyJson(head.text) : null;
  const text = ready ? (pretty ?? head.text) : null;

  return (
    <>
      <div className="model-panel__bar" role="toolbar" aria-label={t("preview.tools", { kind: t("preview.kind.text") })}>
        <WrapTool wrap={wrap} onToggle={toggleWrap} />
        <CopyTool text={text} onNotice={setNotice} />
        <span className="model-panel__spacer" />
        <NoticeText notice={notice} />
        {pretty && (
          <span className="preview-chip" title={t("preview.prettyHint")}>
            {t("preview.pretty")}
          </span>
        )}
        {language && <span className="preview-chip preview-chip--quiet">{language}</span>}
      </div>
      <TextStage state={head} title={entry.title}>
        <CodeLines
          text={text ?? ""}
          language={language}
          wrap={wrap}
          label={t("preview.codeLabel", { title: entry.title, language: language ?? t("preview.kind.text") })}
        />
      </TextStage>
    </>
  );
}
