import type { CSSProperties, ReactNode } from "react";

/**
 * The pane's own icons: drawn on a 16-unit grid with a hairline stroke, so the
 * header's controls read as one set at the small sizes they sit at. Their size
 * follows the interface size like the text around them, which the app-wide
 * Icon — sized in plain pixels — does not.
 */

const PATHS = {
  bolt: <path d="M9.2 1.8 3.8 9h4l-1 5.2L12.2 7h-4z" />,
  diff: <path d="M4.8 2.8v5M2.3 5.3h5M9 11.5h4.7M12.4 3.2 3.6 12.8" />,
  zoom: <path d="M9.6 2.6h3.8v3.8M13.4 2.6 9.2 6.8M6.4 13.4H2.6V9.6M2.6 13.4l4.2-4.2" />,
  unzoom: <path d="M13.4 6.4H9.6V2.6M9.6 6.4l4-4M2.6 9.6h3.8v3.8M6.4 9.6l-4 4" />,
  more: (
    <>
      <circle cx="3.75" cy="8" r="0.9" fill="currentColor" stroke="none" />
      <circle cx="8" cy="8" r="0.9" fill="currentColor" stroke="none" />
      <circle cx="12.25" cy="8" r="0.9" fill="currentColor" stroke="none" />
    </>
  ),
  close: <path d="M4.4 4.4l7.2 7.2M11.6 4.4l-7.2 7.2" />,
  updown: <path d="M5 6.2 8 3.2l3 3M5 9.8l3 3 3-3" />,
  resume: (
    <>
      <path d="M13 8.2A5 5 0 1 1 11.3 4.4" />
      <path d="M11.6 1.9v2.8H8.8" />
    </>
  ),
  clock: (
    <>
      <circle cx="8" cy="8" r="5.75" />
      <path d="M8 4.9V8l2.2 1.4" />
    </>
  ),
  splitRight: (
    <>
      <rect x="1.75" y="2.75" width="12.5" height="10.5" rx="2.2" />
      <path d="M8 2.75v10.5" />
    </>
  ),
  splitDown: (
    <>
      <rect x="1.75" y="2.75" width="12.5" height="10.5" rx="2.2" />
      <path d="M1.75 8h12.5" />
    </>
  ),
  tray: (
    <>
      <path d="M2.4 9.2h3.1l1 1.6h3l1-1.6h3.1" />
      <path d="M2.4 9.2 4 3.6h8l1.6 5.6v3.2H2.4z" />
    </>
  ),
  warn: (
    <>
      <path d="M7.1 2.9a1 1 0 0 1 1.8 0l5.3 9.4a1 1 0 0 1-.9 1.5H2.7a1 1 0 0 1-.9-1.5z" />
      <path d="M8 6.4v3" />
      <circle cx="8" cy="11.5" r="0.5" fill="currentColor" stroke="none" />
    </>
  ),
  bell: (
    <>
      <path d="M4.2 11.2V7.4a3.8 3.8 0 0 1 7.6 0v3.8l1.2 1.3H3z" />
      <path d="M6.6 13.6a1.5 1.5 0 0 0 2.8 0" />
    </>
  ),
} satisfies Record<string, ReactNode>;

export type PaneIconName = keyof typeof PATHS;

export function PaneIcon({ name, size = 14 }: { name: PaneIconName; size?: number }) {
  return (
    <svg
      className="pane-icon"
      style={{ "--pane-icon": `${size}px` } as CSSProperties}
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.35}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
    >
      {PATHS[name]}
    </svg>
  );
}
