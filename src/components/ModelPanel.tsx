import {
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type RefObject,
} from "react";
import { Icon, type IconName } from "./Icon";
import { ToolbarMenu, type MenuCloseReason } from "./ToolbarMenu";
import { FileView } from "./preview/FileView";
import { Glyph } from "./preview/Glyph";
import { KitTabView } from "./preview/KitTabView";
import { Banner, StageMessage } from "./preview/parts";
import { useLocale, useT, type Locale } from "../i18n";
import { FORMAT_PLANS } from "../lib/model3d/formats";
import type {
  AnimationState,
  DisplayOptions,
  LoadProgress,
  ModelViewer,
} from "../lib/model3d/engine";
import {
  useModelLoads,
  useModelWatch,
  useSnapshotter,
  useViewer,
  useViewerTheme,
} from "../lib/model3d/useModelViewer";
import { useAsleep } from "../lib/hibernateStore";
import { captureName, copyPng, savePng } from "../lib/modelCapture";
import {
  activateModel,
  closeModel,
  isKitTab,
  isModel,
  isPreviewFile,
  reloadModel,
  useAllModels,
  useModels,
  useWindowKitTabs,
  type ModelInfo,
  type OpenModel,
  type OpenPreview,
} from "../lib/modelStore";
import { clampModelPanel, MODEL_MIN } from "../lib/sidePanels";
import { getTerminal } from "../lib/terminalRegistry";
import { useDrawer, type DrawerCloseReason } from "../lib/useDrawer";
import { DEFAULT_SETTINGS } from "../types";

/**
 * The preview: the files agents opened, one tab each — a 3D model, an image,
 * a video, a sound, a text or code file, Markdown, a PDF, a web page — and
 * the tabs of the workspace's kits the person opened, in a panel beside the
 * terminals, or a drawer over them when there is no room,
 * or the whole work area when expanded. lib/sidePanels decides which; App
 * passes it in.
 *
 * Each workspace has its own preview (lib/modelStore), and the panel draws
 * the one of the workspace on screen. The tab in front is drawn by its
 * viewer: the 3D view below for a model, components/preview for the rest.
 *
 * There is one panel per window all the same, and one 3D view in it — one
 * WebGL context, one three.js scene — which holds the models of every
 * workspace: a switch of workspace is a switch of tab, and the engine's own
 * budget (KEEP_LOADED) bounds the GPU memory however many workspaces have
 * models open.
 *
 * The panel stays mounted, hidden, for as long as any workspace of the window
 * has a file open, even while another view or workspace is on screen; the 3D
 * view in it for as long as one has a model open, so the loaded models stay
 * on the GPU, a switch is instant, and an agent's snapshot can be rendered
 * whatever the user is looking at. The engine itself (lib/model3d/engine,
 * three.js) arrives with the first model, through a dynamic import: a window
 * that only ever shows images and documents never loads it.
 *
 * Kit tabs stay mounted the same way, every workspace's, each hidden but the
 * one in front: a kit's page keeps what it was doing while the person looks
 * at something else (components/preview/KitTabView). A workspace put to
 * sleep lets its pages go, and they load again when it wakes.
 */

/** In interface points, like the width itself. */
const KEY_STEP = 24;

interface Props {
  /** The workspace on screen, whose preview the panel draws; null for none. */
  workspaceId: string | null;
  /** On screen: docked, an open drawer, or expanded. */
  shown: boolean;
  /** No room beside the terminals: drawn over them while it is asked for. */
  drawer: boolean;
  /** Over the whole work area, the terminals still running underneath. */
  expanded: boolean;
  /** As saved, in interface points: drawn at `width × ui-scale`. */
  width: number;
  /** What the room leaves of it, in pixels, as lib/sidePanels placed it. */
  drawnWidth: number;
  /** The widest the room allows right now, in points. */
  maxWidth: number;
  scale: number;
  /** The focused pane, which takes the keyboard back when nothing else can. */
  paneId: string | null;
  onResize: (width: number) => void;
  onClose: () => void;
  onToggleExpand: () => void;
}

export function ModelPanel(props: Props) {
  const { shown, drawer, expanded, paneId } = props;
  const { models: entries, activeId } = useModels(props.workspaceId);
  const windowModels = useAllModels().length > 0;
  const t = useT();
  const asideRef = useRef<HTMLElement>(null);
  const panelId = useId();
  const active = entries.find((entry) => entry.id === activeId) ?? null;
  const file = active && isPreviewFile(active) ? active : null;
  const kit = active && isKitTab(active) ? active : null;
  const kitTabs = useWindowKitTabs();
  const asleep = useAsleep();

  // ---------------------------------------------------------------------------
  // Focus: as a drawer the panel takes the keyboard and gives it back
  // (lib/useDrawer); hidden with the focus inside, it hands it to the pane.
  // ---------------------------------------------------------------------------

  const closeDrawerRef = useRef<(reason: DrawerCloseReason) => void>(() => props.onClose());
  const focusInside = useRef(false);
  useLayoutEffect(() => {
    if (shown || !focusInside.current) return;
    focusInside.current = false;
    getTerminal(paneId)?.focus();
  }, [shown, paneId]);

  const close = () => (drawer && !expanded ? closeDrawerRef.current("button") : props.onClose());

  function onKeyDown(event: KeyboardEvent<HTMLElement>) {
    // Expanded, Escape puts it back beside the terminals, like leaving a
    // full-screen view; anything inside that handles Escape itself keeps it.
    if (event.key === "Escape" && expanded && !event.defaultPrevented) {
      event.preventDefault();
      props.onToggleExpand();
    }
  }

  return (
    <aside
      ref={asideRef}
      className={[
        "model-panel",
        drawer && !expanded ? "model-panel--drawer" : "",
        expanded ? "model-panel--expanded" : "",
      ].join(" ")}
      style={expanded ? undefined : { width: props.drawnWidth }}
      hidden={!shown}
      aria-label={t("model.panel")}
      // Focused as a region — by F6, or as a drawer — like the board panel.
      tabIndex={-1}
      onKeyDown={onKeyDown}
      onFocus={() => (focusInside.current = true)}
      onBlur={(event) => {
        if (!asideRef.current?.contains(event.relatedTarget as Node | null)) focusInside.current = false;
      }}
    >
      {shown && drawer && !expanded && (
        <DrawerFocus panelRef={asideRef} paneId={paneId} onClose={props.onClose} closeRef={closeDrawerRef} />
      )}
      {!expanded && <Grip {...props} />}

      <div className="model-panel__head">
        <PreviewTabs entries={entries} activeId={activeId} panelId={panelId} />
        <span className="model-panel__spacer" />
        <button
          type="button"
          className="model-panel__tool"
          onClick={props.onToggleExpand}
          aria-pressed={expanded}
          title={expanded ? t("model.collapse") : t("model.expand")}
          aria-label={expanded ? t("model.collapse") : t("model.expand")}
        >
          <Icon name={expanded ? "collapse" : "expand"} size={12} />
        </button>
        <button
          type="button"
          className="model-panel__tool"
          onClick={close}
          title={t("model.hide")}
          aria-label={t("model.hide")}
        >
          <Icon name="close" size={13} />
        </button>
      </div>

      <div
        className="preview-body"
        id={`${panelId}-stage`}
        role="tabpanel"
        aria-labelledby={active ? `${panelId}-tab-${active.id}` : undefined}
      >
        {windowModels && (
          <ModelView active={active && isModel(active) ? active : null} hidden={Boolean(file || kit)} shown={shown} />
        )}
        {kitTabs
          .filter((tab) => !asleep.has(tab.workspaceId))
          .map((tab) => (
            <KitTabView key={tab.id} entry={tab} shown={shown && tab.id === kit?.id} />
          ))}
        {file ? (
          <FileView entry={file} shown={shown} />
        ) : (
          !windowModels &&
          !kit && (
            <div className="model-panel__stage">
              <StageMessage icon="cube" title={t("model.empty")} detail={t("model.emptyHint")} />
            </div>
          )
        )}
      </div>
    </aside>
  );
}

/**
 * The 3D view: its bar, its stage, its animation and its numbers, for the
 * model in front — or for none, hidden, while another kind of file is in
 * front, its models staying loaded. Mounted while any workspace of the
 * window has a model open (see ModelPanel).
 */
function ModelView({
  active,
  hidden,
  shown,
}: {
  active: OpenModel | null;
  /** Another kind of file is in front. */
  hidden: boolean;
  /** The panel is on screen. */
  shown: boolean;
}) {
  const everyModel = useAllModels();
  const asleep = useAsleep();
  const t = useT();
  const locale = useLocale();
  const hostRef = useRef<HTMLDivElement>(null);

  const [generation, setGeneration] = useState(0);
  const { viewer, status, ready } = useViewer(hostRef, generation);
  const theme = useViewerTheme(viewer);
  const { progress, ensure, forget } = useModelLoads(viewer);
  useSnapshotter(ready, ensure);

  const missing = useModelWatch(active, shown && !hidden);

  // Closed tabs let go of their models, and so does a workspace put to sleep:
  // its models stay listed, and load again once it is back on screen.
  const awake = everyModel.filter((model) => !asleep.has(model.workspaceId));
  const openKey = awake.map((model) => model.id).join(" ");
  useEffect(() => {
    forget(new Set(openKey ? openKey.split(" ") : []));
  }, [openKey, forget]);

  // A model an agent opened in a workspace that is not on screen loads all
  // the same: the agent hears what the viewer found, and the user finds it
  // ready. The one in front is loaded below.
  const pending = awake.filter((model) => model.status === "loading" && model.id !== active?.id);
  const pendingKey = pending.map((model) => `${model.id}:${model.revision}`).join(" ");
  useEffect(() => {
    if (!viewer) return;
    for (const model of pending) ensure(model, viewer).catch(() => undefined);
    // The models change on every store write; which revisions wait is what counts.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [viewer, pendingKey, ensure]);

  // The model in front is loaded and shown; while a newer version loads, the
  // previous one stays on screen.
  const [shownRevision, setShownRevision] = useState<Record<string, number>>({});
  useEffect(() => {
    if (!viewer || !active) return;
    viewer.show(active.id);
    let current = true;
    ensure(active, viewer)
      .then(() => {
        // Only if it is still the one in front: a slow load must not take the
        // view back from a tab the user moved to since.
        if (!current) return;
        viewer.show(active.id);
        setShownRevision((map) => ({ ...map, [active.id]: active.revision }));
      })
      .catch(() => undefined);
    return () => {
      current = false;
    };
    // The model object changes on every store write; its id and revision are
    // what decide whether there is something to load.
  }, [viewer, active?.id, active?.revision, ensure]);

  // ---------------------------------------------------------------------------
  // Display toggles and animation, for the model in front.
  // ---------------------------------------------------------------------------

  const [display, setDisplay] = useState<DisplayOptions>({
    wireframe: false,
    grid: true,
    axes: false,
    normals: false,
  });
  useEffect(() => {
    viewer?.setDisplay(display);
  }, [viewer, display]);
  const toggle = (key: keyof DisplayOptions) => setDisplay((now) => ({ ...now, [key]: !now[key] }));

  const [animation, setAnimation] = useState<AnimationState | null>(null);
  useEffect(() => {
    if (!viewer || !active) return setAnimation(null);
    setAnimation(viewer.animationState(active.id));
    return viewer.on((event) => {
      if (event.type === "animation" && event.modelId === active.id) setAnimation(event.state);
    });
  }, [viewer, active?.id, shownRevision]);

  const loading = active ? progress[active.id] : undefined;
  const hasContent = Boolean(active && viewer?.hasAny(active.id));
  // Loading with nothing to show yet: a first load, or a model dropped from
  // memory to make room for others, coming back.
  const firstLoad = Boolean(active && !hasContent && (active.status === "loading" || loading));
  const failedOutright = Boolean(active && active.status === "error" && !hasContent);

  return (
    <div className="preview-model" hidden={hidden}>
      <ModelToolbar
        viewer={viewer}
        active={active}
        ready={hasContent}
        display={display}
        onToggle={toggle}
        background={theme.background}
      />

      <div className="model-panel__stage">
        <div
          className="model-view"
          ref={hostRef}
          data-appearance={theme.dark ? "dark" : "light"}
          data-lost={status.kind === "lost" || undefined}
          // Focusable, so the keyboard can move around the model too; an
          // application, so a screen reader passes it the arrow keys.
          tabIndex={hasContent ? 0 : -1}
          role="application"
          aria-label={active ? t("model.viewLabel", { title: active.title }) : t("model.panel")}
        />

        {active && active.status === "loading" && hasContent && (
          <div className="model-stage__reload" role="status">
            <ProgressBar progress={loading} slim />
            <span className="model-stage__chip">{t("model.reloading")}</span>
          </div>
        )}

        {active && active.status === "error" && hasContent && (
          <Banner
            tone="danger"
            text={t("model.reloadFailed")}
            detail={active.error}
            action={t("model.retry")}
            onAction={() => reloadModel(active.id, active.modifiedMs)}
          />
        )}
        {active && missing && active.status !== "error" && (
          <Banner tone="warning" text={t("model.missing")} detail={missing} />
        )}

        {status.kind === "failed" ? (
          <StageMessage icon="alert" tone="danger" title={t("model.unavailable")} detail={status.error} />
        ) : status.kind === "lost" ? (
          <StageMessage
            icon="alert"
            tone="warning"
            title={t("model.lost")}
            action={t("model.restart")}
            onAction={() => setGeneration((n) => n + 1)}
          />
        ) : !active ? (
          <StageMessage icon="cube" title={t("model.empty")} detail={t("model.emptyHint")} />
        ) : failedOutright ? (
          <StageMessage
            icon="alert"
            tone="danger"
            title={t("model.errorTitle")}
            detail={active.error ?? undefined}
            action={t("model.retry")}
            onAction={() => reloadModel(active.id, active.modifiedMs)}
          />
        ) : firstLoad || status.kind === "starting" ? (
          <LoadingCard title={active.title} progress={loading} starting={status.kind === "starting"} />
        ) : null}
      </div>

      {animation && hasContent && <AnimationBar viewer={viewer} state={animation} />}
      <InfoStrip model={active} info={hasContent ? (active?.info ?? null) : null} locale={locale} />
    </div>
  );
}

/** useDrawer, mounted with each opening, so it knows who had the keyboard then. */
function DrawerFocus({
  panelRef,
  paneId,
  onClose,
  closeRef,
}: {
  panelRef: RefObject<HTMLElement | null>;
  paneId: string | null;
  onClose: () => void;
  closeRef: { current: (reason: DrawerCloseReason) => void };
}) {
  closeRef.current = useDrawer({ panelRef, active: true, onClose: () => onClose(), paneId });
  return null;
}

/** The resize handle on the panel's left edge, as on the board panel. */
function Grip({ width, drawnWidth, maxWidth, scale, onResize }: Props) {
  const t = useT();
  const gripRef = useRef<HTMLDivElement>(null);
  const clamp = (next: number) => Math.min(maxWidth, clampModelPanel(next));
  const shown = Math.round(drawnWidth / scale);

  function beginResize(event: React.PointerEvent<HTMLDivElement>) {
    if (event.button !== 0) return;
    const aside = gripRef.current?.parentElement;
    if (!aside) return;
    event.preventDefault();
    const grip = event.currentTarget;
    const pointerId = event.pointerId;
    const startX = event.clientX;
    const startWidth = aside.getBoundingClientRect().width;
    let latest = shown;
    grip.setPointerCapture(pointerId);
    grip.classList.add("is-active");
    document.body.classList.add("is-resizing");

    // Painted straight onto the element while dragging; the settings hear
    // about it once, on release. The canvas follows through its own
    // resize observer.
    const move = (ev: PointerEvent) => {
      if (ev.pointerId !== pointerId) return;
      latest = clamp((startWidth + (startX - ev.clientX)) / scale);
      aside.style.width = `${latest * scale}px`;
    };
    const end = (ev?: PointerEvent) => {
      if (ev && ev.pointerId !== pointerId) return;
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", end);
      window.removeEventListener("pointercancel", end);
      window.removeEventListener("blur", lost);
      if (grip.hasPointerCapture(pointerId)) grip.releasePointerCapture(pointerId);
      grip.classList.remove("is-active");
      document.body.classList.remove("is-resizing");
      if (latest !== width) onResize(latest);
    };
    const lost = () => end();
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", end);
    window.addEventListener("pointercancel", end);
    window.addEventListener("blur", lost);
  }

  return (
    <div
      ref={gripRef}
      className="model-panel__grip"
      role="separator"
      aria-orientation="vertical"
      aria-label={t("model.resize")}
      aria-valuenow={shown}
      aria-valuemin={MODEL_MIN}
      aria-valuemax={Math.max(MODEL_MIN, Math.round(maxWidth))}
      title={t("model.resize")}
      tabIndex={0}
      onPointerDown={beginResize}
      onDoubleClick={() => onResize(DEFAULT_SETTINGS.modelPanelWidth)}
      onKeyDown={(event) => {
        // The panel grows leftwards, into the terminals.
        if (event.key === "ArrowLeft") onResize(clamp(shown + KEY_STEP));
        else if (event.key === "ArrowRight") onResize(clamp(shown - KEY_STEP));
        else return;
        event.preventDefault();
      }}
    />
  );
}

// -----------------------------------------------------------------------------
// Tabs
// -----------------------------------------------------------------------------

function PreviewTabs({
  entries,
  activeId,
  panelId,
}: {
  entries: OpenPreview[];
  activeId: string | null;
  panelId: string;
}) {
  const t = useT();
  const listRef = useRef<HTMLDivElement>(null);

  // The tab in front is scrolled into view when it changes: an agent may
  // bring forward one that sits past the panel's edge.
  useEffect(() => {
    const tab = listRef.current?.querySelector<HTMLElement>('[aria-selected="true"]');
    tab?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }, [activeId, entries.length]);

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    const at = entries.findIndex((entry) => entry.id === activeId);
    let next = -1;
    if (event.key === "ArrowRight") next = (at + 1) % entries.length;
    else if (event.key === "ArrowLeft") next = (at - 1 + entries.length) % entries.length;
    else if (event.key === "Home") next = 0;
    else if (event.key === "End") next = entries.length - 1;
    else if (event.key === "Delete" && activeId) {
      event.preventDefault();
      closeModel(activeId);
      return;
    } else return;
    event.preventDefault();
    const target = entries[next];
    if (!target) return;
    activateModel(target.id);
    // After the render that makes it the tab stop.
    requestAnimationFrame(() =>
      listRef.current?.querySelector<HTMLElement>(`[data-model="${target.id}"]`)?.focus(),
    );
  }

  return (
    <div
      className="model-tabs"
      role="tablist"
      aria-label={t("model.tabs")}
      ref={listRef}
      onKeyDown={onKeyDown}
    >
      {entries.map((entry) => {
        const selected = entry.id === activeId;
        const by = entry.openedBy ? t("model.openedBy", { name: entry.openedBy }) : null;
        return (
          <div key={entry.id} className={`model-tab ${selected ? "is-active" : ""}`}>
            <button
              type="button"
              role="tab"
              id={`${panelId}-tab-${entry.id}`}
              data-model={entry.id}
              className="model-tab__main"
              aria-selected={selected}
              aria-controls={`${panelId}-stage`}
              tabIndex={selected ? 0 : -1}
              title={
                isKitTab(entry)
                  ? t("kitsTab.tabHint", { title: entry.title, kit: entry.kitId })
                  : by
                    ? `${entry.path}\n${by}`
                    : entry.path
              }
              onClick={() => activateModel(entry.id)}
            >
              {entry.status === "loading" ? (
                <span className="model-tab__spinner" aria-hidden="true" />
              ) : entry.status === "error" ? (
                <Icon name="alert" size={12} className="model-tab__icon model-tab__icon--error" />
              ) : isModel(entry) ? (
                <Icon name="cube" size={12} className="model-tab__icon" />
              ) : isKitTab(entry) ? (
                <Icon name="window" size={12} className="model-tab__icon" />
              ) : (
                <Glyph name={entry.kind} size={12} className="model-tab__icon" />
              )}
              <span className="model-tab__title">{entry.title}</span>
              {/* Beside several tabs it would be cut to "ou…" in each: only
                  the one in front says who opened it, the others in their
                  tooltip. */}
              {by && (selected || entries.length === 1) && <span className="model-tab__by">{by}</span>}
            </button>
            <button
              type="button"
              className="model-tab__close"
              tabIndex={selected ? 0 : -1}
              onClick={() => closeModel(entry.id)}
              title={t("model.close", { title: entry.title })}
              aria-label={t("model.close", { title: entry.title })}
            >
              <Icon name="close" size={11} />
            </button>
          </div>
        );
      })}
    </div>
  );
}

// -----------------------------------------------------------------------------
// Toolbar
// -----------------------------------------------------------------------------

function ModelToolbar({
  viewer,
  active,
  ready,
  display,
  onToggle,
  background,
}: {
  viewer: ModelViewer | null;
  active: OpenModel | null;
  ready: boolean;
  display: DisplayOptions;
  onToggle: (key: keyof DisplayOptions) => void;
  background: string;
}) {
  const t = useT();
  const captureRef = useRef<HTMLButtonElement>(null);
  const [menu, setMenu] = useState(false);
  const [notice, setNotice] = useState<{ text: string; error?: boolean } | null>(null);
  const normalsAllowed = Boolean(active && viewer?.canDrawNormals(active.id));

  useEffect(() => {
    if (!notice) return;
    const timer = window.setTimeout(() => setNotice(null), notice.error ? 6000 : 2200);
    return () => window.clearTimeout(timer);
  }, [notice]);

  const closeMenu = (reason: MenuCloseReason) => {
    setMenu(false);
    if (reason === "escape" || reason === "tab") captureRef.current?.focus();
  };

  async function capture(kind: "copy" | "save") {
    setMenu(false);
    captureRef.current?.focus();
    if (!viewer || !active) return;
    try {
      // Twice the panel's size: sharp on a high-density screen and in a
      // document, and still the user's own framing.
      const png = await viewer.capture(2, background);
      if (kind === "copy") {
        await copyPng(png);
        setNotice({ text: t("model.copied") });
      } else if (await savePng(png, captureName(active.title))) {
        setNotice({ text: t("model.saved") });
      }
    } catch (error) {
      setNotice({
        text: t("model.captureFailed", { error: error instanceof Error ? error.message : String(error) }),
        error: true,
      });
    }
  }

  const tool = (key: keyof DisplayOptions, icon: IconName, label: string, disabled = false) => (
    <button
      type="button"
      className="model-panel__tool"
      aria-pressed={display[key] && !disabled}
      disabled={!ready || disabled}
      onClick={() => onToggle(key)}
      title={label}
      aria-label={label}
    >
      <Icon name={icon} size={14} />
    </button>
  );

  return (
    <div className="model-panel__bar" role="toolbar" aria-label={t("model.tools")}>
      <button
        type="button"
        className="model-panel__tool"
        disabled={!ready}
        onClick={() => viewer?.recenter()}
        title={t("model.recenter")}
        aria-label={t("model.recenter")}
      >
        <Icon name="frame" size={14} />
      </button>
      <span className="model-panel__sep" aria-hidden="true" />
      {tool("wireframe", "wireframe", t("model.wireframe"))}
      {tool("grid", "grid", t("model.grid"))}
      {tool("axes", "axes", t("model.axes"))}
      {tool(
        "normals",
        "normals",
        normalsAllowed || !ready ? t("model.normals") : t("model.normalsTooMany"),
        ready && !normalsAllowed,
      )}
      <span className="model-panel__spacer" />
      {notice && (
        <span className={`model-panel__notice ${notice.error ? "is-error" : ""}`} role="status">
          {!notice.error && <Icon name="check" size={12} />}
          {notice.text}
        </span>
      )}
      <button
        ref={captureRef}
        type="button"
        className={`model-panel__tool ${menu ? "is-open" : ""}`}
        disabled={!ready}
        onClick={() => setMenu((open) => !open)}
        title={t("model.capture")}
        aria-label={t("model.capture")}
        aria-haspopup="menu"
        aria-expanded={menu}
      >
        <Icon name="camera" size={14} />
      </button>
      {menu && (
        <ToolbarMenu anchorRef={captureRef} label={t("model.capture")} onClose={closeMenu}>
          <button type="button" role="menuitem" className="tmenu__item" onClick={() => void capture("copy")}>
            <Icon name="copy" size={14} />
            <span className="tmenu__label">{t("model.copyImage")}</span>
          </button>
          <button type="button" role="menuitem" className="tmenu__item" onClick={() => void capture("save")}>
            <Icon name="download" size={14} />
            <span className="tmenu__label">{t("model.saveImage")}</span>
          </button>
        </ToolbarMenu>
      )}
    </div>
  );
}

// -----------------------------------------------------------------------------
// Animation and info
// -----------------------------------------------------------------------------

function AnimationBar({ viewer, state }: { viewer: ModelViewer | null; state: AnimationState }) {
  const t = useT();
  const played = state.duration ? state.time / state.duration : 0;
  const locale = useLocale();
  const seconds = (value: number) =>
    new Intl.NumberFormat(locale, { minimumFractionDigits: 1, maximumFractionDigits: 1 }).format(value);
  return (
    <div className="model-anim" role="group" aria-label={t("model.animation")}>
      <button
        type="button"
        className="model-panel__tool"
        onClick={() => viewer?.setPlaying(!state.playing)}
        title={state.playing ? t("model.pause") : t("model.play")}
        aria-label={state.playing ? t("model.pause") : t("model.play")}
      >
        <Icon name={state.playing ? "pause" : "play"} size={12} />
      </button>
      {state.clips.length > 1 ? (
        <select
          className="model-anim__clip"
          value={state.clip}
          onChange={(event) => viewer?.setClip(Number(event.target.value))}
          aria-label={t("model.clip")}
        >
          {state.clips.map((name, i) => (
            <option key={i} value={i}>
              {name}
            </option>
          ))}
        </select>
      ) : (
        <span className="model-anim__name" title={state.clips[0]}>
          {state.clips[0]}
        </span>
      )}
      <input
        className="model-anim__scrub"
        type="range"
        min={0}
        max={state.duration || 1}
        step={0.01}
        value={state.time}
        onChange={(event) => viewer?.seek(Number(event.target.value))}
        aria-label={t("model.time")}
        aria-valuetext={t("model.timeOf", { time: seconds(state.time), total: seconds(state.duration) })}
        style={{ "--progress": `${played * 100}%` } as React.CSSProperties}
      />
      <span className="model-anim__time">
        {t("model.timeOf", { time: seconds(state.time), total: seconds(state.duration) })}
      </span>
    </div>
  );
}

const numberFormats = new Map<string, Intl.NumberFormat>();
function formatter(locale: Locale, options: Intl.NumberFormatOptions) {
  const key = `${locale} ${JSON.stringify(options)}`;
  let format = numberFormats.get(key);
  if (!format) {
    format = new Intl.NumberFormat(locale, options);
    numberFormats.set(key, format);
  }
  return format;
}

/** Counts in full up to a hundred thousand, compact above: "4 444", "1,2 M". */
export const formatCount = (value: number, locale: Locale) =>
  formatter(locale, value >= 100_000 ? { notation: "compact", maximumFractionDigits: 1 } : {}).format(value);

/** Three significant figures, which is what a size in a status line can use. */
export const formatLength = (value: number, locale: Locale) =>
  formatter(locale, { maximumSignificantDigits: 3 }).format(value);

function InfoStrip({
  model,
  info,
  locale,
}: {
  model: OpenModel | null;
  info: ModelInfo | null;
  locale: Locale;
}) {
  const t = useT();
  if (!model) return <div className="model-info" />;
  const unit = FORMAT_PLANS[model.format].unit;
  const item = (
    value: number,
    key: "model.triangles" | "model.vertices" | "model.materials",
    modifier: string,
  ) => (
    <span
      className={`model-info__item model-info__item--${modifier}`}
      title={formatter(locale, {}).format(value)}
    >
      <span className="model-info__value">{formatCount(value, locale)}</span> {t(key, { n: value })}
    </span>
  );
  return (
    <div className="model-info" aria-label={t("model.stats")}>
      <span className="model-info__format">{model.format}</span>
      {info && (
        <>
          {item(info.triangles, "model.triangles", "triangles")}
          <span
            className="model-info__item model-info__item--size"
            title={unit ? t("model.size") : `${t("model.size")}\n${t("model.unitless")}`}
          >
            <span className="model-info__value">
              {info.size.map((value) => formatLength(value, locale)).join(" × ")}
            </span>
            {unit && ` ${unit}`}
          </span>
          {item(info.materials, "model.materials", "materials")}
          {item(info.vertices, "model.vertices", "vertices")}
        </>
      )}
    </div>
  );
}

// -----------------------------------------------------------------------------
// States
// -----------------------------------------------------------------------------

function ProgressBar({ progress, slim = false }: { progress?: LoadProgress; slim?: boolean }) {
  const determinate = progress?.phase === "download" && progress.total > 0;
  const ratio = determinate ? Math.min(1, progress!.loaded / progress!.total) : 0;
  return (
    <div
      className={[
        "model-progress",
        slim ? "model-progress--slim" : "",
        determinate ? "" : "is-indeterminate",
      ].join(" ")}
      role="progressbar"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={determinate ? Math.round(ratio * 100) : undefined}
    >
      <span className="model-progress__fill" style={determinate ? { width: `${ratio * 100}%` } : undefined} />
    </div>
  );
}

function LoadingCard({
  title,
  progress,
  starting,
}: {
  title: string;
  progress?: LoadProgress;
  starting: boolean;
}) {
  const t = useT();
  const locale = useLocale();
  const unit = (bytes: number) =>
    t("model.megabytes", {
      size: formatter(locale, { maximumFractionDigits: bytes < 10 * 1024 * 1024 ? 1 : 0 }).format(
        bytes / (1024 * 1024),
      ),
    });
  const phase = starting
    ? t("model.starting")
    : progress?.phase === "parse"
      ? t("model.parse")
      : progress?.phase === "prepare"
        ? t("model.prepare")
        : t("model.download");
  const bytes =
    !starting && progress?.phase === "download" && progress.loaded > 0
      ? progress.total > 0
        ? t("model.downloadBytes", { loaded: unit(progress.loaded), total: unit(progress.total) })
        : unit(progress.loaded)
      : null;
  return (
    <div className="model-stage__center" role="status">
      <div className="model-loading">
        <span className="model-loading__title">{title}</span>
        <ProgressBar progress={starting ? undefined : progress} />
        <span className="model-loading__phase">
          {phase}
          {bytes && <span className="model-loading__bytes">{bytes}</span>}
        </span>
      </div>
    </div>
  );
}
