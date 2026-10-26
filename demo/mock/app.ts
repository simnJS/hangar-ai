import { version } from "../../package.json";

/** Stands in for `@tauri-apps/api/app`, with the version being built. */
export const getVersion = async () => version;
export const getName = async () => "Hangar.AI";
export const getTauriVersion = async () => "2.0.0";
