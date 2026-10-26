import { useT } from "../i18n";
import { usePaneActivity } from "../lib/agentState";
import { useShortcutTitle } from "../lib/useShortcuts";
import { StateGlyph } from "./StateGlyph";

/**
 * The strip over a zoomed pane. Without it a zoomed grid looks like a
 * workspace that lost its other panes: it says which pane fills the grid, that
 * the rest are still running, where each of them stands — and puts each one a
 * click away.
 *
 * Mouse presses are kept off the buttons' focus: the keyboard stays with the
 * terminal, which is where it is wanted next. Reached with Tab instead, the
 * strip lets Escape put every pane back, as its button does.
 */
export function PaneZoomBar({
  zoomedId,
  order,
  names,
  onZoom,
}: {
  zoomedId: string;
  /** Every pane, in reading order. */
  order: string[];
  names: Map<string, string>;
  /** Zooms onto another pane, or with null puts every pane back on screen. */
  onZoom: (paneId: string | null) => void;
}) {
  const t = useT();
  const withKeys = useShortcutTitle();
  const others = order.filter((id) => id !== zoomedId);

  return (
    <div
      className="zoombar"
      role="toolbar"
      aria-label={t("zoom.bar")}
      onMouseDown={(event) => event.preventDefault()}
      onKeyDown={(event) => {
        if (event.key !== "Escape") return;
        event.preventDefault();
        onZoom(null);
      }}
    >
      <span className="zoombar__tag">{t("zoom.tag")}</span>
      <span className="zoombar__name">{names.get(zoomedId)}</span>
      <span className="zoombar__others">
        <span className="zoombar__dot" aria-hidden="true">
          ·
        </span>
        {t("zoom.others", { n: others.length })}
      </span>
      <span className="zoombar__panes">
        {others.map((id) => (
          <ZoomChip
            key={id}
            paneId={id}
            name={names.get(id) ?? ""}
            onClick={() => onZoom(id)}
          />
        ))}
      </span>
      <span className="zoombar__spacer" />
      <button
        className="zoombar__restore"
        onClick={() => onZoom(null)}
        title={withKeys(t("zoom.restore"), "pane.zoom")}
      >
        {t("zoom.restore")}
      </button>
    </div>
  );
}

/** One pane out of sight, with its state: the reason to go and look at it. */
function ZoomChip({ paneId, name, onClick }: { paneId: string; name: string; onClick: () => void }) {
  const t = useT();
  const activity = usePaneActivity(paneId)?.activity ?? "idle";
  return (
    <button
      className="zoombar__pane"
      onClick={onClick}
      title={`${t("zoom.show", { name })} · ${t(`pane.state.${activity}` as const)}`}
    >
      <StateGlyph state={activity} size={7} />
      {name}
    </button>
  );
}
