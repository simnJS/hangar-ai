import { useEffect, useRef, type ReactNode } from "react";

/**
 * Dismisses only when the press *and* the release both land on the backdrop.
 *
 * A plain onClick would fire whenever mousedown and mouseup share an ancestor:
 * selecting text inside the dialog and releasing outside it would close the
 * dialog mid-edit.
 */
export function Backdrop({
  onClose,
  children,
}: {
  onClose: () => void;
  children: ReactNode;
}) {
  const pressedBackdrop = useRef(false);

  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if (event.key === "Escape") onClose();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div
      className="modal-backdrop"
      onMouseDown={(event) => {
        pressedBackdrop.current = event.target === event.currentTarget;
      }}
      onMouseUp={(event) => {
        const shouldClose = pressedBackdrop.current && event.target === event.currentTarget;
        pressedBackdrop.current = false;
        if (shouldClose) onClose();
      }}
    >
      {children}
    </div>
  );
}
