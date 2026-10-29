import { useState, type ReactNode } from "react";
import { Icon } from "../Icon";
import { Glyph } from "./Glyph";
import { errorText, Loading, StageMessage, Tool, useByteSize, type Notice } from "./parts";
import type { TextState } from "./useTextHead";
import { useT } from "../../i18n";
import { TEXT_HEAD_LINES } from "../../lib/preview";

/**
 * What the text viewers share — text and code, Markdown, a page's source:
 * the stage while the head of the file loads or fails, the word that says
 * only its start is shown, and the wrap and copy tools.
 */

/** The stage of a text viewer: `children` once there is text to draw. */
export function TextStage({
  state,
  title,
  children,
}: {
  state: TextState & { retry: () => void };
  title: string;
  children: ReactNode;
}) {
  const t = useT();
  const size = useByteSize();
  return (
    <div className="model-panel__stage preview-stage">
      {state.status === "loading" ? (
        <Loading title={title} />
      ) : state.status === "error" ? (
        <StageMessage
          tone="danger"
          title={t("preview.loadFailed")}
          detail={state.error}
          action={t("preview.retry")}
          onAction={state.retry}
        />
      ) : !state.text.trim() && !state.truncated ? (
        <StageMessage glyph="text" title={t("preview.emptyFile")} />
      ) : (
        <>
          {state.truncated && (
            <p className="preview-note" role="note">
              {state.read < state.total
                ? t("preview.head", { shown: size(state.read), total: size(state.total) })
                : t("preview.headLines", { n: TEXT_HEAD_LINES })}
            </p>
          )}
          {children}
        </>
      )}
    </div>
  );
}

/** Wrapping is one choice for every text tab, kept while the app runs. */
let wrapPreferred = false;

export function useWrap() {
  const [wrap, setWrap] = useState(wrapPreferred);
  const toggle = () =>
    setWrap((now) => {
      wrapPreferred = !now;
      return !now;
    });
  return [wrap, toggle] as const;
}

export function WrapTool({ wrap, onToggle }: { wrap: boolean; onToggle: () => void }) {
  const t = useT();
  return (
    <Tool label={t("preview.wrap")} pressed={wrap} onClick={onToggle}>
      <Glyph name="wrap" size={14} />
    </Tool>
  );
}

export function CopyTool({
  text,
  onNotice,
}: {
  /** What is copied: the text as drawn; null while there is none. */
  text: string | null;
  onNotice: (notice: Notice) => void;
}) {
  const t = useT();
  const copy = () => {
    if (text === null) return;
    navigator.clipboard
      .writeText(text)
      .then(() => onNotice({ text: t("preview.copied") }))
      .catch((error: unknown) =>
        onNotice({ text: t("preview.copyFailed", { error: errorText(error) }), error: true }),
      );
  };
  return (
    <Tool label={t("preview.copy")} disabled={!text} onClick={copy}>
      <Icon name="copy" size={14} />
    </Tool>
  );
}
