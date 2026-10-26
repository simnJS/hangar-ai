/**
 * The glyphs of the settings window and the dialogs around it: one per
 * category in the sidebar, plus the few controls that used to be drawn with
 * text characters (⌕, ↺, ✓), which render at a different weight on every
 * platform and never line up with the text beside them.
 *
 * Same grid, stroke and `currentColor` as components/Icon, so the two sets
 * read as one. Decoration only: the control around a glyph carries the label.
 */
const PATHS = {
  general: (
    <>
      <path d="M21 4h-7M10 4H3M21 12h-9M8 12H3M21 20h-5M12 20H3" />
      <path d="M14 2v4M8 10v4M16 18v4" />
    </>
  ),
  appearance: (
    <>
      <circle cx="12" cy="12" r="9" />
      <path d="M12 3a9 9 0 0 1 0 18z" fill="currentColor" stroke="none" />
    </>
  ),
  terminal: (
    <>
      <rect x="3" y="4" width="18" height="16" rx="2.5" />
      <path d="m7.5 10 2.5 2-2.5 2M12.5 15h4" />
    </>
  ),
  shell: <path d="m5 17 5-5-5-5M12 19h7" />,
  notifications: (
    <>
      <path d="M10.27 21a2 2 0 0 0 3.46 0" />
      <path d="M3.26 15.33A1 1 0 0 0 4 17h16a1 1 0 0 0 .74-1.67C19.41 13.96 18 12.5 18 8A6 6 0 0 0 6 8c0 4.5-1.41 5.96-2.74 7.33" />
    </>
  ),
  voice: (
    <>
      <rect x="9" y="2.5" width="6" height="12" rx="3" />
      <path d="M19 10.5v1a7 7 0 0 1-14 0v-1M12 18.5V22" />
    </>
  ),
  discord: (
    <>
      <path d="M6 11h4M8 9v4M15 12h.01M18 10h.01" />
      <path d="M17.32 5H6.68a4 4 0 0 0-3.98 3.59C2.6 9.42 2 14.46 2 16a3 3 0 0 0 3 3c1 0 1.5-.5 2-1l1.41-1.41A2 2 0 0 1 9.83 16h4.34a2 2 0 0 1 1.41.59L17 18c.5.5 1 1 2 1a3 3 0 0 0 3-3c0-1.55-.6-6.58-.69-7.26A4 4 0 0 0 17.32 5z" />
    </>
  ),
  shortcuts: (
    <>
      <rect x="2" y="5" width="20" height="14" rx="2.5" />
      <path d="M6 9h.01M10 9h.01M14 9h.01M18 9h.01M8 13h.01M12 13h.01M16 13h.01M7.5 16h9" />
    </>
  ),
  about: (
    <>
      <circle cx="12" cy="12" r="9.5" />
      <path d="M12 16.5V11M12 7.5h.01" />
    </>
  ),
  search: (
    <>
      <circle cx="11" cy="11" r="7" />
      <path d="m20 20-4-4" />
    </>
  ),
  check: <path d="M20 6 9 17l-5-5" />,
  plus: <path d="M5 12h14M12 5v14" />,
  minus: <path d="M5 12h14" />,
  reset: (
    <>
      <path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8" />
      <path d="M3 3v5h5" />
    </>
  ),
  close: <path d="M18 6 6 18M6 6l12 12" />,
  download: <path d="M12 4v11M7 10.5l5 5 5-5M5 20h14" />,
  alert: (
    <>
      <path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3" />
      <path d="M12 9v4M12 17h.01" />
    </>
  ),
} as const;

export type GlyphName = keyof typeof PATHS;

/** Sized in CSS (`.glyph-icon`), so it follows the interface size. */
export function Glyph({ name, className = "" }: { name: GlyphName; className?: string }) {
  return (
    <svg
      className={`glyph-icon ${className}`}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.75}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
    >
      {PATHS[name]}
    </svg>
  );
}
