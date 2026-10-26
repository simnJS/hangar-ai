import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";

/**
 * Where the 3D preview's capture button puts its picture: on the clipboard,
 * or in a PNG file the user picks with the system's save dialog.
 */

const bytesOf = (base64: string) => Uint8Array.from(atob(base64), (c) => c.charCodeAt(0));

export async function copyPng(base64: string): Promise<void> {
  const blob = new Blob([bytesOf(base64)], { type: "image/png" });
  await navigator.clipboard.write([new ClipboardItem({ "image/png": blob })]);
}

/** `chair-20261026-204512.png` for `chair.glb`: the name the dialog proposes. */
export function captureName(title: string, now = new Date()): string {
  const stem = title.replace(/\.[^.]+$/, "").replace(/[\\/:*?"<>|]+/g, "-") || "model";
  const pad = (n: number) => String(n).padStart(2, "0");
  const date = `${now.getFullYear()}${pad(now.getMonth() + 1)}${pad(now.getDate())}`;
  const time = `${pad(now.getHours())}${pad(now.getMinutes())}${pad(now.getSeconds())}`;
  const stamp = `${date}-${time}`;
  return `${stem}-${stamp}.png`;
}

/**
 * Asks where to save, then has Rust write the file there. Resolves false
 * when the user cancels the dialog.
 */
export async function savePng(base64: string, suggestedName: string): Promise<boolean> {
  const path = await save({
    defaultPath: suggestedName,
    filters: [{ name: "PNG", extensions: ["png"] }],
  });
  if (!path) return false;
  await invoke<void>("model_save_png", { path, data: base64 });
  return true;
}
