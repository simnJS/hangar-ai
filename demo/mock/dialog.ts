/**
 * Stands in for `@tauri-apps/plugin-dialog`. A browser cannot hand out a
 * folder path, so the picker returns a plausible one and the dialog carries on.
 */

const FOLDERS = [
  "C:\\dev\\hangar-ai",
  "C:\\dev\\storefront",
  "C:\\dev\\telemetry-api",
  "C:\\dev\\design-system",
];

let next = 1;

export async function open(options: { multiple?: boolean; directory?: boolean } = {}) {
  const picked = FOLDERS[next++ % FOLDERS.length];
  return options.multiple ? [picked] : picked;
}

export async function save() {
  return "C:\\dev\\hangar-ai\\workspace.code-workspace";
}

export async function message() {
  return undefined;
}

export async function confirm() {
  return true;
}

export async function ask() {
  return true;
}
