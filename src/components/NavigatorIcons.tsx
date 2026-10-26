/**
 * The navigator's glyphs, drawn on a 16-unit grid with a hairline stroke so
 * they sit at the weight of 12px text, the way a pro app's source list does.
 * The app-wide set (Icon.tsx) is drawn for 24 units and buttons; at this size
 * its stroke would read twice as heavy as the text beside it.
 *
 * Like Icon, a glyph is decoration: whatever carries it says what it means.
 */
const PATHS = {
  chevron: <path d="M6 3.5 10.5 8 6 12.5" />,
  workspace: (
    <>
      <rect x="2" y="2.75" width="12" height="10.5" rx="2" />
      <path d="M7.2 2.75v10.5M7.2 8h6.8" />
    </>
  ),
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
  sidebar: (
    <>
      <rect x="1.75" y="2.75" width="12.5" height="10.5" rx="2.2" />
      <path d="M6 2.75v10.5M3.6 5.2h.9M3.6 7.2h.9" />
    </>
  ),
  plus: <path d="M8 3.25v9.5M3.25 8h9.5" />,
  search: (
    <>
      <circle cx="7" cy="7" r="4.4" />
      <path d="m10.3 10.3 3.3 3.3" />
    </>
  ),
  attention: (
    <>
      <path d="M7.1 2.9a1 1 0 0 1 1.8 0l5.3 9.4a1 1 0 0 1-.9 1.5H2.7a1 1 0 0 1-.9-1.5z" />
      <path d="M8 6.4v3" />
      <circle cx="8" cy="11.5" r=".35" fill="currentColor" />
    </>
  ),
  gear: (
    <>
      <path d="M6.9 1.9h2.2l.35 1.75a4.7 4.7 0 0 1 1.2.7l1.7-.57 1.1 1.9-1.35 1.18a4.7 4.7 0 0 1 0 1.39l1.35 1.18-1.1 1.9-1.7-.57a4.7 4.7 0 0 1-1.2.7L9.1 14.1H6.9l-.35-1.75a4.7 4.7 0 0 1-1.2-.7l-1.7.57-1.1-1.9 1.35-1.18a4.7 4.7 0 0 1 0-1.39L2.55 6.57l1.1-1.9 1.7.57a4.7 4.7 0 0 1 1.2-.7z" />
      <circle cx="8" cy="8" r="2" />
    </>
  ),
  more: (
    <>
      <circle cx="3.6" cy="8" r=".9" fill="currentColor" stroke="none" />
      <circle cx="8" cy="8" r=".9" fill="currentColor" stroke="none" />
      <circle cx="12.4" cy="8" r=".9" fill="currentColor" stroke="none" />
    </>
  ),
  window: (
    <>
      <rect x="1.75" y="5" width="9.5" height="8.25" rx="1.6" />
      <path d="M4.75 5V4.35a1.6 1.6 0 0 1 1.6-1.6h6.3a1.6 1.6 0 0 1 1.6 1.6v5.3a1.6 1.6 0 0 1-1.6 1.6h-1.4" />
    </>
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
  trash: (
    <path d="M2.75 4.25h10.5M6.25 4.25V2.75h3.5v1.5M4.25 4.25l.6 8.3a1 1 0 0 0 1 .95h4.3a1 1 0 0 0 1-.95l.6-8.3" />
  ),
  check: <path d="m3.5 8.4 2.9 2.9 6.1-6.6" />,
  close: <path d="m4.5 4.5 7 7m0-7-7 7" />,
} as const;

export type NavigatorIconName = keyof typeof PATHS;

export function NavigatorIcon({ name, size = 16 }: { name: NavigatorIconName; size?: number }) {
  return (
    <svg
      className={`nav-icon nav-icon--${name}`}
      // Sized by the stylesheet against the interface scale; this is the
      // size at 100 %.
      style={{ "--nav-icon": `${size}px` } as React.CSSProperties}
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
