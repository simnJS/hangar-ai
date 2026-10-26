import { describe, expect, it } from "vitest";
import {
  DOCK_MIN,
  MODEL_MIN,
  DRAWER_MARGIN,
  INSPECTOR_MAX,
  INSPECTOR_MIN,
  placeSidePanels,
  TERMINALS_MIN,
  type SidePanelsInput,
} from "./sidePanels";

/** The window less the navigator (252px at 100 %). */
const NAV = 252;

const input = (patch: Partial<SidePanelsInput> = {}): SidePanelsInput => ({
  room: 1600 - NAV,
  scale: 1,
  inspector: { open: true, width: 296 },
  dock: null,
  ...patch,
});

describe("placeSidePanels", () => {
  it("docks the inspector while the terminals keep their room", () => {
    const { inspector } = placeSidePanels(input());
    expect(inspector.fits).toBe(true);
    expect(inspector.width).toBe(296);
    expect(inspector.max).toBe(INSPECTOR_MAX);
  });

  it("turns the inspector into a drawer in half of a 1920 screen", () => {
    const { inspector } = placeSidePanels(input({ room: 960 - NAV }));
    expect(inspector.fits).toBe(false);
    // Same width as docked: switching modes never resizes what is inside.
    expect(inspector.width).toBe(296);
  });

  it("measures the switch in pixels, so a larger interface size switches sooner", () => {
    const room = 1280 - NAV * 1.3;
    expect(placeSidePanels(input({ room })).inspector.fits).toBe(true);
    expect(placeSidePanels(input({ room, scale: 1.3 })).inspector.fits).toBe(false);
  });

  it("switches exactly where the terminals would drop below their minimum", () => {
    const at = TERMINALS_MIN + 296;
    expect(placeSidePanels(input({ room: at })).inspector.fits).toBe(true);
    expect(placeSidePanels(input({ room: at - 1 })).inspector.fits).toBe(false);
  });

  it("never lets a drag take the terminals' room", () => {
    const { inspector } = placeSidePanels(input({ room: TERMINALS_MIN + 300 }));
    expect(inspector.max).toBe(300);
    // Nor below its own minimum, even when that is all there is.
    const narrow = input({
      room: TERMINALS_MIN + INSPECTOR_MIN,
      inspector: { open: true, width: INSPECTOR_MIN },
    });
    expect(placeSidePanels(narrow).inspector.max).toBe(INSPECTOR_MIN);
  });

  it("leaves a strip of work area uncovered by a drawer", () => {
    const { inspector } = placeSidePanels(input({ room: 300, inspector: { open: true, width: 440 } }));
    expect(inspector.width).toBe(300 - DRAWER_MARGIN);
  });

  it("clamps the dock at render, not only while it is dragged", () => {
    // 1000px window, no navigator, no inspector: a stored 420 would leave 580.
    const { dock } = placeSidePanels(
      input({ room: 1000, inspector: { open: false, width: 296 }, dock: { open: true, width: 420 } }),
    );
    expect(dock.fits).toBe(true);
    expect(dock.width).toBe(1000 - TERMINALS_MIN);
    expect(dock.max).toBe(1000 - TERMINALS_MIN);
  });

  it("draws the dock's width in points times the interface size", () => {
    const { dock } = placeSidePanels(
      input({
        room: 2400,
        scale: 1.25,
        inspector: { open: false, width: 296 },
        dock: { open: true, width: 400 },
      }),
    );
    expect(dock.width).toBe(500);
  });

  it("turns the dock into a drawer when even its minimum would crush the terminals", () => {
    // 1000px window with the navigator: four panes used to share 328px.
    const { dock, inspector } = placeSidePanels(
      input({ room: 1000 - NAV, dock: { open: true, width: 420 } }),
    );
    expect(dock.fits).toBe(false);
    expect(inspector.fits).toBe(false);
    expect(dock.width).toBe(420);
  });

  it("makes the inspector a drawer before it squeezes an open dock below its minimum", () => {
    const { inspector, dock } = placeSidePanels(
      input({ room: 1280 - NAV, dock: { open: true, width: 420 } }),
    );
    expect(inspector.fits).toBe(false);
    expect(dock.fits).toBe(true);
    expect(dock.width).toBe(1280 - NAV - TERMINALS_MIN);
  });

  it("docks both when there is room for the inspector and the dock's minimum", () => {
    const room = TERMINALS_MIN + 296 + DOCK_MIN;
    const { inspector, dock } = placeSidePanels(input({ room, dock: { open: true, width: 420 } }));
    expect(inspector.fits).toBe(true);
    expect(dock.fits).toBe(true);
    // The dock gives way first: down to its minimum, never past it.
    expect(dock.width).toBe(DOCK_MIN);
    expect(inspector.max).toBe(296);
  });

  it("ignores a dock that is closed, or off screen with the board or memory view", () => {
    const room = TERMINALS_MIN + 296;
    expect(
      placeSidePanels(input({ room, dock: { open: false, width: 420 } })).inspector.fits,
    ).toBe(true);
    expect(placeSidePanels(input({ room, dock: null })).inspector.fits).toBe(true);
  });

  it("ignores a dock that would be a drawer itself when placing the inspector", () => {
    // Too narrow for the dock alone, wide enough for a narrow inspector.
    const room = TERMINALS_MIN + 250;
    const { inspector, dock } = placeSidePanels(
      input({ room, inspector: { open: true, width: 250 }, dock: { open: true, width: 420 } }),
    );
    expect(dock.fits).toBe(false);
    expect(inspector.fits).toBe(true);
  });

  it("gives a closed inspector's room to the dock", () => {
    const room = 1280 - NAV;
    const open = placeSidePanels(input({ room, dock: { open: true, width: 600 } }));
    const closed = placeSidePanels(
      input({ room, inspector: { open: false, width: 296 }, dock: { open: true, width: 600 } }),
    );
    // Open but without room, it is a drawer, which takes nothing either.
    expect(open.dock.width).toBe(closed.dock.width);
  });
});

describe("placeSidePanels with the 3D preview", () => {
  const withModel = (patch: Partial<SidePanelsInput> = {}) =>
    input({ model: { open: true, width: 440 }, ...patch });

  it("places nothing differently while no model is open", () => {
    const room = 1600 - NAV;
    const dock = { open: true, width: 420 };
    const before = placeSidePanels(input({ room, dock }));
    const after = placeSidePanels(input({ room, dock, model: null }));
    expect(after.inspector).toEqual(before.inspector);
    expect(after.dock).toEqual(before.dock);
  });

  it("docks the preview at its width when the terminals keep their room", () => {
    const { model, inspector } = placeSidePanels(withModel({ room: 1920 - NAV }));
    expect(model.fits).toBe(true);
    expect(model.width).toBe(440);
    expect(inspector.fits).toBe(true);
  });

  it("makes the inspector a drawer before it squeezes the preview", () => {
    const { model, inspector } = placeSidePanels(withModel({ room: 1280 - NAV }));
    expect(inspector.fits).toBe(false);
    expect(model.fits).toBe(true);
    expect(model.width).toBe(Math.min(440, 1280 - NAV - TERMINALS_MIN));
  });

  it("keeps the preview beside the terminals and makes the dock the drawer when only one fits", () => {
    const { model, dock } = placeSidePanels(
      withModel({
        room: TERMINALS_MIN + MODEL_MIN + 100,
        inspector: { open: false, width: 296 },
        dock: { open: true, width: 420 },
      }),
    );
    expect(model.fits).toBe(true);
    expect(dock.fits).toBe(false);
  });

  it("leaves the dock its minimum beside the preview while there is room for both", () => {
    const room = TERMINALS_MIN + MODEL_MIN + DOCK_MIN + 60;
    const { model, dock } = placeSidePanels(
      withModel({ room, inspector: { open: false, width: 296 }, dock: { open: true, width: 420 } }),
    );
    expect(model.fits).toBe(true);
    expect(dock.fits).toBe(true);
    expect(model.width + dock.width).toBe(room - TERMINALS_MIN);
    expect(dock.width).toBe(DOCK_MIN);
  });

  it("becomes a drawer in half of a 1920 screen, the same width as docked", () => {
    const { model } = placeSidePanels(withModel({ room: 960 - NAV }));
    expect(model.fits).toBe(false);
    expect(model.width).toBe(440);
  });

  it("never lets a drag take the terminals' room", () => {
    const { model } = placeSidePanels(
      withModel({ room: TERMINALS_MIN + 500, inspector: { open: false, width: 296 } }),
    );
    expect(model.max).toBe(500);
  });
});
