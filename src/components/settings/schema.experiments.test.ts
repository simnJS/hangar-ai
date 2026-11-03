import { describe, expect, it, vi } from "vitest";
import { DEFAULT_SETTINGS, type Settings } from "../../types";
import { translate } from "../../i18n";
import { buildCategories, type SettingItem } from "./schema";
import type { Experiment } from "../../lib/experiments";

vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

const demo: Experiment = {
  id: "demo-feature",
  stage: "beta",
  title: "experiments.title",
  hint: "experiments.warning",
  platforms: ["linux", "macos"],
};

function categories(settings: Settings, experiments?: readonly Experiment[]) {
  const set = vi.fn();
  const built = buildCategories({
    t: (key, params) => translate("en", key, params),
    settings,
    shells: [],
    workspace: null,
    version: "",
    experiments,
    set,
    patchWorkspace: vi.fn(),
    resetSettings: vi.fn(),
  });
  return { built, set };
}

describe("settings › Experimental", () => {
  it("is absent while no experiment is registered", () => {
    const { built } = categories(DEFAULT_SETTINGS, []);
    expect(built.map((category) => category.id)).not.toContain("experimental");
  });

  it("sits before About once something is registered", () => {
    const { built } = categories(DEFAULT_SETTINGS, [{ ...demo, platforms: undefined }]);
    const ids = built.map((category) => category.id);
    expect(ids).toContain("experimental");
    expect(ids.indexOf("experimental")).toBe(ids.indexOf("about") - 1);
  });

  it("turns an experiment on and off through the settings", () => {
    const open = { ...demo, platforms: undefined };
    const { built, set } = categories(DEFAULT_SETTINGS, [open]);
    const item = built
      .find((category) => category.id === "experimental")!
      .sections[0].items.find((entry) => entry.id === "experiment.demo-feature") as Extract<
      SettingItem,
      { kind: "toggle" }
    >;
    expect(item.kind).toBe("toggle");
    expect(item.value).toBe(false);

    item.onChange(true);
    expect(set).toHaveBeenLastCalledWith("experiments", ["demo-feature"]);

    const on = categories({ ...DEFAULT_SETTINGS, experiments: ["demo-feature", "other"] }, [open]);
    const row = on.built
      .find((category) => category.id === "experimental")!
      .sections[0].items[0] as Extract<SettingItem, { kind: "toggle" }>;
    expect(row.value).toBe(true);
    row.onChange(false);
    expect(on.set).toHaveBeenLastCalledWith("experiments", ["other"]);
  });

  it("says what stage a feature is at", () => {
    // No platform list: `availableExperiments` reads the real platform, which
    // the node test environment does not have.
    const { built } = categories(DEFAULT_SETTINGS, [
      { ...demo, platforms: undefined, stage: "beta" },
    ]);
    const item = built.find((category) => category.id === "experimental")!.sections[0]
      .items[0] as Extract<SettingItem, { kind: "toggle" }>;
    expect(item.hint).toContain("Beta");
  });
});
