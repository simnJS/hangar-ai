import type { CSSProperties } from "react";

/**
 * The app's one icon set: inline SVG drawn in `currentColor`, so every icon
 * follows the theme and the hover colour of whatever holds it.
 *
 * Every glyph sits on the same 16-unit grid with the same 1.35-unit stroke,
 * round caps and round joins: at the 12–16px the chrome uses, that stroke
 * lands between 1 and 1.35px, the weight of the text beside it, the way SF
 * Symbols match their label. Glyphs that frame a window share one rounded
 * rectangle (12.5 × 10.5, radius 2.2), so a toolbar of them lines up.
 *
 * Some paths are after Lucide (ISC licence), redrawn on this grid. An icon is
 * decoration: the control around it carries the label, so the SVG itself is
 * hidden from assistive technology.
 */
const PATHS = {
  // Window chrome: panels, layouts and views, all in the shared frame.
  layout1: <rect x="1.75" y="2.75" width="12.5" height="10.5" rx="2.2" />,
  layout2: (
    <>
      <rect x="1.75" y="2.75" width="12.5" height="10.5" rx="2.2" />
      <path d="M8 2.75v10.5" />
    </>
  ),
  layout4: (
    <>
      <rect x="1.75" y="2.75" width="12.5" height="10.5" rx="2.2" />
      <path d="M8 2.75v10.5M1.75 8h12.5" />
    </>
  ),
  // Four columns need the frame a little wider to stay apart.
  layout8: (
    <>
      <rect x="1.25" y="2.75" width="13.5" height="10.5" rx="2.2" />
      <path d="M4.63 2.75v10.5M8 2.75v10.5M11.38 2.75v10.5M1.25 8h13.5" />
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
  tidy: (
    <>
      <rect x="2.27" y="2.73" width="4.9" height="4.4" rx="1.1" />
      <rect x="8.83" y="2.73" width="4.9" height="4.4" rx="1.1" />
      <rect x="2.27" y="8.87" width="4.9" height="4.4" rx="1.1" />
      <rect x="8.83" y="8.87" width="4.9" height="4.4" rx="1.1" />
    </>
  ),
  sidebarLeft: (
    <>
      <rect x="1.75" y="2.75" width="12.5" height="10.5" rx="2.2" />
      <path d="M6 2.75v10.5M3.6 5.2h.9M3.6 7.2h.9" />
    </>
  ),
  sidebarRight: (
    <>
      <rect x="1.75" y="2.75" width="12.5" height="10.5" rx="2.2" />
      <path d="M10 2.75v10.5M11.5 5.2h.9M11.5 7.2h.9" />
    </>
  ),
  boardPanel: (
    <>
      <rect x="1.75" y="2.75" width="12.5" height="10.5" rx="2.2" />
      <path d="M9.5 2.75v10.5M11.2 5.5h1.4M11.2 7.7h1.4M11.2 9.9h1.4" />
    </>
  ),
  board: (
    <>
      <rect x="1.75" y="2.75" width="12.5" height="10.5" rx="2.2" />
      <path d="M5.9 2.75v10.5M10.1 2.75v10.5" />
    </>
  ),
  terminal: (
    <>
      <rect x="1.75" y="2.75" width="12.5" height="10.5" rx="2.2" />
      <path d="m4.6 6.2 2 1.8-2 1.8M8.2 10.2h3.2" />
    </>
  ),
  workspace: (
    <>
      <rect x="1.75" y="2.75" width="12.5" height="10.5" rx="2.2" />
      <path d="M7.2 2.75v10.5M7.2 8h7.05" />
    </>
  ),
  window: (
    <>
      <rect x="1.75" y="5" width="9.5" height="8.25" rx="1.6" />
      <path d="M4.75 5v-.65a1.6 1.6 0 0 1 1.6-1.6h6.3a1.6 1.6 0 0 1 1.6 1.6v5.3a1.6 1.6 0 0 1-1.6 1.6h-1.4" />
    </>
  ),
  windowReturn: <path d="M13 3.5v4.2a2 2 0 0 1-2 2H3.5M6.2 6.8l-2.7 2.9 2.7 2.8" />,

  // Things.
  folder: (
    <>
      <path d="M2 4.6A1.6 1.6 0 0 1 3.6 3h2.5l1.5 1.6h4.8A1.6 1.6 0 0 1 14 6.2v5.2a1.6 1.6 0 0 1-1.6 1.6H3.6A1.6 1.6 0 0 1 2 11.4z" />
      <path d="M2 6.6h12" />
    </>
  ),
  folderPlus: (
    <>
      <path d="M8 13H3.6A1.6 1.6 0 0 1 2 11.4V4.6A1.6 1.6 0 0 1 3.6 3h2.5l1.5 1.6h4.8A1.6 1.6 0 0 1 14 6.2V8" />
      <path d="M12 10v4M10 12h4" />
    </>
  ),
  bookmark: <path d="M3 3.5A1.5 1.5 0 0 1 4.5 2h7a1.5 1.5 0 0 1 1.5 1.5v9.7l-5-2.6-5 2.6z" />,
  trash: (
    <path d="M2.75 4.25h10.5M6.25 4.25v-1.5h3.5v1.5M4.25 4.25l.6 8.3a1 1 0 0 0 1 .95h4.3a1 1 0 0 0 1-.95l.6-8.3" />
  ),
  pencil: <path d="M10.9 2.6a1.4 1.4 0 0 1 2 0l.5.5a1.4 1.4 0 0 1 0 2L6 12.5l-3 .75.75-3z" />,
  branch: (
    <>
      <circle cx="4.75" cy="3.75" r="1.5" />
      <circle cx="4.75" cy="12.25" r="1.5" />
      <circle cx="11.25" cy="5.25" r="1.5" />
      <path d="M4.75 5.25v5.5M11.25 6.75c0 2.6-2.2 3.2-6.2 4.1" />
    </>
  ),
  plug: <path d="M6 1.9v3M10 1.9v3M4.3 4.9h7.4v2.5a3.7 3.7 0 0 1-7.4 0zM8 11.1v3" />,
  lock: (
    <>
      <rect x="2.67" y="7.33" width="10.67" height="7.33" rx="1.33" />
      <path d="M5.33 7.33V4.67a2.67 2.67 0 0 1 5.34 0v2.66" />
    </>
  ),
  link: (
    <path d="M6.67 8.67a3.33 3.33 0 0 0 5.03.36l2-2a3.33 3.33 0 0 0-4.71-4.71l-1.15 1.14M9.33 7.33a3.33 3.33 0 0 0-5.03-.36l-2 2a3.33 3.33 0 0 0 4.71 4.71l1.14-1.14" />
  ),
  bubble: (
    <path d="M14 10a1.33 1.33 0 0 1-1.33 1.33H4.67L2 14V3.33A1.33 1.33 0 0 1 3.33 2h9.34A1.33 1.33 0 0 1 14 3.33z" />
  ),
  tray: (
    <>
      <path d="M2.4 9.2h3.1l1 1.6h3l1-1.6h3.1" />
      <path d="M2.4 9.2 4 3.6h8l1.6 5.6v3.2H2.4z" />
    </>
  ),
  bell: (
    <>
      <path d="M6.85 14a1.33 1.33 0 0 0 2.3 0" />
      <path d="M2.17 10.22a.67.67 0 0 0 .5 1.11h10.66a.67.67 0 0 0 .5-1.11C12.94 9.31 12 8.33 12 5.33a4 4 0 0 0-8 0c0 3-.94 3.97-1.83 4.89" />
    </>
  ),
  mic: (
    <>
      <rect x="5.9" y="1.9" width="4.2" height="7.6" rx="2.1" />
      <path d="M3.6 7.6a4.4 4.4 0 0 0 8.8 0M8 12v2.2" />
    </>
  ),
  clock: (
    <>
      <circle cx="8" cy="8" r="5.75" />
      <path d="M8 4.9V8l2.2 1.4" />
    </>
  ),
  gear: (
    <>
      <path d="M8.15 1.33h-.29a1.33 1.33 0 0 0-1.33 1.33v.12a1.33 1.33 0 0 1-.67 1.15l-.29.17a1.33 1.33 0 0 1-1.33 0l-.1-.05a1.33 1.33 0 0 0-1.82.49l-.15.25a1.33 1.33 0 0 0 .49 1.82l.1.07a1.33 1.33 0 0 1 .67 1.15v.34a1.33 1.33 0 0 1-.67 1.16l-.1.06a1.33 1.33 0 0 0-.49 1.82l.15.25a1.33 1.33 0 0 0 1.82.49l.1-.05a1.33 1.33 0 0 1 1.33 0l.29.17a1.33 1.33 0 0 1 .67 1.15v.12a1.33 1.33 0 0 0 1.33 1.33h.29a1.33 1.33 0 0 0 1.33-1.33v-.12a1.33 1.33 0 0 1 .67-1.15l.29-.17a1.33 1.33 0 0 1 1.33 0l.1.05a1.33 1.33 0 0 0 1.82-.49l.15-.26a1.33 1.33 0 0 0-.49-1.82l-.1-.05a1.33 1.33 0 0 1-.67-1.16v-.33a1.33 1.33 0 0 1 .67-1.16l.1-.06a1.33 1.33 0 0 0 .49-1.82l-.15-.25a1.33 1.33 0 0 0-1.82-.49l-.1.05a1.33 1.33 0 0 1-1.33 0l-.29-.17a1.33 1.33 0 0 1-.67-1.15v-.12a1.33 1.33 0 0 0-1.33-1.33z" />
      <circle cx="8" cy="8" r="2" />
    </>
  ),
  sliders: (
    <path d="M14 2.67H9.33M6.67 2.67H2M14 8H8M5.33 8H2M14 13.33h-3.33M8 13.33H2M9.33 1.33V4M5.33 6.67v2.66M10.67 12v2.67" />
  ),
  appearance: (
    <>
      <circle cx="8" cy="8" r="6" />
      <path d="M8 2a6 6 0 0 1 0 12z" fill="currentColor" stroke="none" />
    </>
  ),
  prompt: <path d="m3.33 11.33 3.34-3.33-3.34-3.33M8 12.67h4.67" />,
  gamepad: (
    <>
      <path d="M4 7.33h2.67M5.33 6v2.67M10 8h.01M12 6.67h.01" />
      <path d="M11.55 3.33h-7.1a2.67 2.67 0 0 0-2.65 2.39c-.07.56-.47 3.92-.47 4.95a2 2 0 0 0 2 2c.67 0 1-.34 1.34-.67l.94-.94a1.33 1.33 0 0 1 .94-.39h2.9a1.33 1.33 0 0 1 .94.39l.94.94c.33.33.67.67 1.33.67a2 2 0 0 0 2-2c0-1.03-.4-4.39-.46-4.84a2.67 2.67 0 0 0-2.65-2.5z" />
    </>
  ),
  keyboard: (
    <>
      <rect x="1.33" y="3.33" width="13.34" height="9.34" rx="1.67" />
      <path d="M4 6h.01M6.67 6h.01M9.33 6h.01M12 6h.01M5.33 8.67h.01M8 8.67h.01M10.67 8.67h.01M5 10.67h6" />
    </>
  ),
  info: (
    <>
      <circle cx="8" cy="8" r="6" />
      <path d="M8 11V7.33M8 5h.01" />
    </>
  ),
  broadcast: (
    <>
      <circle cx="8" cy="8" r="1.1" />
      <path d="M5.4 5.4a3.7 3.7 0 0 0 0 5.2M10.6 5.4a3.7 3.7 0 0 1 0 5.2M3.3 3.3a6.6 6.6 0 0 0 0 9.4M12.7 3.3a6.6 6.6 0 0 1 0 9.4" />
    </>
  ),
  send: <path d="M13.6 2.4 7 9M13.6 2.4l-4.2 11.2-2.4-4.6-4.6-2.4z" />,
  download: <path d="M8 2.67V10M4.67 7 8 10.33 11.33 7M3.33 13.33h9.34" />,

  // Marks and actions.
  plus: <path d="M8 3.25v9.5M3.25 8h9.5" />,
  minus: <path d="M3.25 8h9.5" />,
  close: <path d="m4.5 4.5 7 7m0-7-7 7" />,
  check: <path d="m3.5 8.4 2.9 2.9 6.1-6.6" />,
  more: (
    <>
      <circle cx="3.6" cy="8" r=".9" fill="currentColor" stroke="none" />
      <circle cx="8" cy="8" r=".9" fill="currentColor" stroke="none" />
      <circle cx="12.4" cy="8" r=".9" fill="currentColor" stroke="none" />
    </>
  ),
  search: (
    <>
      <circle cx="7" cy="7" r="4.4" />
      <path d="m10.3 10.3 3.3 3.3" />
    </>
  ),
  filter: <path d="M2 4h12M4.67 8h6.66M6.67 12h2.66" />,
  chevronRight: <path d="M6 3.5 10.5 8 6 12.5" />,
  chevronDown: <path d="M3.5 6 8 10.5 12.5 6" />,
  chevronUp: <path d="M3.5 10 8 5.5l4.5 4.5" />,
  updown: <path d="M5 6.2 8 3.2l3 3M5 9.8l3 3 3-3" />,
  arrowRight: <path d="M3 8h10M9 4l4 4-4 4" />,
  // Leaves the view for another one: the board, a file.
  arrowUpRight: <path d="M4.67 11.33 11.33 4.67M5.33 4.67h6v6" />,
  expand: <path d="M9.6 2.6h3.8v3.8M13.4 2.6 9.2 6.8M6.4 13.4H2.6V9.6M2.6 13.4l4.2-4.2" />,
  collapse: <path d="M13.4 6.4H9.6V2.6M9.6 6.4l4-4M2.6 9.6h3.8v3.8M6.4 9.6l-4 4" />,
  restart: (
    <>
      <path d="M13 8.2A5 5 0 1 1 11.3 4.4" />
      <path d="M11.6 1.9v2.8H8.8" />
    </>
  ),
  // The same arrow turned back: undo what was set.
  reset: (
    <>
      <path d="M3 8.2A5 5 0 1 0 4.7 4.4" />
      <path d="M4.4 1.9v2.8h2.8" />
    </>
  ),
  alert: (
    <>
      <path d="M7.1 2.9a1 1 0 0 1 1.8 0l5.3 9.4a1 1 0 0 1-.9 1.5H2.7a1 1 0 0 1-.9-1.5z" />
      <path d="M8 6.4v3" />
      <circle cx="8" cy="11.5" r=".35" fill="currentColor" />
    </>
  ),
  bolt: <path d="M9.2 1.8 3.8 9h4l-1 5.2L12.2 7h-4z" />,
  // Plus, slash, minus: what changed, the way SF Symbols draws a diff.
  diff: <path d="M4.8 2.8v5M2.3 5.3h5M9 11.5h4.7M12.4 3.2 3.6 12.8" />,
} as const;

export type IconName = keyof typeof PATHS;

/**
 * `size` is the side in px at 100 % interface size: like the text around it,
 * the icon grows with the interface size. A stylesheet rule that sets the
 * width and height for a context wins over it.
 */
export function Icon({
  name,
  size = 14,
  className,
}: {
  name: IconName;
  size?: number;
  className?: string;
}) {
  return (
    <svg
      className={className ? `icon ${className}` : "icon"}
      style={{ "--icon": `${size}px` } as CSSProperties}
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
