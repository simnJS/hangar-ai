import { useCallback, useEffect, useId, useMemo, useRef, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { useStore } from "../store";
import { useT } from "../i18n";
import { Logo } from "./Logo";
import { WorktreePanel } from "./WorktreePanel";
import { Icon } from "./Icon";
import { StateGlyph } from "./StateGlyph";
import { NavigatorMenu, type NavigatorMenuEntry } from "./NavigatorMenu";
import { NavigatorActivityLine, NavigatorAgentRow, NavigatorPaneWatch } from "./NavigatorAgents";
import { NavigatorPlan } from "./NavigatorPlan";
import {
  buildNavTree,
  rowKey,
  type NavFolder,
  type NavRow,
  type NavWorkspace,
} from "./NavigatorModel";
import { getPaneActivity } from "../lib/agentState";
import { mcpScopeClear } from "../lib/mcpScope";
import { useShortcutTitle } from "../lib/useShortcuts";

interface Props {
  onOpenSettings: () => void;
  onNewWorkspace: () => void;
  /** The focused pane of each workspace, by workspace id. */
  focusByWorkspace: Record<string, string>;
  /** Brings a pane forward, switching workspace if it has to. */
  onFocusPane: (workspaceId: string, paneId: string) => void;
  /** Opens the choice of MCP servers for a workspace. */
  onOpenMcpServers: (workspaceId: string) => void;
}

interface DragItem {
  kind: "workspace" | "folder";
  id: string;
}

/**
 * Where a drag lands if released now. `row` sits beside another entry of the
 * same kind — before it, or after it when `after` is set. `into` files a
 * workspace last in a folder, `root` sends it last to the top level (or a
 * folder last among folders).
 */
type DropTarget =
  | { kind: "row"; id: string; after: boolean }
  | { kind: "into"; folderId: string }
  | { kind: "root" };

/** The context menu on screen: whose, where, and whether a key opened it. */
interface MenuState {
  kind: "workspace" | "folder";
  id: string;
  x: number;
  y: number;
  fromKeyboard: boolean;
}

/** Pointer travel before a press becomes a drag, so a click stays a click. */
const DRAG_THRESHOLD = 5;

/**
 * A context menu this soon after a press was opened by the pointer; later
 * than that, by the menu key or Shift+F10 on the focused row.
 */
const POINTER_MENU_MS = 800;

/** The click a drag's release still fires would open or fold whatever it ended on. */
function swallowNextClick() {
  const swallow = (event: MouseEvent) => event.stopPropagation();
  window.addEventListener("click", swallow, { capture: true, once: true });
  window.setTimeout(() => window.removeEventListener("click", swallow, true), 0);
}

const wantsYou = (paneId: string) => {
  const activity = getPaneActivity(paneId)?.activity;
  return activity === "waiting" || activity === "yours";
};

/**
 * The navigator: the workspaces, their folders and, under each one opened,
 * its agents and what they are doing — a source list in the manner of
 * Xcode's. It is a tree to assistive technology and to the keyboard: one tab
 * stop, arrows to move, Right and Left to open and fold, Enter to go, F2 to
 * rename, the menu key for the rest.
 */
export function Sidebar({
  onOpenSettings,
  onNewWorkspace,
  focusByWorkspace,
  onFocusPane,
  onOpenMcpServers,
}: Props) {
  const {
    state,
    activeWorkspace,
    removeWorkspace,
    updateWorkspace,
    setActiveWorkspace,
    addFolder,
    updateFolder,
    removeFolder,
    moveFolder,
    moveWorkspace,
    updateSettings,
  } = useStore();
  /** Id of the workspace or folder being renamed — ids never collide. */
  const [renaming, setRenaming] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  /** Workspace whose worktrees are on screen; it may be deleted under us. */
  const [worktreesFor, setWorktreesFor] = useState<string | null>(null);
  const [menu, setMenu] = useState<MenuState | null>(null);
  const [drag, setDrag] = useState<{ item: DragItem; target: DropTarget | null } | null>(
    null,
  );
  /**
   * Workspaces opened by hand, by id. The active one is open whatever this
   * says. A view, like the zoom: nothing here is saved.
   */
  const [opened, setOpened] = useState<Record<string, boolean>>({});
  const [query, setQuery] = useState("");
  const [attentionOnly, setAttentionOnly] = useState(false);
  /** Bumped when a pane changes state while the list depends on it. */
  const [activityTick, setActivityTick] = useState(0);
  /** The row the keyboard is on, which is the tree's one tab stop. */
  const [cursor, setCursor] = useState<string | null>(null);
  const [version, setVersion] = useState("");
  const listRef = useRef<HTMLDivElement>(null);
  const treeRef = useRef<HTMLDivElement>(null);
  const filterRef = useRef<HTMLInputElement>(null);
  /** When the pointer last went down in the tree, to tell its menus from the keyboard's. */
  const lastPress = useRef(0);
  /** Set once a rename is settled, so the blur its own Enter or Escape causes adds nothing. */
  const renameSettled = useRef(false);
  /** A row to give the focus back to once the next render has drawn it. */
  const pendingFocus = useRef<string | null>(null);
  const t = useT();
  const withKeys = useShortcutTitle();
  const uid = useId();

  // A drag is committed from listeners installed when it started; they must
  // place it against the list as it is on release, not as it was then.
  const stateRef = useRef(state);
  stateRef.current = state;

  useEffect(() => {
    // Dev builds and older permission sets may not answer; the number just stays out.
    getVersion()
      .then(setVersion)
      .catch(() => setVersion(""));
  }, []);

  const activeId = activeWorkspace?.id ?? null;
  const tree = useMemo(
    () =>
      buildNavTree({
        workspaces: state.workspaces,
        folders: state.folders,
        activeId,
        opened,
        query,
        attentionOnly,
        wantsYou,
      }),
    // `activityTick` stands for the panes' states, which `wantsYou` reads
    // outside React.
    [state.workspaces, state.folders, activeId, opened, query, attentionOnly, activityTick],
  );
  const bumpActivity = useCallback(() => setActivityTick((n) => n + 1), []);

  const rowsByKey = useMemo(() => new Map(tree.rows.map((row) => [row.key, row])), [tree]);
  const activeKey = activeId ? rowKey("ws", activeId) : null;
  const tabKey =
    cursor && rowsByKey.has(cursor)
      ? cursor
      : activeKey && rowsByKey.has(activeKey)
        ? activeKey
        : (tree.rows[0]?.key ?? null);

  const worktreesWs = state.workspaces.find((ws) => ws.id === worktreesFor) ?? null;
  const filtered = tree.filtering;

  function focusRow(key: string | null | undefined) {
    if (!key) return;
    setCursor(key);
    treeRef.current?.querySelector<HTMLElement>(`[data-key="${CSS.escape(key)}"]`)?.focus();
  }

  // After the menu: the row it was about may have been redrawn elsewhere (moved
  // into a folder) or be gone (deleted), so it is looked up again by key, and
  // the tree's tab stop stands in for one that no longer exists.
  useEffect(() => {
    const key = pendingFocus.current;
    if (!key) return;
    pendingFocus.current = null;
    focusRow(rowsByKey.has(key) ? key : tabKey);
  });

  function startRename(id: string, name: string) {
    renameSettled.current = false;
    setRenaming(id);
    setDraft(name);
  }

  /** `refocus` when a key ended it: the row gets the focus back, as before the rename. */
  function finishRename(save: boolean, refocus: boolean) {
    if (renameSettled.current || !renaming) return;
    renameSettled.current = true;
    const isFolder = state.folders.some((folder) => folder.id === renaming);
    const name = draft.trim();
    if (save && name) {
      if (isFolder) updateFolder(renaming, { name });
      else updateWorkspace(renaming, { name });
    }
    setRenaming(null);
    if (refocus) focusRow(rowKey(isFolder ? "folder" : "ws", renaming));
  }

  /** A new folder opens straight into its name, which is the first thing it needs. */
  function createFolder(): string {
    const name = t("sidebar.folderDefault");
    const id = addFolder(name);
    startRename(id, name);
    return id;
  }

  function toggleWorkspace(row: NavRow | NavWorkspace) {
    const id = "workspace" in row ? row.workspace.id : row.id;
    if (!row.foldable) return;
    setOpened((prev) => ({ ...prev, [id]: !row.expanded }));
  }

  function toggleFolder(folderId: string) {
    // Under a filter a folder is held open: folding it would hide the matches.
    if (filtered) return;
    const folder = state.folders.find((entry) => entry.id === folderId);
    if (folder) updateFolder(folderId, { collapsed: !folder.collapsed });
  }

  function clearFilter() {
    setQuery("");
    setAttentionOnly(false);
  }

  const closeMenu = useCallback(() => setMenu(null), []);

  /** At the pointer for a right-click; under the row for the menu key, which has no pointer. */
  function openMenu(kind: MenuState["kind"], id: string, anchor: Element, event: React.MouseEvent) {
    const fromKeyboard = Date.now() - lastPress.current > POINTER_MENU_MS;
    const box = anchor.getBoundingClientRect();
    setMenu({
      kind,
      id,
      x: fromKeyboard ? box.left + 24 : event.clientX,
      y: fromKeyboard ? box.bottom + 2 : event.clientY,
      fromKeyboard,
    });
  }

  /**
   * What sits under the pointer for `item`. A gap between two rows belongs to
   * no row, and keeps whatever the pointer was over just before rather than
   * making the marker blink on every row boundary.
   */
  function targetAt(
    item: DragItem,
    x: number,
    y: number,
    previous: DropTarget | null,
  ): DropTarget | null {
    const hit = document.elementFromPoint(x, y);
    const el = hit?.closest<HTMLElement>("[data-drop]");
    if (!el) return hit && listRef.current?.contains(hit) ? previous : null;

    const { drop, id = "" } = el.dataset;
    const box = el.getBoundingClientRect();
    const after = y > box.top + box.height / 2;

    if (drop === "root") return { kind: "root" };
    if (item.kind === "folder") {
      return drop === "folder" && id !== item.id ? { kind: "row", id, after } : null;
    }
    if (drop === "workspace") return id === item.id ? null : { kind: "row", id, after };
    if (drop === "folder" || drop === "folder-body") return { kind: "into", folderId: id };
    return null;
  }

  function commit(item: DragItem, target: DropTarget) {
    const { folders, workspaces } = stateRef.current;

    if (item.kind === "folder") {
      if (target.kind === "root") return moveFolder(item.id, null);
      if (target.kind !== "row") return;
      const others = folders.filter((folder) => folder.id !== item.id);
      const at = others.findIndex((folder) => folder.id === target.id);
      if (at < 0) return;
      moveFolder(item.id, target.after ? (others[at + 1]?.id ?? null) : target.id);
      return;
    }

    if (target.kind === "root") return moveWorkspace(item.id, null, null);
    if (target.kind === "into") return moveWorkspace(item.id, target.folderId, null);

    // Beside another workspace: into its folder, next to it. "After" is
    // "before the next one", or last in that folder when it is the last.
    const anchor = workspaces.find((ws) => ws.id === target.id);
    if (!anchor) return;
    const group = workspaces.filter(
      (ws) => ws.id !== item.id && ws.folderId === anchor.folderId,
    );
    const at = group.findIndex((ws) => ws.id === anchor.id);
    const beforeId = target.after ? (group[at + 1]?.id ?? null) : anchor.id;
    moveWorkspace(item.id, anchor.folderId, beforeId);
  }

  /**
   * Pointer events, like the pane grid, rather than HTML5 drag and drop: Tauri
   * handles file drops on the window itself, which keeps the webview's own
   * drop events from firing on Windows.
   */
  function beginDrag(item: DragItem, event: React.PointerEvent<HTMLElement>) {
    if (event.button !== 0) return;
    // The row's own controls and its rename field keep the pointer for themselves.
    if ((event.target as Element).closest(".nav-ctl, input")) return;

    const row = event.currentTarget;
    const { pointerId, clientX: startX, clientY: startY } = event;
    let active = false;
    let target: DropTarget | null = null;

    function move(ev: PointerEvent) {
      if (ev.pointerId !== pointerId) return;
      if (!active) {
        if (Math.hypot(ev.clientX - startX, ev.clientY - startY) < DRAG_THRESHOLD) return;
        active = true;
        document.body.classList.add("is-sorting");
        // Keeps the gesture aimed here even once the pointer leaves the window,
        // so a release over another app still ends it.
        row.setPointerCapture(pointerId);
      }
      target = targetAt(item, ev.clientX, ev.clientY, target);
      setDrag({ item, target });
    }

    function end(ev?: PointerEvent) {
      if (ev && ev.pointerId !== pointerId) return;
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", end);
      window.removeEventListener("pointercancel", end);
      window.removeEventListener("blur", lostFocus);
      if (!active) return;
      if (row.hasPointerCapture(pointerId)) row.releasePointerCapture(pointerId);
      document.body.classList.remove("is-sorting");
      setDrag(null);
      swallowNextClick();
      // Only a release drops. A cancelled gesture or a lost window puts
      // everything back where it was.
      if (ev?.type === "pointerup" && target) commit(item, target);
    }

    // A webview can lose the pointer without ever reporting it.
    const lostFocus = () => end();

    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", end);
    window.addEventListener("pointercancel", end);
    window.addEventListener("blur", lostFocus);
  }

  function activateRow(row: NavRow) {
    if (row.kind === "folder") return toggleFolder(row.id);
    if (row.kind === "ws") return setActiveWorkspace(row.id);
    const workspaceId = row.parent ? rowsByKey.get(row.parent)?.id : null;
    if (workspaceId) onFocusPane(workspaceId, row.id);
  }

  function onTreeKeyDown(event: React.KeyboardEvent<HTMLDivElement>) {
    const target = event.target as HTMLElement;
    if (target.closest("input") || event.altKey || event.ctrlKey || event.metaKey) return;
    const key = target.closest<HTMLElement>('[role="treeitem"]')?.dataset.key;
    const at = tree.rows.findIndex((row) => row.key === key);
    if (at < 0) return;
    const row = tree.rows[at];

    switch (event.key) {
      case "ArrowDown":
        focusRow(tree.rows[at + 1]?.key);
        break;
      case "ArrowUp":
        focusRow(tree.rows[at - 1]?.key);
        break;
      case "Home":
        focusRow(tree.rows[0]?.key);
        break;
      case "End":
        focusRow(tree.rows[tree.rows.length - 1]?.key);
        break;
      case "ArrowRight":
        if (row.kind === "pane") break;
        if (!row.expanded) {
          if (row.kind === "folder") toggleFolder(row.id);
          else toggleWorkspace(row);
        } else if (tree.rows[at + 1]?.parent === row.key) {
          focusRow(tree.rows[at + 1].key);
        }
        break;
      case "ArrowLeft":
        if (row.expanded && row.foldable) {
          if (row.kind === "folder") toggleFolder(row.id);
          else toggleWorkspace(row);
        } else {
          focusRow(row.parent);
        }
        break;
      case "Enter":
      case " ":
        activateRow(row);
        break;
      case "F2": {
        if (row.kind === "pane") return;
        const entity =
          row.kind === "folder"
            ? state.folders.find((folder) => folder.id === row.id)
            : state.workspaces.find((ws) => ws.id === row.id);
        if (entity) startRename(entity.id, entity.name);
        break;
      }
      default:
        return;
    }
    event.preventDefault();
  }

  /**
   * One handler for the whole tree, so the menu key on a focused row works
   * like a right-click on it. An agent row has no menu of its own; the
   * webview's (reload, inspect) has no business in the list either.
   */
  function onTreeContextMenu(event: React.MouseEvent<HTMLDivElement>) {
    const target = event.target as HTMLElement;
    if (target.closest("input")) return;
    event.preventDefault();
    const item = target.closest<HTMLElement>('[role="treeitem"]');
    const row = item?.dataset.key ? rowsByKey.get(item.dataset.key) : undefined;
    if (!item || !row || row.kind === "pane") return;
    const head = item.querySelector(".nav-ws__head, .nav-folder__head") ?? item;
    openMenu(row.kind === "folder" ? "folder" : "workspace", row.id, head, event);
  }

  function renameField(hint: string, selectAll: boolean) {
    return (
      <input
        className="nav-rename"
        autoFocus
        title={hint}
        aria-label={hint}
        value={draft}
        spellCheck={false}
        onChange={(e) => setDraft(e.target.value)}
        // A folder's name is a placeholder until typed over.
        onFocus={selectAll ? (e) => e.target.select() : undefined}
        onBlur={() => finishRename(true, false)}
        onKeyDown={(e) => {
          if (e.key === "Enter") finishRename(true, true);
          if (e.key === "Escape") {
            // The field, not whatever else listens for Escape (the zoom).
            e.stopPropagation();
            finishRename(false, true);
          }
        }}
        onClick={(e) => e.stopPropagation()}
        onDoubleClick={(e) => e.stopPropagation()}
      />
    );
  }

  /** The "…" of a row: the menu a right-click opens, for those who would not think to. */
  function moreButton(kind: MenuState["kind"], id: string) {
    return (
      <span
        className="nav-more nav-ctl"
        title={t("sidebar.more")}
        aria-hidden="true"
        onClick={(e) => {
          e.stopPropagation();
          const box = e.currentTarget.getBoundingClientRect();
          setMenu({ kind, id, x: box.left, y: box.bottom + 4, fromKeyboard: false });
        }}
      >
        <Icon name="more" size={14} />
      </span>
    );
  }

  /** Before/after marker on a row a drag of `kind` would land beside. */
  function slotClass(kind: DragItem["kind"], id: string, base: string) {
    const target = drag?.target;
    if (drag?.item.kind !== kind || target?.kind !== "row" || target.id !== id) return "";
    return `${base}--drop-${target.after ? "after" : "before"}`;
  }

  function renderWorkspace(entry: NavWorkspace, level: number) {
    const ws = entry.workspace;
    const key = rowKey("ws", ws.id);
    const active = ws.id === activeId;
    const dragged = drag?.item.kind === "workspace" && drag.item.id === ws.id;
    const id = `${uid}-${ws.id}`;
    const focusedPane = focusByWorkspace[ws.id];

    return (
      <div
        key={ws.id}
        className={[
          "nav-ws",
          active && "nav-ws--active",
          dragged && "nav-ws--dragging",
          slotClass("workspace", ws.id, "nav-ws"),
        ]
          .filter(Boolean)
          .join(" ")}
        style={{ "--nav-level": level - 1 } as React.CSSProperties}
        role="treeitem"
        aria-level={level}
        aria-expanded={entry.expanded}
        aria-current={active ? "true" : undefined}
        aria-labelledby={`${id}-name`}
        aria-describedby={`${id}-window ${id}-path ${id}-activity`}
        tabIndex={tabKey === key ? 0 : -1}
        data-key={key}
        data-drop="workspace"
        data-id={ws.id}
      >
        <div
          className="nav-ws__head"
          onClick={() => setActiveWorkspace(ws.id)}
          onPointerDown={(e) => beginDrag({ kind: "workspace", id: ws.id }, e)}
          onDoubleClick={(e) => {
            // The chevron and the "…" carry their own meaning; the rest of
            // the row opens the rename field.
            if ((e.target as Element).closest(".nav-ctl")) return;
            startRename(ws.id, ws.name);
          }}
        >
          <span
            className={[
              "nav-disc",
              entry.expanded && "nav-disc--open",
              entry.foldable && "nav-ctl",
            ]
              .filter(Boolean)
              .join(" ")}
            title={
              entry.foldable ? t(entry.expanded ? "sidebar.collapse" : "sidebar.expand") : undefined
            }
            aria-hidden="true"
            onClick={(e) => {
              // Folding is not choosing: the workspace on screen stays.
              e.stopPropagation();
              toggleWorkspace(entry);
            }}
          >
            <Icon name="chevronRight" size={12} />
          </span>
          <span className="nav-ws__icon">
            <Icon name="workspace" size={14} />
          </span>
          {renaming === ws.id ? (
            renameField(t("sidebar.renameHint"), false)
          ) : (
            <>
              <span className="nav-ws__title">
                <span className="nav-ws__name" id={`${id}-name`}>
                  {ws.name}
                </span>
                {ws.detached && (
                  <span
                    className="ws__window"
                    id={`${id}-window`}
                    role="img"
                    title={t("window.inOwnWindow")}
                    aria-label={t("window.inOwnWindow")}
                  >
                    <Icon name="window" size={12} />
                  </span>
                )}
              </span>
              <span className="nav-end">
                <span
                  className="nav-badge"
                  title={t("sidebar.paneCount", { n: ws.panes.length })}
                  aria-hidden="true"
                >
                  {ws.panes.length}
                </span>
                {moreButton("workspace", ws.id)}
              </span>
            </>
          )}
          <span className="nav-ws__path" id={`${id}-path`} title={ws.cwd}>
            <bdi>{ws.cwd}</bdi>
          </span>
          <NavigatorActivityLine workspaceId={ws.id} id={`${id}-activity`} />
        </div>

        {entry.expanded && entry.panes.length > 0 && (
          <div className="nav-ws__agents" role="group">
            {entry.panes.map((pane) => {
              const paneKey = rowKey("pane", pane.id);
              return (
                <NavigatorAgentRow
                  key={pane.id}
                  pane={pane}
                  rowKey={paneKey}
                  level={level + 1}
                  selected={active && focusedPane === pane.id}
                  tabbable={tabKey === paneKey}
                  onActivate={() => onFocusPane(ws.id, pane.id)}
                />
              );
            })}
          </div>
        )}
      </div>
    );
  }

  function renderFolder(entry: NavFolder) {
    const folder = entry.folder;
    const key = rowKey("folder", folder.id);
    const dragged = drag?.item.kind === "folder" && drag.item.id === folder.id;
    const into = drag?.target?.kind === "into" && drag.target.folderId === folder.id;
    // Folded away, the workspace on screen would otherwise be nowhere to see.
    const holdsActive =
      !entry.expanded &&
      state.workspaces.some((ws) => ws.folderId === folder.id && ws.id === activeId);
    const nameId = `${uid}-${folder.id}-name`;

    return (
      <div
        key={folder.id}
        className={`nav-folder ${dragged ? "nav-folder--dragging" : ""}`}
        role="treeitem"
        aria-level={1}
        aria-expanded={entry.expanded}
        aria-labelledby={nameId}
        tabIndex={tabKey === key ? 0 : -1}
        data-key={key}
      >
        <div
          className={[
            "nav-folder__head",
            into && "nav-folder__head--into",
            holdsActive && "nav-folder__head--current",
            slotClass("folder", folder.id, "nav-folder__head"),
          ]
            .filter(Boolean)
            .join(" ")}
          data-drop="folder"
          data-id={folder.id}
          onClick={() => toggleFolder(folder.id)}
          onPointerDown={(e) => beginDrag({ kind: "folder", id: folder.id }, e)}
        >
          <span
            className={`nav-disc ${entry.expanded ? "nav-disc--open" : ""}`}
            aria-hidden="true"
          >
            <Icon name="chevronRight" size={12} />
          </span>
          <span className="nav-folder__icon">
            <Icon name="folder" size={14} />
          </span>
          {renaming === folder.id ? (
            renameField(t("sidebar.renameHint"), true)
          ) : (
            <>
              <span className="nav-folder__name" id={nameId}>
                {folder.name}
              </span>
              <span className="nav-end">
                <span className="nav-badge" aria-hidden="true">
                  {entry.size}
                </span>
                {moreButton("folder", folder.id)}
              </span>
            </>
          )}
        </div>

        {entry.expanded && (
          <div className="nav-folder__body" role="group">
            {entry.workspaces.map((ws) => renderWorkspace(ws, 2))}
            {entry.size === 0 && (
              <p className="nav-folder__empty" data-drop="folder-body" data-id={folder.id}>
                {t("sidebar.folderEmpty")}
              </p>
            )}
          </div>
        )}
      </div>
    );
  }

  function menuEntries(current: MenuState): NavigatorMenuEntry[] {
    if (current.kind === "folder") {
      const folder = state.folders.find((entry) => entry.id === current.id);
      if (!folder) return [];
      return [
        {
          kind: "item",
          label: t("sidebar.menuRename"),
          icon: "pencil",
          takesFocus: true,
          onSelect: () => startRename(folder.id, folder.name),
        },
        { kind: "separator" },
        {
          kind: "item",
          label: t("sidebar.menuRemoveFolder"),
          title: t("sidebar.removeFolder"),
          icon: "trash",
          danger: true,
          onSelect: () => removeFolder(folder.id),
        },
      ];
    }

    const ws = state.workspaces.find((entry) => entry.id === current.id);
    if (!ws) return [];
    const filedIn =
      ws.folderId && state.folders.some((folder) => folder.id === ws.folderId)
        ? ws.folderId
        : null;
    return [
      {
        kind: "item",
        label: t("sidebar.menuRename"),
        icon: "pencil",
        takesFocus: true,
        onSelect: () => startRename(ws.id, ws.name),
      },
      {
        kind: "item",
        label: t("worktrees.action"),
        icon: "branch",
        takesFocus: true,
        onSelect: () => setWorktreesFor(ws.id),
      },
      {
        kind: "item",
        label: t("mcpScope.menu"),
        icon: "plug",
        takesFocus: true,
        onSelect: () => onOpenMcpServers(ws.id),
      },
      { kind: "separator" },
      // Dragging does the same, but a menu is the one way to find out
      // folders exist.
      { kind: "heading", label: t("sidebar.moveTo") },
      ...state.folders.map(
        (folder): NavigatorMenuEntry => ({
          kind: "item",
          label: folder.name,
          icon: "folder",
          checked: folder.id === filedIn,
          disabled: folder.id === filedIn,
          onSelect: () => moveWorkspace(ws.id, folder.id, null),
        }),
      ),
      ...(filedIn
        ? [
            {
              kind: "item",
              label: t("sidebar.noFolder"),
              onSelect: () => moveWorkspace(ws.id, null, null),
            } satisfies NavigatorMenuEntry,
          ]
        : []),
      {
        kind: "item",
        label: t("sidebar.newFolder"),
        icon: "folderPlus",
        // Straight into the new folder's name.
        takesFocus: true,
        onSelect: () => moveWorkspace(ws.id, createFolder(), null),
      },
      { kind: "separator" },
      {
        kind: "item",
        label: t("sidebar.remove"),
        icon: "trash",
        danger: true,
        onSelect: () => {
          removeWorkspace(ws.id);
          // The copies of its MCP servers' entries go with it (lib/mcpScope).
          mcpScopeClear(ws.id).catch(() => undefined);
        },
      },
    ];
  }

  const menuEntriesNow = menu ? menuEntries(menu) : [];
  const menuLabel = menu
    ? ((menu.kind === "folder" ? state.folders : state.workspaces).find(
        (entry) => entry.id === menu.id,
      )?.name ?? "")
    : "";
  const filterActive = query.trim() !== "" || attentionOnly;

  return (
    <aside className="sidebar" aria-label={t("sidebar.label")}>
      <div className="nav-top">
        <Logo size={15} />
        <span className="nav-top__name">Hangar.AI</span>
        {version && (
          <span className="nav-top__version" title={t("sidebar.version", { version })}>
            {version}
          </span>
        )}
        <button
          className="nav-tb nav-top__hide"
          title={withKeys(t("sidebar.hide"), "view.navigator")}
          aria-label={t("sidebar.hide")}
          onClick={() => updateSettings({ navigatorOpen: false })}
        >
          <Icon name="sidebarLeft" size={16} />
        </button>
      </div>

      <div className="nav-section">
        <span id={`${uid}-section`}>{t("sidebar.workspaces")}</span>
        <button
          className="nav-tb nav-section__add"
          title={t("sidebar.newFolder")}
          aria-label={t("sidebar.newFolder")}
          onClick={createFolder}
        >
          <Icon name="folderPlus" size={14} />
        </button>
      </div>

      <div className="nav-list" ref={listRef}>
        <div
          className="nav-tree"
          role="tree"
          aria-labelledby={`${uid}-section`}
          ref={treeRef}
          onKeyDown={onTreeKeyDown}
          onContextMenu={onTreeContextMenu}
          onPointerDown={() => (lastPress.current = Date.now())}
          onFocus={(e) => {
            const key = (e.target as HTMLElement).dataset.key;
            if (key && (e.target as HTMLElement).getAttribute("role") === "treeitem") {
              setCursor(key);
            }
          }}
        >
          {tree.folders.map(renderFolder)}
          {tree.loose.map((entry) => renderWorkspace(entry, 1))}
        </div>

        {state.workspaces.length === 0 && <p className="nav-empty">{t("sidebar.empty")}</p>}

        {filtered && state.workspaces.length > 0 && tree.rows.length === 0 && (
          <div className="nav-empty nav-empty--filter" role="status">
            <p>
              {query.trim()
                ? t("sidebar.noMatch", { query: query.trim() })
                : t("sidebar.noneWaiting")}
            </p>
            <button
              className="nav-link"
              onClick={() => {
                clearFilter();
                filterRef.current?.focus();
              }}
            >
              {t("sidebar.clearFilter")}
            </button>
          </div>
        )}

        {/* The space under the list: dropping there takes a workspace out of
            its folder, which no row can offer once every one is filed. */}
        <div
          className={`nav-root ${drag?.target?.kind === "root" ? "nav-root--drop" : ""}`}
          data-drop="root"
        >
          {drag?.item.kind === "workspace" && state.folders.length > 0 && t("sidebar.noFolder")}
        </div>
      </div>

      <NavigatorPlan />

      <div className="nav-filter">
        <button
          className="nav-tb"
          title={withKeys(t("cmd.workspace.new"), "workspace.new")}
          aria-label={t("cmd.workspace.new")}
          onClick={onNewWorkspace}
        >
          <Icon name="plus" size={16} />
        </button>
        <div
          className={`nav-field ${filterActive ? "nav-field--active" : ""}`}
          // The field is wider than its input: a press anywhere on it types.
          onPointerDown={(e) => {
            if (!(e.target as Element).closest("button, input")) {
              e.preventDefault();
              filterRef.current?.focus();
            }
          }}
        >
          <Icon name="search" size={14} />
          <input
            ref={filterRef}
            className="nav-field__input"
            value={query}
            placeholder={t("sidebar.filter")}
            aria-label={t("sidebar.filterLabel")}
            spellCheck={false}
            autoComplete="off"
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "ArrowDown") {
                e.preventDefault();
                focusRow(tabKey);
              }
              if (e.key !== "Escape") return;
              // First the text, then the toggle; an empty field gives the
              // focus back to the list.
              e.preventDefault();
              e.stopPropagation();
              if (query) setQuery("");
              else if (attentionOnly) setAttentionOnly(false);
              else focusRow(tabKey);
            }}
          />
          <button
            className="nav-field__scope"
            aria-pressed={attentionOnly}
            title={t("sidebar.attention")}
            aria-label={t("sidebar.attention")}
            onClick={() => setAttentionOnly((on) => !on)}
          >
            {/* The waiting state's own diamond: this keeps only what waits. */}
            <StateGlyph state="waiting" size={9} />
          </button>
        </div>
        <button
          className="nav-tb"
          title={withKeys(t("sidebar.settings"), "view.settings")}
          aria-label={t("sidebar.settings")}
          onClick={onOpenSettings}
        >
          <Icon name="gear" size={16} />
        </button>
      </div>

      {worktreesWs && (
        <WorktreePanel
          workspaceId={worktreesWs.id}
          cwd={worktreesWs.cwd}
          onClose={() => {
            const key = rowKey("ws", worktreesWs.id);
            setWorktreesFor(null);
            // Opened from the row's menu, which is gone: the dialog's own
            // restore has nowhere to go, so the row takes the keyboard back —
            // unless whatever closed the panel already put it somewhere.
            requestAnimationFrame(() => {
              const at = document.activeElement;
              if (at && at !== document.body) return;
              const row = treeRef.current?.querySelector(`[data-key="${CSS.escape(key)}"]`);
              focusRow(row ? key : tabKey);
            });
          }}
        />
      )}

      {menu && menuEntriesNow.length > 0 && (
        <NavigatorMenu
          // A new opening is a new menu, with its own opener to go back to.
          key={`${menu.kind}:${menu.id}:${menu.x}:${menu.y}`}
          x={menu.x}
          y={menu.y}
          label={menuLabel}
          entries={menuEntriesNow}
          fromKeyboard={menu.fromKeyboard}
          onClose={closeMenu}
          onRestoreFocus={() => {
            pendingFocus.current = rowKey(menu.kind === "folder" ? "folder" : "ws", menu.id);
          }}
        />
      )}

      {/* Only this view depends on every pane's state at once. */}
      {attentionOnly &&
        state.workspaces.flatMap((ws) =>
          ws.panes.map((pane) => (
            <NavigatorPaneWatch key={pane.id} paneId={pane.id} onChange={bumpActivity} />
          )),
        )}
    </aside>
  );
}
