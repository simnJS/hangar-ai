/**
 * The app mark: a workspace cut into panes, one of them active — the same
 * shape the interface makes. Drawn as the bare glyph, so it inherits the
 * theme's foreground and accent like everything else; the tile-backed version
 * used for the OS icon is drawn by `scripts/logo.ps1`.
 *
 * Decorative wherever it ships: the name is always written beside it.
 */
export function Logo({ size = 16 }: { size?: number }) {
  return (
    <svg
      className="logo"
      width={size}
      height={size}
      viewBox="0 0 32 32"
      aria-hidden="true"
      focusable="false"
    >
      <rect className="logo__active" x="4" y="4" width="10" height="24" />
      <rect x="17" y="4" width="11" height="10" />
      <rect x="17" y="17" width="11" height="11" />
    </svg>
  );
}
