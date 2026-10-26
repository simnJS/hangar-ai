/**
 * Where the panels on the right of the terminals go: the 3D preview, the
 * board panel (the dock) and the inspector.
 *
 * Each one sits beside the terminals while that leaves them at least
 * `TERMINALS_MIN` pixels. Without that room it stops taking any: it becomes a
 * drawer over the right of the work area, shown only while asked for, so a
 * narrow window — half of a 1920 screen, or a larger interface size — keeps
 * its terminals whatever the panels' saved state says.
 *
 * Widths come in interface points and are drawn at `points × scale`, so a
 * larger interface size keeps the same amount of text per line. The room and
 * every width this returns are in pixels, except the `max`es, which are in
 * points, like the setting a drag writes.
 */

/**
 * What the terminals keep beside docked panels, in pixels: the terminals
 * themselves do not follow the interface size. About a 1190px window with the
 * navigator and the inspector open.
 */
export const TERMINALS_MIN = 640;

export const INSPECTOR_MIN = 240;
export const INSPECTOR_MAX = 440;
export const DOCK_MIN = 300;
/** A 3D view narrower than this shows a model as a sliver. */
export const MODEL_MIN = 320;

/** The strip of work area a drawer, however wide, always leaves uncovered. */
export const DRAWER_MARGIN = 48;

export interface SidePanelsInput {
  /** The row the terminals share with both panels, in pixels. */
  room: number;
  /** The interface size, 1 for 100 %. */
  scale: number;
  inspector: { open: boolean; width: number };
  /** Null when the terminals are not on screen, which is the only place it goes. */
  dock: { open: boolean; width: number } | null;
  /**
   * The 3D preview, beside the terminals like the dock. Null while no model
   * is open, off screen with the terminals, or expanded over the whole work
   * area, where it takes no room from anyone.
   */
  model?: { open: boolean; width: number } | null;
}

export interface PanelPlace {
  /** Beside the terminals; otherwise a drawer over them, shown on demand. */
  fits: boolean;
  /** Drawn width, in pixels. */
  width: number;
  /** The widest a drag may make it right now, in points. */
  max: number;
}

export interface SidePanels {
  inspector: PanelPlace;
  dock: PanelPlace;
  model: PanelPlace;
}

export const clampInspector = (width: number) =>
  Math.round(Math.min(INSPECTOR_MAX, Math.max(INSPECTOR_MIN, width)));

export const clampDock = (width: number) => Math.round(Math.max(DOCK_MIN, width));

export const clampModelPanel = (width: number) => Math.round(Math.max(MODEL_MIN, width));

/**
 * When both are open and only one fits, the inspector is the one that turns
 * into a drawer: the board panel is what the user asked to keep beside the
 * panes, while the inspector details one pane that its header and the toolbar
 * already sum up. It reserves the dock's minimum width, not its saved one, so
 * a wide dock narrows before the inspector gives way. The dock then takes
 * what is left, down to its minimum, and the terminals keep the rest.
 *
 * The 3D preview goes before both: it is what an agent has just put in front
 * of the user, and it only exists while a model is open. The dock gives way
 * to it the way the inspector gives way to the dock — the preview keeps the
 * dock's minimum free beside it while there is room for that, and takes the
 * rest of its width first.
 */
export function placeSidePanels({
  room,
  scale,
  inspector,
  dock,
  model = null,
}: SidePanelsInput): SidePanels {
  const inspectorPx = clampInspector(inspector.width) * scale;
  const dockMinPx = DOCK_MIN * scale;
  const dockWanted = dock ? clampDock(dock.width) * scale : dockMinPx;
  const modelMinPx = MODEL_MIN * scale;
  const modelWanted = model ? clampModelPanel(model.width) * scale : modelMinPx;

  const modelCounts = Boolean(model?.open) && room - modelMinPx >= TERMINALS_MIN;
  const besideModel = room - (modelCounts ? modelMinPx : 0);
  const dockCounts = Boolean(dock?.open) && besideModel - dockMinPx >= TERMINALS_MIN;
  const reserved = (modelCounts ? modelMinPx : 0) + (dockCounts ? dockMinPx : 0);
  const inspectorFits = room - inspectorPx - reserved >= TERMINALS_MIN;

  const inspectorPlace: PanelPlace = inspectorFits
    ? {
        fits: true,
        width: inspectorPx,
        max: clampInspector((room - reserved - TERMINALS_MIN) / scale),
      }
    : {
        fits: false,
        width: Math.max(0, Math.min(inspectorPx, room - DRAWER_MARGIN)),
        max: clampInspector((room - DRAWER_MARGIN) / scale),
      };

  // What the preview and the dock share with the terminals: the row, less a
  // docked inspector.
  const workarea = room - (inspector.open && inspectorFits ? inspectorPx : 0);

  const modelFits = workarea - modelMinPx >= TERMINALS_MIN;
  const dockKept =
    dock?.open && workarea - modelMinPx - dockMinPx >= TERMINALS_MIN ? dockMinPx : 0;
  const modelRoom = workarea - (modelFits ? TERMINALS_MIN + dockKept : DRAWER_MARGIN);
  const modelPlace: PanelPlace = {
    fits: modelFits,
    width: modelFits
      ? Math.max(modelMinPx, Math.min(modelWanted, modelRoom))
      : Math.max(0, Math.min(modelWanted, modelRoom)),
    max: clampModelPanel(modelRoom / scale),
  };

  const dockArea = workarea - (model?.open && modelFits ? modelPlace.width : 0);
  const dockFits = dockArea - dockMinPx >= TERMINALS_MIN;
  const dockRoom = dockArea - (dockFits ? TERMINALS_MIN : DRAWER_MARGIN);

  const dockPlace: PanelPlace = {
    fits: dockFits,
    width: dockFits
      ? Math.max(dockMinPx, Math.min(dockWanted, dockRoom))
      : Math.max(0, Math.min(dockWanted, dockRoom)),
    max: clampDock(dockRoom / scale),
  };

  return { inspector: inspectorPlace, dock: dockPlace, model: modelPlace };
}
