import { useState } from "react";
import { useStore } from "../store";
import { useT } from "../i18n";
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
  const t = useT();

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
        <span className="sidebar__logo">◧</span>
        <span className="sidebar__title">IaBench</span>
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
              onDoubleClick={() => startRename(ws)}
            >
              {renaming === ws.id ? (
                <input
                  className="ws__input"
                  autoFocus
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
                  <span className="ws__badge">{ws.layout}</span>
                  <button
                    className="ws__remove"
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
    </aside>
  );
}
