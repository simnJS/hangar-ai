import type { CSSProperties } from "react";

/**
 * The phone's icons, drawn like the desktop's (src/components/Icon.tsx): a
 * 16-unit grid, a 1.35-unit stroke with round caps, `currentColor`. Several
 * paths are the desktop's own. An icon is decoration: the control around it
 * carries the label.
 */
const PATHS = {
  // For you: what waits in the tray.
  tray: (
    <>
      <path d="M2.4 9.2h3.1l1 1.6h3l1-1.6h3.1" />
      <path d="M2.4 9.2 4 3.6h8l1.6 5.6v3.2H2.4z" />
    </>
  ),
  // Projects: the logo's panes.
  panes: (
    <>
      <rect x="2" y="2.5" width="5" height="11" rx="1" />
      <rect x="9" y="2.5" width="5" height="4.6" rx="1" />
      <rect x="9" y="8.9" width="5" height="4.6" rx="1" />
    </>
  ),
  // Shared: a picture.
  image: (
    <>
      <rect x="2" y="2.75" width="12" height="10.5" rx="2" />
      <circle cx="5.6" cy="6.2" r="1.1" />
      <path d="m2.4 12.2 3.6-3.4 2.4 2.2 2.3-2.1 3 2.9" />
    </>
  ),
  // The project manager: one pane over the others.
  orchestrator: (
    <>
      <circle cx="8" cy="3.6" r="1.7" />
      <circle cx="3.6" cy="12.4" r="1.7" />
      <circle cx="12.4" cy="12.4" r="1.7" />
      <path d="M8 5.3V8l-3.2 3.2M8 8l3.2 3.2" />
    </>
  ),
  clock: (
    <>
      <circle cx="8" cy="8" r="5.75" />
      <path d="M8 4.9V8l2.2 1.4" />
    </>
  ),
  moon: <path d="M8 2.25a3.85 3.85 0 0 0 5.75 5.75A5.75 5.75 0 1 1 8 2.25z" />,
  alert: (
    <>
      <path d="M7.1 2.9a1 1 0 0 1 1.8 0l5.3 9.4a1 1 0 0 1-.9 1.5H2.7a1 1 0 0 1-.9-1.5z" />
      <path d="M8 6.4v3" />
      <circle cx="8" cy="11.5" r=".35" fill="currentColor" />
    </>
  ),
  flag: <path d="M3.75 14V2.5M3.75 3h8.4l-1.7 2.9 1.7 2.9H3.75" />,
  chevronRight: <path d="M6 3.5 10.5 8 6 12.5" />,
  chevronLeft: <path d="M10 3.5 5.5 8l4.5 4.5" />,
  arrowDown: <path d="M8 3v10M4 9l4 4 4-4" />,
  arrowUp: <path d="M8 13V3M4 7l4-4 4 4" />,
  // A branch landing on another.
  arrowRight: <path d="M3 8h10M9 4l4 4-4 4" />,
  paperclip: (
    <path d="m13.2 7.6-5.4 5.4a3.2 3.2 0 0 1-4.6-4.6l5.6-5.6a2.1 2.1 0 0 1 3 3l-5.5 5.5a1 1 0 0 1-1.5-1.5l5-5" />
  ),
  // iOS's Share: a box with an arrow out of it.
  share: (
    <>
      <path d="M8 1.9v8M5.3 4.4 8 1.7l2.7 2.7" />
      <path d="M5.4 6.6H4.3a1 1 0 0 0-1 1v5.6a1 1 0 0 0 1 1h7.4a1 1 0 0 0 1-1V7.6a1 1 0 0 0-1-1h-1.1" />
    </>
  ),
  check: <path d="m3.5 8.4 2.9 2.9 6.1-6.6" />,
  chevronDown: <path d="M3.5 6 8 10.5 12.5 6" />,
  // The chat's actions, by what they do to the project.
  pencil: (
    <>
      <path d="M10.7 2.8a1.5 1.5 0 0 1 2.1 0l.4.4a1.5 1.5 0 0 1 0 2.1L6 12.5l-3.3.8.8-3.3z" />
      <path d="m9.4 4.1 2.5 2.5" />
    </>
  ),
  terminal: (
    <>
      <rect x="1.75" y="2.75" width="12.5" height="10.5" rx="2" />
      <path d="m4.6 6.3 2 1.7-2 1.7M8.3 9.9h3.1" />
    </>
  ),
  file: (
    <>
      <path d="M4.25 1.9h4.9l2.6 2.6v9.6h-7.5z" />
      <path d="M9 2v2.7h2.7M6.2 8.3h3.6M6.2 10.8h3.6" />
    </>
  ),
  search: (
    <>
      <circle cx="7.1" cy="7.1" r="4.35" />
      <path d="m10.3 10.3 3.3 3.3" />
    </>
  ),
  tool: (
    <path d="M10.1 2.2a3.1 3.1 0 0 0-3.6 4.1l-4 4a1.4 1.4 0 0 0 2 2l4-4a3.1 3.1 0 0 0 4.1-3.6l-1.8 1.8-1.7-.4-.4-1.7z" />
  ),
  failed: (
    <>
      <circle cx="8" cy="8" r="5.75" />
      <path d="m6.1 6.1 3.8 3.8M9.9 6.1 6.1 9.9" />
    </>
  ),
  stop: <rect x="4.25" y="4.25" width="7.5" height="7.5" rx="1.6" />,
  // Notifications.
  bell: (
    <>
      <path d="M4.1 11.2V7.4a3.9 3.9 0 0 1 7.8 0v3.8l1.3 1.4H2.8z" />
      <path d="M6.6 14.2h2.8" />
    </>
  ),
  // Close a viewer.
  close: <path d="m4 4 8 8M12 4l-8 8" />,
  // Stats: columns on a baseline.
  chart: <path d="M2.25 13.75h11.5M4.5 11V8.25M8 11V3.75M11.5 11V6.25" />,
} as const;

export type IconName = keyof typeof PATHS;

export function Icon({ name, size, className }: { name: IconName; size?: number; className?: string }) {
  return (
    <svg
      className={className ? `icon ${className}` : "icon"}
      style={size ? ({ "--icon": `${size}px` } as CSSProperties) : undefined}
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

/** The mark: a workspace cut into panes, one of them active (src/components/Logo.tsx). */
export function Logo({ size = 20 }: { size?: number }) {
  return (
    <svg className="logo" width={size} height={size} viewBox="0 0 32 32" aria-hidden="true" focusable="false">
      <rect className="logo__active" x="4" y="4" width="10" height="24" />
      <rect x="17" y="4" width="11" height="10" />
      <rect x="17" y="17" width="11" height="11" />
    </svg>
  );
}
