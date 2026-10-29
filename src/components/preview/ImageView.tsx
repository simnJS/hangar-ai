import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type PointerEvent,
  type ReactNode,
} from "react";
import { Icon } from "../Icon";
import { CodeLines } from "./CodeLines";
import { Glyph } from "./Glyph";
import { NoticeText, StageMessage, Tool, useNotice } from "./parts";
import { CopyTool, TextStage, useWrap, WrapTool } from "./TextParts";
import { useTextHead } from "./useTextHead";
import type { ViewerProps } from "./FileView";
import { useLocale, useT } from "../../i18n";
import { fitZoom, hasSource, previewUrl, zoomAround, type ImageFrame, type Size } from "../../lib/preview";

/** One press of + or −, or of the bar's buttons. */
const STEP = 1.25;
/** An arrow key moves the image this far, in pixels. */
const NUDGE = 40;
/** From this zoom up, pixels are drawn as squares: what someone zooming that far wants to see. */
const PIXELATED = 3;

type Point = { x: number; y: number };

/**
 * An image, or an SVG's source on demand — the scheme serves it as plain
 * text when asked `?as=text`, as it does a page's.
 */
export function ImageView(props: ViewerProps) {
  const t = useT();
  const [source, setSource] = useState(false);
  const toggle = hasSource(props.entry.path) && (
    <>
      <Tool label={t("preview.sourceHint")} pressed={source} onClick={() => setSource((now) => !now)}>
        <Glyph name="code" size={14} />
      </Tool>
      <span className="model-panel__sep" aria-hidden="true" />
    </>
  );
  return source ? <ImageSource {...props} toggle={toggle} /> : <Picture {...props} toggle={toggle} />;
}

/**
 * The image itself: fitted to the panel, then zoomed by the wheel, a
 * trackpad's or a screen's pinch, the bar or the keyboard, and moved by
 * dragging; 1:1 shows its own pixels. A checkerboard behind it shows what is
 * transparent. An animated GIF plays, as the webview plays it. Following the
 * panel's size while fitted, it keeps the user's own framing once they zoom
 * or move it.
 */
function Picture({ entry, toggle }: ViewerProps & { toggle: ReactNode }) {
  const t = useT();
  const locale = useLocale();
  const stageRef = useRef<HTMLDivElement>(null);
  const [natural, setNatural] = useState<Size | null>(null);
  const [stage, setStage] = useState<Size>({ width: 0, height: 0 });
  /** Null while fitted to the panel. */
  const [chosen, setChosen] = useState<ImageFrame | null>(null);
  const [failed, setFailed] = useState(false);

  const fitted: ImageFrame = { zoom: natural && stage.width ? fitZoom(natural, stage) : 1, x: 0, y: 0 };
  const frame = chosen ?? fitted;
  // For the handlers registered once: several wheel events can land between renders.
  const fittedRef = useRef(fitted);
  fittedRef.current = fitted;
  const change = (next: (frame: ImageFrame) => ImageFrame) =>
    setChosen((current) => next(current ?? fittedRef.current));
  const changeRef = useRef(change);
  changeRef.current = change;

  useLayoutEffect(() => {
    const element = stageRef.current;
    if (!element) return;
    const observer = new ResizeObserver(([item]) =>
      setStage({ width: item.contentRect.width, height: item.contentRect.height }),
    );
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  /** Where an event is, from the stage's centre. */
  const local = (clientX: number, clientY: number): Point => {
    const rect = stageRef.current?.getBoundingClientRect();
    return rect
      ? { x: clientX - rect.left - rect.width / 2, y: clientY - rect.top - rect.height / 2 }
      : { x: 0, y: 0 };
  };

  // Registered by hand: React's wheel listener is passive, and the page must
  // not scroll under the zoom.
  useEffect(() => {
    const element = stageRef.current;
    if (!element) return;
    const wheel = (event: WheelEvent) => {
      event.preventDefault();
      const lines = event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? element.clientHeight : 1;
      // A trackpad's pinch arrives as a wheel with Ctrl held, in fine steps.
      const factor = Math.exp(-event.deltaY * lines * (event.ctrlKey ? 0.01 : 0.002));
      const point = local(event.clientX, event.clientY);
      changeRef.current((now) => zoomAround(now, factor, point));
    };
    element.addEventListener("wheel", wheel, { passive: false });
    return () => element.removeEventListener("wheel", wheel);
  }, []);

  // Dragging moves it; two fingers on a screen zoom around their middle and
  // move with it.
  const pointers = useRef(new Map<number, Point>());
  function onPointerDown(event: PointerEvent<HTMLDivElement>) {
    if (event.pointerType === "mouse" && event.button !== 0) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    pointers.current.set(event.pointerId, local(event.clientX, event.clientY));
  }
  function onPointerMove(event: PointerEvent<HTMLDivElement>) {
    const held = pointers.current;
    const before = held.get(event.pointerId);
    if (!before) return;
    const after = local(event.clientX, event.clientY);
    if (held.size === 1) {
      change((now) => ({ ...now, x: now.x + after.x - before.x, y: now.y + after.y - before.y }));
    } else if (held.size === 2) {
      const other = [...held].find(([id]) => id !== event.pointerId)?.[1];
      if (other) {
        const spread = (p: Point) => Math.hypot(p.x - other.x, p.y - other.y);
        const middle = (p: Point) => ({ x: (p.x + other.x) / 2, y: (p.y + other.y) / 2 });
        const [from, to] = [middle(before), middle(after)];
        const factor = spread(before) ? spread(after) / spread(before) : 1;
        change((now) => {
          const zoomed = zoomAround(now, factor, from);
          return { ...zoomed, x: zoomed.x + to.x - from.x, y: zoomed.y + to.y - from.y };
        });
      }
    }
    held.set(event.pointerId, after);
  }
  const onPointerEnd = (event: PointerEvent<HTMLDivElement>) => pointers.current.delete(event.pointerId);

  // Exactly 1, not what 1 / zoom × zoom rounds to.
  const actualSize = (point?: Point) => change((now) => ({ ...zoomAround(now, 1 / now.zoom, point), zoom: 1 }));

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    const moves: Record<string, Point> = {
      ArrowLeft: { x: NUDGE, y: 0 },
      ArrowRight: { x: -NUDGE, y: 0 },
      ArrowUp: { x: 0, y: NUDGE },
      ArrowDown: { x: 0, y: -NUDGE },
    };
    const move = moves[event.key];
    if (move) change((now) => ({ ...now, x: now.x + move.x, y: now.y + move.y }));
    else if (event.key === "+" || event.key === "=") change((now) => zoomAround(now, STEP));
    else if (event.key === "-") change((now) => zoomAround(now, 1 / STEP));
    else if (event.key === "0") setChosen(null);
    else if (event.key === "1") actualSize();
    else return;
    event.preventDefault();
  }

  const percent = new Intl.NumberFormat(locale, { style: "percent", maximumFractionDigits: 0 }).format(frame.zoom);

  return (
    <>
      <div className="model-panel__bar" role="toolbar" aria-label={t("preview.tools", { kind: t("preview.kind.image") })}>
        {toggle}
        <Tool label={t("preview.zoomOut")} disabled={!natural} onClick={() => change((now) => zoomAround(now, 1 / STEP))}>
          <Icon name="minus" size={14} />
        </Tool>
        <span className="preview-zoom" title={t("preview.zoom")}>
          {percent}
        </span>
        <Tool label={t("preview.zoomIn")} disabled={!natural} onClick={() => change((now) => zoomAround(now, STEP))}>
          <Icon name="plus" size={14} />
        </Tool>
        <span className="model-panel__sep" aria-hidden="true" />
        <Tool label={t("preview.fit")} pressed={!chosen} disabled={!natural} onClick={() => setChosen(null)}>
          <Icon name="frame" size={14} />
        </Tool>
        <Tool label={t("preview.actualSize")} pressed={frame.zoom === 1} disabled={!natural} onClick={() => actualSize()}>
          <span className="preview-one">1:1</span>
        </Tool>
        <span className="model-panel__spacer" />
        {natural && (
          <span className="preview-chip preview-chip--quiet">
            {t("preview.pixels", { width: natural.width, height: natural.height })}
          </span>
        )}
      </div>
      <div
        ref={stageRef}
        className="model-panel__stage preview-stage preview-image"
        tabIndex={0}
        // An application, as the 3D view is: a screen reader passes it the keys.
        role="application"
        aria-label={t("preview.imageLabel", { title: entry.title })}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerEnd}
        onPointerCancel={onPointerEnd}
        onDoubleClick={(event) => (chosen ? setChosen(null) : actualSize(local(event.clientX, event.clientY)))}
        onKeyDown={onKeyDown}
      >
        {failed ? (
          <StageMessage glyph="image" tone="danger" title={t("preview.imageFailed")} detail={entry.path} />
        ) : (
          <img
            className={`preview-image__img ${frame.zoom >= PIXELATED ? "is-pixelated" : ""}`}
            src={previewUrl(entry.path, entry.revision)}
            alt=""
            draggable={false}
            decoding="async"
            onLoad={(event) =>
              setNatural({
                // An SVG without a size of its own still needs one to fit.
                width: event.currentTarget.naturalWidth || 512,
                height: event.currentTarget.naturalHeight || 512,
              })
            }
            onError={() => setFailed(true)}
            style={
              natural
                ? {
                    width: natural.width * frame.zoom,
                    height: natural.height * frame.zoom,
                    transform: `translate(${frame.x}px, ${frame.y}px) translate(-50%, -50%)`,
                  }
                : { visibility: "hidden" }
            }
          />
        )}
      </div>
    </>
  );
}

function ImageSource({ entry, toggle }: ViewerProps & { toggle: ReactNode }) {
  const t = useT();
  const head = useTextHead(previewUrl(entry.path, entry.revision, true));
  const [wrap, toggleWrap] = useWrap();
  const [notice, setNotice] = useNotice();
  const text = head.status === "ready" ? head.text : null;
  return (
    <>
      <div className="model-panel__bar" role="toolbar" aria-label={t("preview.tools", { kind: t("preview.kind.image") })}>
        {toggle}
        <WrapTool wrap={wrap} onToggle={toggleWrap} />
        <CopyTool text={text} onNotice={setNotice} />
        <span className="model-panel__spacer" />
        <NoticeText notice={notice} />
      </div>
      <TextStage state={head} title={entry.title}>
        <CodeLines
          text={text ?? ""}
          language="xml"
          wrap={wrap}
          label={t("preview.codeLabel", { title: entry.title, language: "SVG" })}
        />
      </TextStage>
    </>
  );
}
