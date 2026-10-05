import { useCallback, useRef, useState } from "react";
import { useStore } from "../store";
import { useT } from "../i18n";
import { Logo } from "./Logo";
import { WorktreePanel } from "./WorktreePanel";
import { FolderIcon, FolderMenu } from "./FolderMenu";
import { Icon } from "./Icon";
import { usePlanLimits, useWorkspaceActivity } from "../lib/agentState";
import type { Workspace, WorkspaceFolder } from "../types";

interface Props {
  onOpenSettings: () => void;
  onNewWorkspace: () => void;
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

/** Pointer travel before a press becomes a drag, so a click stays a click. */
const DRAG_THRESHOLD = 5;

/** The click a drag's release still fires would open or fold whatever it ended on. */
function swallowNextClick() {
  const swallow = (event: MouseEvent) => event.stopPropagation();
  window.addEventListener("click", swallow, { capture: true, once: true });
  window.setTimeout(() => window.removeEventListener("click", swallow, true), 0);
}

export function Sidebar({ onOpenSettings, onNewWorkspace }: Props) {
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
  } = useStore();
  /** Id of the workspace or folder being renamed — ids never collide. */
  const [renaming, setRenaming] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  /** Workspace whose worktrees are on screen; it may be deleted under us. */
  const [worktreesFor, setWorktreesFor] = useState<string | null>(null);
  const [menu, setMenu] = useState<{ workspaceId: string; x: number; y: number } | null>(
    null,
  );
  const [drag, setDrag] = useState<{ item: DragItem; target: DropTarget | null } | null>(
    null,
  );
  const listRef = useRef<HTMLElement>(null);
  const t = useT();

  // A drag is committed from listeners installed when it started; they must
  // place it against the list as it is on release, not as it was then.
  const stateRef = useRef(state);
  stateRef.current = state;

  const worktreesWs = state.workspaces.find((ws) => ws.id === worktreesFor) ?? null;
  const menuWs = state.workspaces.find((ws) => ws.id === menu?.workspaceId) ?? null;
  const filed = new Set(state.folders.map((folder) => folder.id));
  const loose = state.workspaces.filter((ws) => !ws.folderId || !filed.has(ws.folderId));

  function startRename(id: string, name: string) {
    setRenaming(id);
    setDraft(name);
  }

  function commitRename() {
    const name = draft.trim();
    if (renaming && name) {
      if (state.folders.some((folder) => folder.id === renaming)) {
        updateFolder(renaming, { name });
      } else {
        updateWorkspace(renaming, { name });
      }
    }
    setRenaming(null);
  }

  /** A new folder opens straight into its name, which is the first thing it needs. */
  function createFolder(): string {
    const name = t("sidebar.folderDefault");
    const id = addFolder(name);
    startRename(id, name);
    return id;
  }

  const closeMenu = useCallback(() => setMenu(null), []);

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
    // The row's buttons and its rename field keep the pointer for themselves.
    if ((event.target as Element).closest("button, input")) return;

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

  function renameField(hint: string, selectAll: boolean) {
    return (
      <input
        className="ws__input"
        autoFocus
        title={hint}
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        // A folder's name is a placeholder until typed over.
        onFocus={selectAll ? (e) => e.target.select() : undefined}
        onBlur={commitRename}
        onKeyDown={(e) => {
          if (e.key === "Enter") commitRename();
          if (e.key === "Escape") setRenaming(null);
        }}
        onClick={(e) => e.stopPropagation()}
      />
    );
  }

  /** Before/after marker on a row a drag of `kind` would land beside. */
  function slotClass(kind: DragItem["kind"], id: string, base: string) {
    const target = drag?.target;
    if (drag?.item.kind !== kind || target?.kind !== "row" || target.id !== id) return "";
    return `${base}--drop-${target.after ? "after" : "before"}`;
  }

  function renderWorkspace(ws: Workspace) {
    const active = ws.id === activeWorkspace?.id;
    const dragged = drag?.item.kind === "workspace" && drag.item.id === ws.id;
    return (
      <div
        key={ws.id}
        className={[
          "ws",
          active && "ws--active",
          dragged && "ws--dragging",
          slotClass("workspace", ws.id, "ws"),
        ]
          .filter(Boolean)
          .join(" ")}
        data-drop="workspace"
        data-id={ws.id}
        onClick={() => setActiveWorkspace(ws.id)}
        onPointerDown={(e) => beginDrag({ kind: "workspace", id: ws.id }, e)}
        onContextMenu={(e) => {
          e.preventDefault();
          setMenu({ workspaceId: ws.id, x: e.clientX, y: e.clientY });
        }}
        onDoubleClick={(e) => {
          // The row's buttons carry their own meaning; only the label
          // itself opens the rename field.
          if ((e.target as Element).closest("button")) return;
          startRename(ws.id, ws.name);
        }}
      >
        {renaming === ws.id ? (
          renameField(t("sidebar.renameHint"), false)
        ) : (
          <>
            <div className="ws__meta">
              <span className="ws__name">{ws.name}</span>
              <span className="ws__path" title={ws.cwd}>
                {ws.cwd}
              </span>
            </div>
            <WorkspaceActivityBadge workspaceId={ws.id} />
            {ws.detached && (
              <span className="ws__window" title={t("window.inOwnWindow")} aria-label={t("window.inOwnWindow")}>
                ⧉
              </span>
            )}
            <span className="ws__badge">{ws.panes.length}</span>
            {/* Double-click renames too, but nothing on the row says so. */}
            <button
              className="ws__action"
              title={t("sidebar.rename")}
              aria-label={t("sidebar.rename")}
              onClick={(e) => {
                e.stopPropagation();
                startRename(ws.id, ws.name);
              }}
            >
              <Icon name="pencil" size={12} />
            </button>
            <button
              className="ws__action"
              title={t("worktrees.action")}
              aria-label={t("worktrees.action")}
              onClick={(e) => {
                e.stopPropagation();
                setWorktreesFor(ws.id);
              }}
            >
              <Icon name="branch" size={12} />
            </button>
            <button
              className="ws__action ws__remove"
              title={t("sidebar.remove")}
              aria-label={t("sidebar.remove")}
              onClick={(e) => {
                e.stopPropagation();
                removeWorkspace(ws.id);
              }}
            >
              <Icon name="close" size={12} />
            </button>
          </>
        )}
      </div>
    );
  }

  function renderFolder(folder: WorkspaceFolder) {
    const items = state.workspaces.filter((ws) => ws.folderId === folder.id);
    const dragged = drag?.item.kind === "folder" && drag.item.id === folder.id;
    const into = drag?.target?.kind === "into" && drag.target.folderId === folder.id;
    // Folded away, the workspace on screen would otherwise be nowhere to see.
    const holdsActive =
      folder.collapsed && items.some((ws) => ws.id === activeWorkspace?.id);

    return (
      <div key={folder.id} className={`folder ${dragged ? "folder--dragging" : ""}`}>
        <div
          className={[
            "folder__head",
            into && "folder__head--into",
            holdsActive && "folder__head--current",
            slotClass("folder", folder.id, "folder__head"),
          ]
            .filter(Boolean)
            .join(" ")}
          data-drop="folder"
          data-id={folder.id}
          onClick={() => updateFolder(folder.id, { collapsed: !folder.collapsed })}
          onPointerDown={(e) => beginDrag({ kind: "folder", id: folder.id }, e)}
        >
          <span
            className={`folder__chevron ${folder.collapsed ? "" : "folder__chevron--open"}`}
          >
            ▸
          </span>
          <FolderIcon />
          {renaming === folder.id ? (
            renameField(t("sidebar.renameHint"), true)
          ) : (
            <>
              <span className="folder__name">{folder.name}</span>
              <span className="ws__badge">{items.length}</span>
              <button
                className="ws__action"
                title={t("sidebar.renameFolder")}
                aria-label={t("sidebar.renameFolder")}
                onClick={(e) => {
                  e.stopPropagation();
                  startRename(folder.id, folder.name);
                }}
              >
                <Icon name="pencil" size={12} />
              </button>
              <button
                className="ws__action ws__remove"
                title={t("sidebar.removeFolder")}
                aria-label={t("sidebar.removeFolder")}
                onClick={(e) => {
                  e.stopPropagation();
                  removeFolder(folder.id);
                }}
              >
                <Icon name="close" size={12} />
              </button>
            </>
          )}
        </div>

        {!folder.collapsed && (
          <div className="folder__body">
            {items.map(renderWorkspace)}
            {items.length === 0 && (
              <p className="folder__empty" data-drop="folder-body" data-id={folder.id}>
                {t("sidebar.folderEmpty")}
              </p>
            )}
          </div>
        )}
      </div>
    );
  }

  return (
    <aside className="sidebar">
      <div className="sidebar__brand">
        <Logo size={17} />
        <span className="sidebar__title">Hangar.AI</span>
      </div>

      <div className="sidebar__section">
        <span>{t("sidebar.workspaces")}</span>
        <button
          className="sidebar__add-folder"
          title={t("sidebar.newFolder")}
          aria-label={t("sidebar.newFolder")}
          onClick={createFolder}
        >
          <FolderIcon plus />
        </button>
      </div>

      <nav className="sidebar__list" ref={listRef}>
        {state.folders.map(renderFolder)}
        {loose.map(renderWorkspace)}

        {state.workspaces.length === 0 && (
          <p className="sidebar__empty">{t("sidebar.empty")}</p>
        )}

        {/* The space under the list: dropping there takes a workspace out of
            its folder, which no row can offer once every one is filed. */}
        <div
          className={`sidebar__root ${drag?.target?.kind === "root" ? "sidebar__root--drop" : ""}`}
          data-drop="root"
        >
          {drag?.item.kind === "workspace" && state.folders.length > 0 && t("sidebar.noFolder")}
        </div>
      </nav>

      <div className="sidebar__footer">
        <PlanLimits />
        <button className="btn btn--primary btn--block" onClick={onNewWorkspace}>
          {t("sidebar.new")}
        </button>
        <button className="btn btn--ghost btn--block" onClick={onOpenSettings}>
          <Icon name="settings" size={13} />
          {t("sidebar.settings")}
        </button>
      </div>

      {worktreesWs && (
        <WorktreePanel
          workspaceId={worktreesWs.id}
          cwd={worktreesWs.cwd}
          onClose={() => setWorktreesFor(null)}
        />
      )}

      {menu && menuWs && (
        <FolderMenu
          x={menu.x}
          y={menu.y}
          folders={state.folders}
          current={menuWs.folderId}
          onClose={closeMenu}
          onMove={(folderId) => {
            moveWorkspace(menuWs.id, folderId, null);
            setMenu(null);
          }}
          onMoveToNew={() => {
            moveWorkspace(menuWs.id, createFolder(), null);
            setMenu(null);
          }}
        />
      )}
    </aside>
  );
}

/**
 * What a workspace's agents are up to, readable from any other workspace:
 * their terminals keep running out of sight. The states that want you come
 * first and carry a count; work in progress is a dot.
 */
function WorkspaceActivityBadge({ workspaceId }: { workspaceId: string }) {
  const { working, waiting, yours } = useWorkspaceActivity(workspaceId);
  const t = useT();
  if (!working && !waiting && !yours) return null;
  const parts = [
    waiting && t("sidebar.activityWaiting", { n: waiting }),
    yours && t("sidebar.activityYours", { n: yours }),
    working && t("sidebar.activityWorking", { n: working }),
  ].filter(Boolean);
  const label = parts.join(" · ");
  return (
    <span className="sidebar__activity" title={label} role="status" aria-label={label}>
      {waiting > 0 && (
        <span className="sidebar__activity-chip sidebar__activity-chip--waiting">
          {waiting}
        </span>
      )}
      {yours > 0 && (
        <span className="sidebar__activity-chip sidebar__activity-chip--yours">{yours}</span>
      )}
      {working > 0 && <span className="sidebar__activity-dot" aria-hidden="true" />}
    </span>
  );
}

/**
 * The plan's rate-limit windows, as the last Claude Code session to report
 * saw them — one account behind every pane, so one line for all of them.
 * Nothing shows until a pane with the hangar-bridge mod has reported.
 */
function PlanLimits() {
  const plan = usePlanLimits();
  const t = useT();
  if (!plan) return null;
  const label = (kind: string) =>
    kind === "five_hour"
      ? t("sidebar.planFiveHour")
      : kind === "seven_day"
        ? t("sidebar.planWeek")
        : kind.replace(/_/g, " ");
  const resets = plan.limits
    .filter((limit) => limit.resetsAt)
    .map((limit) =>
      t("sidebar.planResets", {
        window: label(limit.kind),
        time: new Date(limit.resetsAt as string).toLocaleString(),
      }),
    );
  const title = [t("sidebar.planHint"), ...resets].join("\n");
  return (
    <div className="plan" title={title}>
      <span className="plan__label">{t("sidebar.plan")}</span>
      {plan.limits.map((limit) => {
        const pct = Math.round(limit.percentUsed);
        return (
          <span
            key={limit.kind}
            className={`plan__window ${pct >= 90 ? "plan__window--hot" : pct >= 70 ? "plan__window--warn" : ""}`}
          >
            {label(limit.kind)} <strong>{pct}%</strong>
          </span>
        );
      })}
    </div>
  );
}
