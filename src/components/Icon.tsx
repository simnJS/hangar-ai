/**
 * The app's icon set: inline SVG, one stroke weight, drawn in `currentColor`
 * so every icon follows the theme and the hover colour of its button.
 *
 * Paths after Lucide (ISC licence), redrawn on a 24-unit grid. An icon is
 * decoration: the button around it carries the label, so the SVG itself is
 * hidden from assistive technology.
 */
const PATHS = {
  // Toolbar, status bar and command palette. Drawn for 16px, where a 2-unit
  // stroke comes out at the 1.33px hairline the rest of the chrome uses.
  layout1: <rect x="3" y="4.5" width="18" height="15" rx="2.7" />,
  layout2: (
    <>
      <rect x="3" y="4.5" width="18" height="15" rx="2.7" />
      <path d="M12 4.5v15" />
    </>
  ),
  layout4: (
    <>
      <rect x="3" y="4.5" width="18" height="15" rx="2.7" />
      <path d="M12 4.5v15M3 12h18" />
    </>
  ),
  layout8: (
    <>
      <rect x="2.25" y="4.5" width="19.5" height="15" rx="2.7" />
      <path d="M7.13 4.5v15M12 4.5v15M16.88 4.5v15M2.25 12h19.5" />
    </>
  ),
  tidy: (
    <>
      <rect x="3.4" y="4.1" width="7.35" height="6.6" rx="1.65" />
      <rect x="13.25" y="4.1" width="7.35" height="6.6" rx="1.65" />
      <rect x="3.4" y="13.3" width="7.35" height="6.6" rx="1.65" />
      <rect x="13.25" y="13.3" width="7.35" height="6.6" rx="1.65" />
    </>
  ),
  sidebarLeft: (
    <>
      <rect x="2.6" y="4.1" width="18.8" height="15.8" rx="3.3" />
      <path d="M9 4.1v15.8M5.4 7.8h1.35M5.4 10.8h1.35" />
    </>
  ),
  sidebarRight: (
    <>
      <rect x="2.6" y="4.1" width="18.8" height="15.8" rx="3.3" />
      <path d="M15 4.1v15.8M17.25 7.8h1.35M17.25 10.8h1.35" />
    </>
  ),
  boardPanel: (
    <>
      <rect x="2.6" y="4.1" width="18.8" height="15.8" rx="3.3" />
      <path d="M14.25 4.1v15.8M16.8 8.25h2.1M16.8 11.55h2.1M16.8 14.85h2.1" />
    </>
  ),
  board: (
    <>
      <rect x="2.6" y="4.1" width="18.8" height="15.8" rx="3.3" />
      <path d="M8.85 4.1v15.8M15.15 4.1v15.8" />
    </>
  ),
  terminal: (
    <>
      <rect x="2.6" y="4.1" width="18.8" height="15.8" rx="3.3" />
      <path d="m6.9 9.3 3 2.7-3 2.7M12.3 15.3h4.8" />
    </>
  ),
  memory: (
    <path d="M4.5 5.25A2.25 2.25 0 0 1 6.75 3h10.5a2.25 2.25 0 0 1 2.25 2.25v14.55l-7.5-3.9-7.5 3.9z" />
  ),
  more: (
    <>
      <circle cx="5.6" cy="12" r="0.9" fill="currentColor" />
      <circle cx="12" cy="12" r="0.9" fill="currentColor" />
      <circle cx="18.4" cy="12" r="0.9" fill="currentColor" />
    </>
  ),
  search: (
    <>
      <circle cx="10.5" cy="10.5" r="6.6" />
      <path d="m15.45 15.45 4.95 4.95" />
    </>
  ),
  broadcast: (
    <>
      <circle cx="12" cy="12" r="1.65" />
      <path d="M8.1 8.1a5.55 5.55 0 0 0 0 7.8M15.9 8.1a5.55 5.55 0 0 1 0 7.8M4.95 4.95a9.9 9.9 0 0 0 0 14.1M19.05 4.95a9.9 9.9 0 0 1 0 14.1" />
    </>
  ),
  send: <path d="M20.4 3.6 10.5 13.5M20.4 3.6l-6.3 16.8-3.6-6.9-6.9-3.6z" />,
  clock: (
    <>
      <circle cx="12" cy="12" r="8.6" />
      <path d="M12 7.35V12l3.3 2.1" />
    </>
  ),
  mic: (
    <>
      <rect x="8.85" y="2.85" width="6.3" height="11.4" rx="3.15" />
      <path d="M5.4 11.4a6.6 6.6 0 0 0 13.2 0M12 18v3.3" />
    </>
  ),
  check: <path d="m5.1 12.9 4.5 4.5 9.3-10.8" />,
  plug: (
    <path d="M9 2.85v4.5M15 2.85v4.5M6.45 7.35h11.1v3.75a5.55 5.55 0 0 1-11.1 0zM12 16.65v4.5" />
  ),
  expand: <path d="M14.4 3.9h5.7v5.7M20.1 3.9l-6.3 6.3M9.6 20.1H3.9v-5.7M3.9 20.1l6.3-6.3" />,
  folder: (
    <path d="M3 6.9a2.4 2.4 0 0 1 2.4-2.4h3.75l2.25 2.4h7.2A2.4 2.4 0 0 1 21 9.3v7.8a2.4 2.4 0 0 1-2.4 2.4H5.4A2.4 2.4 0 0 1 3 17.1z" />
  ),
  window: (
    <>
      <rect x="3" y="8.25" width="13.5" height="11.25" rx="2.25" />
      <path d="M7.5 5.25A2.25 2.25 0 0 1 9.75 3h9A2.25 2.25 0 0 1 21 5.25v8.25a2.25 2.25 0 0 1-2.25 2.25" />
    </>
  ),
  windowReturn: <path d="M19.5 5.25v6.3a3 3 0 0 1-3 3H5.25M9.3 10.2l-4.05 4.35 4.05 4.2" />,
  arrowRight: <path d="M4.5 12h15M13.5 6l6 6-6 6" />,

  zap: (
    <path d="M4 14a1 1 0 0 1-.78-1.63l9.9-10.2a.5.5 0 0 1 .86.46l-1.92 6.02A1 1 0 0 0 13 10h7a1 1 0 0 1 .78 1.63l-9.9 10.2a.5.5 0 0 1-.86-.46l1.92-6.02A1 1 0 0 0 11 14z" />
  ),
  restart: (
    <>
      <path d="M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8" />
      <path d="M21 3v5h-5" />
    </>
  ),
  reset: (
    <>
      <path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8" />
      <path d="M3 3v5h5" />
    </>
  ),
  split: (
    <>
      <rect width="18" height="18" x="3" y="3" rx="2" />
      <path d="M12 3v18" />
    </>
  ),
  close: (
    <>
      <path d="M18 6 6 18" />
      <path d="m6 6 12 12" />
    </>
  ),
  plus: (
    <>
      <path d="M5 12h14" />
      <path d="M12 5v14" />
    </>
  ),
  history: (
    <>
      <path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8" />
      <path d="M3 3v5h5" />
      <path d="M12 7v5l4 2" />
    </>
  ),
  bell: (
    <>
      <path d="M10.27 21a2 2 0 0 0 3.46 0" />
      <path d="M3.26 15.33A1 1 0 0 0 4 17h16a1 1 0 0 0 .74-1.67C19.41 13.96 18 12.5 18 8A6 6 0 0 0 6 8c0 4.5-1.41 5.96-2.74 7.33" />
    </>
  ),
  alert: (
    <>
      <path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3" />
      <path d="M12 9v4" />
      <path d="M12 17h.01" />
    </>
  ),
  hourglass: (
    <>
      <path d="M5 22h14" />
      <path d="M5 2h14" />
      <path d="M17 22v-4.17a2 2 0 0 0-.59-1.42L12 12l-4.41 4.41A2 2 0 0 0 7 17.83V22" />
      <path d="M7 2v4.17a2 2 0 0 0 .59 1.42L12 12l4.41-4.41A2 2 0 0 0 17 6.17V2" />
    </>
  ),
  branch: (
    <>
      <path d="M6 3v12" />
      <circle cx="18" cy="6" r="3" />
      <circle cx="6" cy="18" r="3" />
      <path d="M18 9a9 9 0 0 1-9 9" />
    </>
  ),
  diff: (
    <>
      <path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z" />
      <path d="M9 10h6" />
      <path d="M12 13V7" />
      <path d="M9 17h6" />
    </>
  ),
  settings: (
    <>
      <path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z" />
      <circle cx="12" cy="12" r="3" />
    </>
  ),
  pencil: (
    <path d="M21.17 6.81a1 1 0 0 0-3.99-3.99L3.84 16.17a2 2 0 0 0-.5.83l-1.32 4.35a.5.5 0 0 0 .62.62l4.35-1.32a2 2 0 0 0 .83-.5z" />
  ),
  folderPlus: (
    <>
      <path d="M12 10v6" />
      <path d="M9 13h6" />
      <path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z" />
    </>
  ),
} as const;

export type IconName = keyof typeof PATHS;

export function Icon({ name, size = 14 }: { name: IconName; size?: number }) {
  return (
    <svg
      className="icon"
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
    >
      {PATHS[name]}
    </svg>
  );
}
