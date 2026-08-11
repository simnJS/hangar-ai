import { useState } from "react";
import { useStore } from "../store";
import { useT } from "../i18n";
import { Logo } from "./Logo";
import { WorktreePanel } from "./WorktreePanel";
import type { Workspace } from "../types";

interface Props {
  onOpenSettings: () => void;
  onNewWorkspace: () => void;
}

export function Sidebar({ onOpenSettings, onNewWorkspace }: Props) {
  const { state, activeWorkspace, removeWorkspace, updateWorkspace, setActiveWorkspace } =
    useStore();
  const [renaming, setRenaming] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  /** Workspace whose worktrees are on screen; it may be deleted under us. */
  const [worktreesFor, setWorktreesFor] = useState<string | null>(null);
  const t = useT();

  const worktreesWs = state.workspaces.find((ws) => ws.id === worktreesFor) ?? null;

  function startRename(ws: Workspace) {
    setRenaming(ws.id);
    setDraft(ws.name);
  }

  function commitRename() {
    if (renaming && draft.trim()) {
      updateWorkspace(renaming, { name: draft.trim() });
    }
    setRenaming(null);
  }

  return (
    <aside className="sidebar">
      <div className="sidebar__brand">
        <Logo size={17} />
        <span className="sidebar__title">Hangar.AI</span>
      </div>

      <div className="sidebar__section">{t("sidebar.workspaces")}</div>

      <nav className="sidebar__list">
        {state.workspaces.map((ws) => {
          const active = ws.id === activeWorkspace?.id;
          return (
            <div
              key={ws.id}
              className={`ws ${active ? "ws--active" : ""}`}
              onClick={() => setActiveWorkspace(ws.id)}
              onDoubleClick={(e) => {
                // The row's buttons carry their own meaning; only the label
                // itself opens the rename field.
                if ((e.target as Element).closest("button")) return;
                startRename(ws);
              }}
            >
              {renaming === ws.id ? (
                <input
                  className="ws__input"
                  autoFocus
                  title={t("sidebar.renameHint")}
                  value={draft}
                  onChange={(e) => setDraft(e.target.value)}
                  onBlur={commitRename}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") commitRename();
                    if (e.key === "Escape") setRenaming(null);
                  }}
                  onClick={(e) => e.stopPropagation()}
                />
              ) : (
                <>
                  <div className="ws__meta">
                    <span className="ws__name">{ws.name}</span>
                    <span className="ws__path" title={ws.cwd}>
                      {ws.cwd}
                    </span>
                  </div>
                  <span className="ws__badge">{ws.panes.length}</span>
                  {/* Double-click renames too, but nothing on the row says so. */}
                  <button
                    className="ws__action"
                    title={t("sidebar.rename")}
                    onClick={(e) => {
                      e.stopPropagation();
                      startRename(ws);
                    }}
                  >
                    ✎
                  </button>
                  <button
                    className="ws__action"
                    title={t("worktrees.action")}
                    onClick={(e) => {
                      e.stopPropagation();
                      setWorktreesFor(ws.id);
                    }}
                  >
                    ⎇
                  </button>
                  <button
                    className="ws__action ws__remove"
                    title={t("sidebar.remove")}
                    onClick={(e) => {
                      e.stopPropagation();
                      removeWorkspace(ws.id);
                    }}
                  >
                    ×
                  </button>
                </>
              )}
            </div>
          );
        })}

        {state.workspaces.length === 0 && (
          <p className="sidebar__empty">{t("sidebar.empty")}</p>
        )}
      </nav>

      <div className="sidebar__footer">
        <button className="btn btn--primary btn--block" onClick={onNewWorkspace}>
          {t("sidebar.new")}
        </button>
        <button className="btn btn--ghost btn--block" onClick={onOpenSettings}>
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
    </aside>
  );
}
