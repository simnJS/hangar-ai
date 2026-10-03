import { useEffect, useMemo, useRef, useState } from "react";
import type { Terminal } from "@xterm/xterm";
import { SearchAddon, type ISearchOptions } from "@xterm/addon-search";
import { formatBinding } from "../lib/keys";
import { useT } from "../i18n";
import { mix, type TerminalTheme } from "../themes";

/**
 * The search bar floating over one terminal.
 *
 * It owns its addon: loaded when the bar opens, disposed — highlights and all —
 * when it closes, so a pane nobody is searching pays nothing for it.
 *
 * The search runs upwards. What anyone looks for in a terminal is nearly always
 * the latest occurrence, at the bottom, so typing lands there and Enter steps
 * back through older output.
 */

/** The addon stops highlighting past this many matches; the count says so. */
const HIGHLIGHT_LIMIT = 1000;

type Decorations = NonNullable<ISearchOptions["decorations"]>;

interface Props {
  term: Terminal;
  theme: TerminalTheme;
  /** Bumped each time the shortcut asks for the bar, already open or not. */
  request: number;
  /** What to look for when asked: the selection, or the previous query. */
  seed: string;
  /** Hands the query back, so the bar reopens on it. */
  onClose: (query: string) => void;
}

interface Results {
  /** -1 while no match is the current one. */
  index: number;
  count: number;
}

function isValidRegex(source: string): boolean {
  try {
    new RegExp(source);
    return true;
  } catch {
    return false;
  }
}

/**
 * Matches in the theme's own yellow, the current one in its accent, both laid
 * over the background so the text on top stays legible in light themes too.
 * The addon only takes #RRGGBB, which is what `mix` returns.
 */
function decorationsFor(theme: TerminalTheme): Decorations {
  const bg = theme.xterm.background ?? "#000000";
  const match = theme.xterm.yellow ?? "#e0af68";
  return {
    matchBackground: mix(bg, match, 0.3),
    matchBorder: mix(bg, match, 0.65),
    matchOverviewRuler: match,
    activeMatchBackground: mix(bg, theme.accent, 0.45),
    activeMatchBorder: theme.accent,
    activeMatchColorOverviewRuler: theme.accent,
  };
}

const Chevron = ({ up }: { up: boolean }) => (
  <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden>
    <path
      d={up ? "M3 7.5 6 4.5l3 3" : "M3 4.5 6 7.5l3-3"}
      fill="none"
      stroke="currentColor"
      strokeWidth="1.4"
      strokeLinecap="round"
      strokeLinejoin="round"
    />
  </svg>
);

const Cross = () => (
  <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden>
    <path
      d="M3.5 3.5l5 5m0-5-5 5"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.4"
      strokeLinecap="round"
    />
  </svg>
);

export function TerminalSearch({ term, theme, request, seed, onClose }: Props) {
  const t = useT();
  const inputRef = useRef<HTMLInputElement>(null);
  const addonRef = useRef<SearchAddon | null>(null);
  const [query, setQuery] = useState(seed);
  const [caseSensitive, setCaseSensitive] = useState(false);
  const [regex, setRegex] = useState(false);
  const [results, setResults] = useState<Results | null>(null);

  const decorations = useMemo(() => decorationsFor(theme), [theme]);
  // The addon hands a pattern straight to RegExp and throws on a broken one,
  // which is every regex halfway through being typed.
  const invalid = regex && query !== "" && !isValidRegex(query);

  useEffect(() => {
    const search = new SearchAddon({ highlightLimit: HIGHLIGHT_LIMIT });
    term.loadAddon(search);
    const listener = search.onDidChangeResults(({ resultIndex, resultCount }) =>
      setResults({ index: resultIndex, count: resultCount }),
    );
    addonRef.current = search;
    return () => {
      listener.dispose();
      search.dispose();
      addonRef.current = null;
    };
  }, [term]);

  // Each request, the first one included, takes the seed if there is one and
  // hands the keyboard to the field, its text selected so typing replaces it.
  useEffect(() => {
    if (seed) setQuery(seed);
    const input = inputRef.current;
    if (!input) return;
    input.focus();
    // After the render that writes the seed in: setting the value moves the
    // caret to the end and would drop a selection made now.
    const frame = requestAnimationFrame(() => input.select());
    return () => cancelAnimationFrame(frame);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [request]);

  useEffect(() => {
    const search = addonRef.current;
    if (!search) return;
    if (!query || invalid) {
      search.clearDecorations();
      setResults(null);
      return;
    }
    search.findPrevious(query, { caseSensitive, regex, decorations, incremental: true });
  }, [query, caseSensitive, regex, decorations, invalid]);

  function step(up: boolean) {
    const search = addonRef.current;
    if (!search || !query || invalid) return;
    const options: ISearchOptions = { caseSensitive, regex, decorations };
    if (up) search.findPrevious(query, options);
    else search.findNext(query, options);
  }

  /**
   * Buttons never keep the focus: the field does. Without this a click on a
   * toggle would leave Enter and Escape with nothing to act on.
   */
  const refocus = () => inputRef.current?.focus();

  const missed = invalid || (query !== "" && !results?.count);
  let status = "";
  if (invalid) status = t("search.invalid");
  else if (missed) status = t("search.none");
  else if (results && results.count > 0) {
    const total = results.count >= HIGHLIGHT_LIMIT ? `${HIGHLIGHT_LIMIT}+` : `${results.count}`;
    status = results.index >= 0 ? `${results.index + 1}/${total}` : total;
  }

  return (
    <div className="term-search" role="search" aria-label={t("search.label")}>
      <input
        ref={inputRef}
        className="term-search__input"
        value={query}
        placeholder={t("search.placeholder")}
        spellCheck={false}
        aria-invalid={invalid}
        onChange={(event) => setQuery(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Escape") {
            event.preventDefault();
            onClose(query);
          } else if (event.key === "Enter") {
            event.preventDefault();
            step(!event.shiftKey);
          } else if (event.altKey && event.code === "KeyC") {
            event.preventDefault();
            setCaseSensitive((on) => !on);
          } else if (event.altKey && event.code === "KeyR") {
            event.preventDefault();
            setRegex((on) => !on);
          }
        }}
      />
      <span
        className={`term-search__count ${missed ? "is-missed" : ""}`}
        aria-live="polite"
      >
        {status}
      </span>
      <button
        className={`term-search__opt ${caseSensitive ? "is-on" : ""}`}
        aria-pressed={caseSensitive}
        onMouseDown={(event) => event.preventDefault()}
        onClick={() => {
          setCaseSensitive((on) => !on);
          refocus();
        }}
        title={t("search.case", { keys: formatBinding("Alt+C") })}
      >
        Aa
      </button>
      <button
        className={`term-search__opt ${regex ? "is-on" : ""}`}
        aria-pressed={regex}
        onMouseDown={(event) => event.preventDefault()}
        onClick={() => {
          setRegex((on) => !on);
          refocus();
        }}
        title={t("search.regex", { keys: formatBinding("Alt+R") })}
      >
        .*
      </button>
      <span className="term-search__sep" aria-hidden />
      <button
        className="term-search__btn"
        onMouseDown={(event) => event.preventDefault()}
        onClick={() => {
          step(true);
          refocus();
        }}
        disabled={!query || invalid}
        title={t("search.up", { keys: formatBinding("Enter") })}
      >
        <Chevron up />
      </button>
      <button
        className="term-search__btn"
        onMouseDown={(event) => event.preventDefault()}
        onClick={() => {
          step(false);
          refocus();
        }}
        disabled={!query || invalid}
        title={t("search.down", { keys: formatBinding("Shift+Enter") })}
      >
        <Chevron up={false} />
      </button>
      <button
        className="term-search__btn"
        onClick={() => onClose(query)}
        title={t("search.close", { keys: formatBinding("Escape") })}
      >
        <Cross />
      </button>
    </div>
  );
}
