import { invoke } from "@tauri-apps/api/core";

/**
 * Loadout: the plugin catalog a team equips its Claude Code projects from.
 *
 * Everything here is computed by the catalog's own engine (`loadout.mjs`),
 * which the backend runs with `--json`; the shapes below are its output. The
 * window only draws them, so a catalog that changes its checks needs no
 * Hangar release.
 */

/** `session`: can only be checked from inside a Claude session (an MCP server). */
export type ProviderState = "ok" | "missing" | "session";

export interface ProviderUse {
  provider: string;
  label: string;
  state: ProviderState;
  detail: string;
  /** What to do to enable it, written for people. */
  how?: string;
  /** What using it costs, when the catalog says. */
  cost?: string;
  /** Paid: the skill asks before spending. */
  ask_before?: boolean;
}

export interface Capability {
  id: string;
  label: string;
  ready: boolean;
  /** The provider the capability would use right now, first of its chain. */
  uses: string | null;
  /** All of them are needed. */
  requires: ProviderUse[];
  /** The first available one is used, in order. */
  chain: ProviderUse[];
}

export interface LoadoutPlugin {
  name: string;
  unit: string;
  /** `null` when no project was asked about. */
  equipped: boolean | null;
  /** False when its repository is private and out of reach for this account. */
  access: boolean;
  capabilities: Capability[];
}

export interface LoadoutStatus {
  catalog: string;
  project: string | null;
  plugins: LoadoutPlugin[];
}

export interface Preset {
  label: string;
  plugins: string[];
}

export interface EquipReport {
  file: string;
  plugins: string[];
}

/** Path to the engine, or `null` when Loadout is not installed. */
export const loadoutLocate = () => invoke<string | null>("loadout_locate");

export const loadoutStatus = (cwd: string) => invoke<LoadoutStatus>("loadout_status", { cwd });

export const loadoutPresets = () => invoke<Record<string, Preset>>("loadout_presets");

export const loadoutEquip = (cwd: string, preset: string) =>
  invoke<EquipReport>("loadout_equip", { cwd, preset });

export interface Fix {
  provider: string;
  label: string;
  how: string;
}

/**
 * What is missing on this machine and how to enable it — once per provider,
 * however many capabilities cite it. Plugins out of reach are left out: no
 * install fixes a repository nobody gave access to.
 */
export function missingFixes(status: LoadoutStatus): Fix[] {
  const seen = new Map<string, Fix>();
  for (const plugin of status.plugins) {
    if (!plugin.access) continue;
    for (const capability of plugin.capabilities) {
      for (const use of [...capability.requires, ...capability.chain]) {
        if (use.state !== "missing" || seen.has(use.provider)) continue;
        seen.set(use.provider, {
          provider: use.provider,
          label: use.label,
          how: use.how ?? use.detail,
        });
      }
    }
  }
  return [...seen.values()];
}

/** Whether the project already has every plugin of the preset. */
export function presetIsEquipped(preset: Preset, status: LoadoutStatus): boolean {
  const equipped = new Set(status.plugins.filter((p) => p.equipped).map((p) => p.name));
  return preset.plugins.every((name) => equipped.has(name));
}
