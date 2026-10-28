import { invoke } from "@tauri-apps/api/core";

/**
 * The two options that keep Hangar reachable while the person is away, beside
 * the Mobile switch: the PC kept awake, and Hangar started at login. Mirrors
 * `Options` (src-tauri/src/remote/mod.rs).
 */
export interface MobileOptions {
  /** The PC does not sleep while Hangar Mobile is on (the display still may). */
  keepAwake: boolean;
  /** False where it cannot work (Linux). */
  keepAwakeSupported: boolean;
  /** Hangar starts when the person logs in. */
  autostart: boolean;
}

export const mobileOptions = () => invoke<MobileOptions>("remote_options");
export const setKeepAwake = (enabled: boolean) => invoke<void>("remote_set_keep_awake", { enabled });
export const setAutostart = (enabled: boolean) => invoke<void>("remote_set_autostart", { enabled });
