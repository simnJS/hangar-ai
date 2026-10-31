import { kitFileUrl } from "./kitTabs";
import type { KitInfo } from "./kits";

/**
 * Where a kit's own icon (`manifest.icon`, a svg or png of its folder) is
 * served by the `hangar-kit` scheme; null when the kit has none or cannot be
 * used, so the caller keeps its generic glyph. Always meant for an `<img>`:
 * an SVG loaded that way cannot run scripts, and is never put in the DOM.
 */
export function kitIconUrl(
  kit: Pick<KitInfo, "id" | "error" | "manifest">,
  platformIsWindows?: boolean,
): string | null {
  const manifest = kit.manifest;
  if (kit.error || !manifest || manifest.id !== kit.id) return null;
  const icon = manifest.icon?.trim();
  if (!icon) return null;
  return kitFileUrl(kit.id, icon, platformIsWindows);
}
