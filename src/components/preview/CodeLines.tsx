import { useEffect, useMemo, useState, type CSSProperties } from "react";
import { textLines } from "../../lib/preview";

/**
 * Text as numbered lines, coloured once the highlighter has arrived (plain
 * until then, never empty). Each line is a row beside its number, so a long
 * line wraps under itself and the numbers stay with their lines. The numbers
 * are not part of a selection: what is copied is the text.
 */
export function CodeLines({
  text,
  language,
  wrap,
  label,
}: {
  text: string;
  /** A highlight.js language or alias; null for plain text. */
  language: string | null;
  wrap: boolean;
  label: string;
}) {
  const lines = useMemo(() => textLines(text), [text]);
  const coloured = useHighlighted(text, language);
  const digits = String(lines.length).length;
  return (
    <div
      className={`preview-code ${wrap ? "is-wrapped" : ""}`}
      // Focusable, so the keyboard scrolls it.
      tabIndex={0}
      role="region"
      aria-label={label}
      style={{ "--digits": digits } as CSSProperties}
    >
      {lines.map((line, i) => (
        <div className="preview-code__line" key={i}>
          <span className="preview-code__no" aria-hidden="true">
            {i + 1}
          </span>
          {coloured ? (
            <code className="preview-code__text hljs" dangerouslySetInnerHTML={{ __html: coloured[i] ?? "" }} />
          ) : (
            <code className="preview-code__text">{line}</code>
          )}
        </div>
      ))}
    </div>
  );
}

/** The lines of `text` as highlighted HTML (./highlight escapes them), or null while it loads or for plain text. */
function useHighlighted(text: string, language: string | null): string[] | null {
  const [result, setResult] = useState<{ text: string; language: string; lines: string[] } | null>(null);
  useEffect(() => {
    if (!language) return;
    let live = true;
    import("./highlight")
      .then(({ highlightLines }) => {
        const lines = highlightLines(text, language);
        if (live && lines) setResult({ text, language, lines });
      })
      // Shown plain: the text is all there either way.
      .catch(() => undefined);
    return () => {
      live = false;
    };
  }, [text, language]);
  return result && result.text === text && result.language === language ? result.lines : null;
}
