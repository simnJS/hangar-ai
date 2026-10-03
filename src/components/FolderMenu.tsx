import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useT } from "../i18n";
import type { WorkspaceFolder } from "../types";

/** Kept between the menu and the window edges. */
const MARGIN = 8;

export function FolderIcon({ plus = false }: { plus?: boolean }) {
  return (
    <svg className="folder-icon" width="14" height="14" viewBox="0 0 16 16" aria-hidden="true">
      <path
        d="M1.5 3.5h4.3l1.5 1.6h7.2v7.4h-13z"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.2"
      />
      {plus && <path d="M8 6.8v4M6 8.8h4" stroke="currentColor" strokeWidth="1.2" />}
    </svg>
  );
}

interface Props {
  /** Where the right click landed, in window coordinates. */
  x: number;
  y: number;
  folders: WorkspaceFolder[];
  /** The folder the workspace is filed under, null at the top level. */
  current: string | null;
  onMove: (folderId: string | null) => void;
  onMoveToNew: () => void;
  onClose: () => void;
}

/**
 * The right-click menu of a sidebar workspace: which folder it is filed under.
 * Dragging does the same, but a menu is the one way to find out folders exist.
 */
export function FolderMenu({ x, y, folders, current, onMove, onMoveToNew, onClose }: Props) {
  const menuRef = useRef<HTMLDivElement>(null);
  const [at, setAt] = useState<{ left: number; top: number } | null>(null);
  const t = useT();

  // Opens at the pointer, pulled back inside the window when a click near the
  // bottom would leave it hanging off the edge.
  useLayoutEffect(() => {
    const menu = menuRef.current;
    if (!menu) return;
    const { width, height } = menu.getBoundingClientRect();
    setAt({
      left: Math.max(MARGIN, Math.min(x, window.innerWidth - width - MARGIN)),
      top: Math.max(MARGIN, Math.min(y, window.innerHeight - height - MARGIN)),
    });
  }, [x, y]);

  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if (event.key === "Escape") onClose();
    }
    function onPointer(event: PointerEvent) {
      if (!menuRef.current?.contains(event.target as Node)) onClose();
    }
    // Capture, so a press on a terminal closes the menu before xterm takes the
    // pointer for a selection.
    window.addEventListener("keydown", onKey);
    window.addEventListener("pointerdown", onPointer, true);
    window.addEventListener("blur", onClose);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("pointerdown", onPointer, true);
      window.removeEventListener("blur", onClose);
    };
  }, [onClose]);

  return (
    <div
      className="ctx-menu"
      ref={menuRef}
      role="menu"
      aria-label={t("sidebar.moveTo")}
      style={{
        left: at?.left ?? 0,
        top: at?.top ?? 0,
        // Hidden for the one frame it takes to measure itself.
        visibility: at ? undefined : "hidden",
      }}
    >
      <span className="ctx-menu__head">{t("sidebar.moveTo")}</span>

      {folders.map((folder) => {
        const here = folder.id === current;
        return (
          <button
            key={folder.id}
            className="ctx-menu__item"
            role="menuitemradio"
            aria-checked={here}
            disabled={here}
            onClick={() => onMove(folder.id)}
          >
            <FolderIcon />
            <span className="ctx-menu__label">{folder.name}</span>
            {here && <span className="ctx-menu__check">✓</span>}
          </button>
        );
      })}

      {current !== null && (
        <button className="ctx-menu__item" role="menuitem" onClick={() => onMove(null)}>
          <span className="ctx-menu__label">{t("sidebar.noFolder")}</span>
        </button>
      )}

      <button
        className="ctx-menu__item ctx-menu__add"
        role="menuitem"
        onClick={onMoveToNew}
      >
        {t("sidebar.moveToNew")}
      </button>
    </div>
  );
}
