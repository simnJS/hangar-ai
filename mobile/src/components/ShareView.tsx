import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode, type RefObject } from "react";
import type { Share } from "../api";
import { ago, fileSize } from "../format";
import { highlight, type Language } from "../highlight";
import { locale, t } from "../i18n";
import { useRemoteContext } from "../remote";
import { extensionOf, languageOf, TEXT_HEAD_BYTES, viewerOf, wrapsByDefault } from "../viewer";
import { Markdown } from "./Markdown";

/**
 * One share, in the viewer of its kind (viewer.ts): a picture to zoom into,
 * a player, a text coloured and folded, Markdown drawn, a PDF that opens in
 * the phone's own viewer, a page in its sandbox. Every card ends the same
 * way: who shared it and when, and Open.
 */
export function ShareView({ share, now, onZoom }: { share: Share; now: number; onZoom: (share: Share) => void }) {
  const { client } = useRemoteContext();
  const url = client.fileUrl(share.id);
  switch (viewerOf(share)) {
    case "image":
      return <Picture share={share} url={url} now={now} onZoom={onZoom} />;
    case "video":
      return <Video share={share} url={url} now={now} />;
    case "audio":
      return <Audio share={share} url={url} now={now} />;
    case "text":
      return <TextFile share={share} url={url} now={now} />;
    case "markdown":
      return <MarkdownFile share={share} url={url} now={now} />;
    case "pdf":
      return <Pdf share={share} url={url} now={now} />;
    case "html":
      return <Page share={share} url={url} now={now} />;
    default:
      return <File share={share} url={url} now={now} />;
  }
}

interface ViewerProps {
  share: Share;
  url: string;
  now: number;
}

const from = (share: Share, now: number) =>
  `${t("shares.from", { pane: share.pane, workspace: share.workspace })}, ${ago(share.at, now, locale)}`;

/** Who shared it and when, and the one thing to do: open it. */
function Foot({ share, url, now }: ViewerProps) {
  return (
    <div className="share__foot">
      <span className="share__meta">{from(share, now)}</span>
      <a className="btn" href={url} target="_blank" rel="noopener" aria-label={`${t("shares.open")} ${share.name}`}>
        {t("shares.open")}
      </a>
    </div>
  );
}

/** A file's badge, name, caption and size. */
function FileLine({ share }: { share: Share }) {
  return (
    <>
      <span className="file__type" aria-hidden="true">
        {extensionOf(share.name)}
      </span>
      <span className="file__main">
        <span className="file__name">{share.name}</span>
        {share.caption && <span className="file__caption">{share.caption}</span>}
        <span className="file__size">{fileSize(share.size, locale)}</span>
      </span>
    </>
  );
}

function Picture({ share, url, now, onZoom }: ViewerProps & { onZoom: (share: Share) => void }) {
  return (
    <figure>
      <button type="button" className="share__image" onClick={() => onZoom(share)} aria-label={t("shares.zoom", { name: share.name })}>
        <img src={url} alt={share.caption ?? share.name} loading="lazy" decoding="async" />
      </button>
      <figcaption className="share__body">
        <p className="share__caption">{share.caption ?? share.name}</p>
        <Foot share={share} url={url} now={now} />
      </figcaption>
    </figure>
  );
}

/** Its first frame shown before it plays: Safari draws none without a start time. */
function Video({ share, url, now }: ViewerProps) {
  return (
    <figure>
      <video className="share__video" src={`${url}#t=0.001`} controls playsInline preload="metadata" aria-label={share.caption ?? share.name} />
      <figcaption className="share__body">
        <p className="share__caption">{share.caption ?? share.name}</p>
        <Foot share={share} url={url} now={now} />
      </figcaption>
    </figure>
  );
}

function Audio({ share, url, now }: ViewerProps) {
  return (
    <div className="share__body">
      <div className="file">
        <FileLine share={share} />
      </div>
      <audio className="share__audio" src={url} controls preload="metadata" aria-label={share.name} />
      <Foot share={share} url={url} now={now} />
    </div>
  );
}

/** The phone's own PDF viewer, full screen, shows every page; an iframe would show the first alone. */
function Pdf({ share, url, now }: ViewerProps) {
  return (
    <div className="share__body">
      <a className="file file--link" href={url} target="_blank" rel="noopener" tabIndex={-1} aria-hidden="true">
        <FileLine share={share} />
      </a>
      <Foot share={share} url={url} now={now} />
    </div>
  );
}

function File({ share, url, now }: ViewerProps) {
  return (
    <div className="share__body">
      <div className="file">
        <FileLine share={share} />
      </div>
      <Foot share={share} url={url} now={now} />
    </div>
  );
}

function TextFile({ share, url, now }: ViewerProps) {
  const language = languageOf(share.name);
  const [wrap, setWrap] = useState(() => wrapsByDefault(language));
  const text = useShareText(share);
  return (
    <div className="share__body" ref={text.ref}>
      <div className="file">
        <FileLine share={share} />
      </div>
      <div className="viewer__tools">
        <button type="button" className="btn btn--quiet viewer__tool" aria-pressed={wrap} onClick={() => setWrap(!wrap)}>
          {t("shares.wrap")}
        </button>
        {text.state === "ready" && <CopyButton text={text.text} />}
      </div>
      <Loaded share={share} text={text}>
        {(body) => <Code text={body} language={language} wrap={wrap} />}
      </Loaded>
      <Foot share={share} url={url} now={now} />
    </div>
  );
}

function MarkdownFile({ share, url, now }: ViewerProps) {
  const [source, setSource] = useState(false);
  const text = useShareText(share);
  return (
    <div className="share__body" ref={text.ref}>
      <div className="file">
        <FileLine share={share} />
      </div>
      <div className="viewer__tools">
        <Segmented
          options={[
            { value: false, label: t("shares.rendered") },
            { value: true, label: t("shares.source") },
          ]}
          value={source}
          onChange={setSource}
        />
        {text.state === "ready" && <CopyButton text={text.text} />}
      </div>
      <Loaded share={share} text={text}>
        {(body) => (source ? <Code text={body} language="plain" wrap /> : <Markdown text={body} className="viewer__md" />)}
      </Loaded>
      <Foot share={share} url={url} now={now} />
    </div>
  );
}

/**
 * A page runs in an iframe sandboxed to its scripts alone — no
 * `allow-same-origin` — as the PC's own policy serves it
 * (`sandbox allow-scripts`): it cannot reach the app, its cookie or its
 * storage, wherever it is opened. Its source is read as text.
 */
function Page({ share, url, now }: ViewerProps) {
  const [source, setSource] = useState(false);
  return (
    <div className="share__body">
      <div className="file">
        <FileLine share={share} />
      </div>
      <div className="viewer__tools">
        <Segmented
          options={[
            { value: false, label: t("shares.page") },
            { value: true, label: t("shares.source") },
          ]}
          value={source}
          onChange={setSource}
        />
      </div>
      {source ? (
        <PageSource share={share} />
      ) : (
        <iframe
          className="share__page"
          src={url}
          sandbox="allow-scripts"
          referrerPolicy="no-referrer"
          loading="lazy"
          title={t("shares.pageTitle", { name: share.name })}
        />
      )}
      <Foot share={share} url={url} now={now} />
    </div>
  );
}

function PageSource({ share }: { share: Share }) {
  const text = useShareText(share);
  return (
    <div ref={text.ref}>
      <Loaded share={share} text={text}>
        {(body) => <Code text={body} language="markup" wrap={false} />}
      </Loaded>
    </div>
  );
}

/* ── Texts: read when near the screen, kept, folded ────────────── */

type Text =
  | { state: "waiting" | "loading" | "failed" }
  | { state: "ready"; text: string; partial: boolean };

/** Texts already read, a share never changing: switching tabs reads nothing again. A few, not all. */
const kept = new Map<string, { text: string; partial: boolean }>();
const KEEP_TEXTS = 12;

function keep(id: string, text: { text: string; partial: boolean }) {
  kept.set(id, text);
  if (kept.size > KEEP_TEXTS) kept.delete(kept.keys().next().value!);
}

/** Read once the card nears the screen, so a long list does not read every file it holds. */
function useShareText(share: Share): Text & { ref: RefObject<HTMLDivElement | null>; retry: () => void } {
  const { client } = useRemoteContext();
  const ref = useRef<HTMLDivElement>(null);
  const [text, setText] = useState<Text>(() => {
    const known = kept.get(share.id);
    return known ? { state: "ready", ...known } : { state: "waiting" };
  });
  const near = useNearScreen(ref, text.state === "waiting");
  const ready = text.state === "ready";
  const [attempt, setAttempt] = useState(0);
  const { id, size } = share;

  useEffect(() => {
    if (!near || ready) return;
    let live = true;
    setText({ state: "loading" });
    client.fileText(id, size, TEXT_HEAD_BYTES).then(
      (read) => {
        keep(id, read);
        if (live) setText({ state: "ready", ...read });
      },
      () => live && setText({ state: "failed" }),
    );
    return () => {
      live = false;
    };
  }, [near, ready, id, size, attempt, client]);

  const retry = useCallback(() => setAttempt((n) => n + 1), []);
  return { ...text, ref, retry };
}

function useNearScreen(ref: RefObject<HTMLElement | null>, watching: boolean): boolean {
  const [near, setNear] = useState(false);
  useEffect(() => {
    if (!watching || near) return;
    const element = ref.current;
    if (!element || typeof IntersectionObserver === "undefined") {
      setNear(true);
      return;
    }
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) setNear(true);
      },
      { rootMargin: "600px 0px" },
    );
    observer.observe(element);
    return () => observer.disconnect();
  }, [ref, watching, near]);
  return near;
}

/** A text's body once read, folded; or what stands for it until then. A head says it is one. */
function Loaded({ share, text, children }: { share: Share; text: Text & { retry: () => void }; children: (text: string) => ReactNode }) {
  if (text.state === "failed") {
    return (
      <div className="viewer__state">
        <span>{t("shares.unreadable")}</span>
        <button type="button" className="btn btn--quiet" onClick={text.retry}>
          {t("error.retry")}
        </button>
      </div>
    );
  }
  if (text.state !== "ready") {
    return (
      <p className="viewer__state" role="status">
        {t("shares.loading")}
      </p>
    );
  }
  return (
    <>
      <Fold>{children(text.text)}</Fold>
      {text.partial && (
        <p className="viewer__note">
          {t("shares.head", { shown: fileSize(TEXT_HEAD_BYTES, locale), size: fileSize(share.size, locale) })}
        </p>
      )}
    </>
  );
}

/** A long body cut to a screenful, faded at the cut, and opened in place: the list scrolls, never the body. */
function Fold({ children }: { children: ReactNode }) {
  const body = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useState(false);
  const [overflows, setOverflows] = useState(false);

  useLayoutEffect(() => {
    const element = body.current;
    if (!element || open) return;
    const measure = () => setOverflows(element.scrollHeight > element.clientHeight + 1);
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, [open, children]);

  const cut = overflows && !open;
  return (
    <div className={open ? "fold fold--open" : cut ? "fold fold--cut" : "fold"}>
      <div ref={body} className="fold__body">
        {children}
      </div>
      {cut && (
        <button type="button" className="btn btn--quiet fold__more" onClick={() => setOpen(true)}>
          {t("shares.showAll")}
        </button>
      )}
    </div>
  );
}

function Code({ text, language, wrap }: { text: string; language: Language; wrap: boolean }) {
  const tokens = useMemo(() => highlight(text, language), [text, language]);
  return (
    <pre className={wrap ? "viewer__code viewer__code--wrap" : "viewer__code"} tabIndex={0}>
      <code>
        {tokens.map((token, index) =>
          token.kind === "plain" ? (
            token.text
          ) : (
            <span key={index} className={`tok tok--${token.kind}`}>
              {token.text}
            </span>
          ),
        )}
      </code>
    </pre>
  );
}

function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!copied) return;
    const timer = setTimeout(() => setCopied(false), 2000);
    return () => clearTimeout(timer);
  }, [copied]);
  // Only over HTTPS, as the PC serves the app through Tailscale.
  if (typeof navigator === "undefined" || !navigator.clipboard) return null;
  return (
    <button
      type="button"
      className="btn btn--quiet viewer__tool"
      onClick={() => void navigator.clipboard.writeText(text).then(() => setCopied(true), () => {})}
    >
      <span aria-live="polite">{copied ? t("shares.copied") : t("shares.copy")}</span>
    </button>
  );
}

function Segmented<T>({ options, value, onChange }: { options: { value: T; label: string }[]; value: T; onChange: (value: T) => void }) {
  return (
    <div className="segmented" role="group" aria-label={t("shares.viewAs")}>
      {options.map((option) => (
        <button
          key={option.label}
          type="button"
          className="segmented__option"
          aria-pressed={option.value === value}
          onClick={() => onChange(option.value)}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}
