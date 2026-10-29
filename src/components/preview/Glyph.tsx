import type { CSSProperties } from "react";

/**
 * The preview's own glyphs: a mark for each kind of file a tab can hold, and
 * the few tools the app's icon set (components/Icon) has no need for
 * elsewhere. Drawn as Icon draws its own — the same 16-unit grid, the same
 * 1.35-unit stroke, round caps and joins, the shared frame — so a tab of an
 * image sits beside a tab of a model, and a toolbar of both lines up.
 */
const PATHS = {
  image: (
    <>
      <rect x="1.75" y="2.75" width="12.5" height="10.5" rx="2.2" />
      <circle cx="5.6" cy="6.3" r="1.2" />
      <path d="M1.9 11.6 5.5 8.2l2.7 2.5 2.3-2.1 3.6 3.3" />
    </>
  ),
  video: (
    <>
      <rect x="1.75" y="2.75" width="12.5" height="10.5" rx="2.2" />
      <path d="M6.6 5.7v4.6L10.4 8z" />
    </>
  ),
  audio: (
    <>
      <path d="M6.1 12.1V3.7l6.6-1.5v8.3" />
      <circle cx="4.4" cy="12.1" r="1.75" />
      <circle cx="11" cy="10.5" r="1.75" />
    </>
  ),
  text: (
    <>
      <path d="M4.4 1.75h4.9l3.3 3.3v7.7a1.5 1.5 0 0 1-1.5 1.5H4.4a1.5 1.5 0 0 1-1.5-1.5V3.25a1.5 1.5 0 0 1 1.5-1.5z" />
      <path d="M9.3 1.75v3.3h3.3M5.5 8.3h5M5.5 11h3.4" />
    </>
  ),
  markdown: (
    <>
      <rect x="1.75" y="2.75" width="12.5" height="10.5" rx="2.2" />
      <path d="M4.3 10.4V5.6l1.9 2.3 1.9-2.3v4.8M11.4 5.6v4.6M9.9 8.8l1.5 1.5 1.5-1.5" />
    </>
  ),
  pdf: (
    <>
      <path d="M4.4 1.75h4.9l3.3 3.3v7.7a1.5 1.5 0 0 1-1.5 1.5H4.4a1.5 1.5 0 0 1-1.5-1.5V3.25a1.5 1.5 0 0 1 1.5-1.5z" />
      <path d="M9.3 1.75v3.3h3.3" />
      <rect x="1.6" y="8.2" width="8.4" height="3.6" rx="1" />
    </>
  ),
  html: (
    <>
      <circle cx="8" cy="8" r="6.1" />
      <path d="M1.9 8h12.2M8 1.9c1.6 1.7 2.5 3.8 2.5 6.1S9.6 12.4 8 14.1C6.4 12.4 5.5 10.3 5.5 8S6.4 3.6 8 1.9z" />
    </>
  ),
  code: <path d="M5.4 4.6 2 8l3.4 3.4M10.6 4.6 14 8l-3.4 3.4M9.1 3.1 6.9 12.9" />,
  wrap: <path d="M2.2 4h11.6M2.2 8h9a2.25 2.25 0 0 1 0 4.5H8.4M10 10.9l-1.6 1.6 1.6 1.6M2.2 12.5h3.4" />,
  loop: (
    <path d="M2.6 7.3v-.8A2.5 2.5 0 0 1 5.1 4h7.6M10.9 2.2 12.7 4l-1.8 1.8M13.4 8.7v.8a2.5 2.5 0 0 1-2.5 2.5H3.3M5.1 13.8 3.3 12l1.8-1.8" />
  ),
};

export type GlyphName = keyof typeof PATHS;

/** `size` as Icon takes it: the side in px at 100 % interface size. */
export function Glyph({ name, size = 14, className }: { name: GlyphName; size?: number; className?: string }) {
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
