import { useEffect, useState } from "react";
import { errorText } from "./parts";
import { reloadModel, type OpenFile } from "../../lib/modelStore";
import { checkPreviewFile, previewInspect } from "../../lib/preview";

/** How often the file on screen is looked at for a newer version. */
const WATCH_MS = 1500;
/** A changed file is read again this long after, to not load it half-written. */
const SETTLE_MS = 300;

/**
 * Watches the file in front while the panel is on screen, as the 3D view
 * watches its model (lib/model3d/useModelViewer › useModelWatch): a newer
 * modification time, steady for a moment, has the viewer read it again. A
 * file that disappears or is now refused keeps its last version on screen,
 * and the reason is returned to be said.
 */
export function useFileWatch(entry: OpenFile, watching: boolean): string | null {
  const [missing, setMissing] = useState<string | null>(null);
  const { id, path, modifiedMs } = entry;

  useEffect(() => setMissing(null), [id]);

  useEffect(() => {
    if (!watching) return;
    let cancelled = false;
    let busy = false;
    const look = async () => checkPreviewFile(await previewInspect(path));
    const tick = async () => {
      if (busy || document.hidden) return;
      busy = true;
      try {
        const now = await look();
        if (cancelled) return;
        setMissing(null);
        if (now.modifiedMs <= modifiedMs) return;
        // Once more a moment later: an agent writing the file in several
        // goes would otherwise have its first half read.
        await new Promise((resolve) => window.setTimeout(resolve, SETTLE_MS));
        const again = await look();
        if (!cancelled && again.modifiedMs === now.modifiedMs && again.size === now.size) {
          reloadModel(id, again.modifiedMs, again.size);
        }
      } catch (error) {
        if (!cancelled) setMissing(errorText(error));
      } finally {
        busy = false;
      }
    };
    const timer = window.setInterval(tick, WATCH_MS);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [watching, id, path, modifiedMs]);

  return missing;
}
