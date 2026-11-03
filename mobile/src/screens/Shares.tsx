import { useCallback, useEffect, useState } from "react";
import type { Share } from "../api";
import { EmptyState, PcChip } from "../components/Chrome";
import { Icon } from "../components/Icon";
import { ShareView } from "../components/ShareView";
import { Zoom } from "../components/Zoom";
import { t } from "../i18n";
import { useRemoteContext } from "../remote";

/** What agents shared with the phone, newest first, each in the viewer of its kind. */
export function Shares({ now, onSeen }: { now: number; onSeen: (at: number) => void }) {
  const { shares, client, pc, connection } = useRemoteContext();
  const newest = shares[0]?.at;
  const zoom = useZoomStep();
  const zoomed = zoom.id === null ? undefined : shares.find((share) => share.id === zoom.id);

  // Seen as soon as they are on screen.
  useEffect(() => {
    if (newest !== undefined) onSeen(newest);
  }, [newest, onSeen]);

  return (
    <div className="page">
      <div className="top">
        <h1 className="title">{t("shares.title")}</h1>
        <PcChip pc={pc} connection={connection} />
      </div>
      {shares.length === 0 ? (
        <EmptyState title={t("shares.empty")} text={t("shares.emptyHint")} mark={<Icon name="image" size={40} />} />
      ) : (
        <ul className="stack section">
          {shares.map((share) => (
            <li key={share.id} className="share">
              <ShareView share={share} now={now} onZoom={zoom.open} />
            </li>
          ))}
        </ul>
      )}
      {zoomed && <Zoom src={client.fileUrl(zoomed.id)} alt={zoomed.caption ?? zoomed.name} onClose={zoom.close} />}
    </div>
  );
}

const zoomOf = (state: unknown) => {
  const zoom = (state as { zoom?: unknown } | null)?.zoom;
  return typeof zoom === "string" ? zoom : null;
};

/**
 * A picture zoomed into, as a step of its own in the phone's history: the
 * back gesture, or the close button, returns to the list.
 */
function useZoomStep() {
  const [id, setId] = useState<string | null>(() => zoomOf(history.state));
  useEffect(() => {
    const update = () => setId(zoomOf(history.state));
    addEventListener("popstate", update);
    return () => removeEventListener("popstate", update);
  }, []);
  const open = useCallback((share: Share) => {
    history.pushState({ ...history.state, depth: (history.state?.depth ?? 0) + 1, zoom: share.id }, "");
    setId(share.id);
  }, []);
  const close = useCallback(() => {
    if (zoomOf(history.state) !== null) history.back();
    else setId(null);
  }, []);
  return { id, open, close };
}
