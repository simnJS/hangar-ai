import { describe, expect, it } from "vitest";
import { missingFixes, presetIsEquipped, type LoadoutStatus, type ProviderUse } from "./loadout";

const use = (provider: string, state: ProviderUse["state"], how?: string): ProviderUse => ({
  provider,
  label: provider.toUpperCase(),
  state,
  detail: `${provider} detail`,
  how,
});

const status: LoadoutStatus = {
  catalog: "loadout",
  project: "C:/game",
  plugins: [
    {
      name: "media",
      unit: "open",
      equipped: true,
      access: true,
      capabilities: [
        {
          id: "models-3d",
          label: "3D",
          ready: false,
          uses: null,
          requires: [use("blender", "missing", "install Blender")],
          chain: [use("splice-mcp", "session"), use("node", "ok")],
        },
        {
          id: "rig",
          label: "Rig",
          ready: false,
          uses: null,
          requires: [use("blender", "missing", "install Blender"), use("python", "missing")],
          chain: [],
        },
      ],
    },
    {
      name: "minecraft",
      unit: "minecraft",
      equipped: false,
      access: false,
      capabilities: [
        { id: "x", label: "X", ready: false, uses: null, requires: [use("java", "missing")], chain: [] },
      ],
    },
  ],
};

describe("missingFixes", () => {
  it("lists each missing provider once, with its fix", () => {
    expect(missingFixes(status)).toEqual([
      { provider: "blender", label: "BLENDER", how: "install Blender" },
      // An engine without `how` still says what failed.
      { provider: "python", label: "PYTHON", how: "python detail" },
    ]);
  });

  it("leaves out plugins whose repository is out of reach", () => {
    expect(missingFixes(status).some((f) => f.provider === "java")).toBe(false);
  });
});

describe("presetIsEquipped", () => {
  it("is true only when every plugin of the preset is equipped", () => {
    expect(presetIsEquipped({ label: "", plugins: ["media"] }, status)).toBe(true);
    expect(presetIsEquipped({ label: "", plugins: ["media", "minecraft"] }, status)).toBe(false);
  });
});
