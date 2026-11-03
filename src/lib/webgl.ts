import type { Terminal } from "@xterm/xterm";
import { WebglAddon } from "@xterm/addon-webgl";

/**
 * GPU rendering for a terminal, with xterm's DOM renderer as the fallback.
 *
 * Browsers cap the number of live WebGL contexts — 16 in Chromium, WebView2
 * included — and silently drop the oldest past it. The panes of every opened
 * workspace stay mounted, so a pane only holds a context while it is on screen,
 * and the caller gives it back once the pane has been away for a while.
 */

/** How long an off-screen pane keeps its context before handing it back. */
export const WEBGL_PARK_MS = 10_000;

/** Set once WebGL2 cannot be had at all: no point asking every pane again. */
let unavailable = false;

/**
 * Linux runs the webview on WebKitGTK, where WebGL goes through a compositor
 * path that is slow and sometimes broken on NVIDIA drivers: every keystroke
 * redraws late, so typing and deleting lag. xterm's DOM renderer is the better
 * trade there. `localStorage.setItem("hangar.webgl", "1")` brings WebGL back
 * on a machine where it is fast; "0" turns it off anywhere.
 */
export function webglWanted(): boolean {
  try {
    const choice = localStorage.getItem("hangar.webgl");
    if (choice === "1") return true;
    if (choice === "0") return false;
  } catch {
    /* storage blocked: fall through to the platform default */
  }
  const ua = typeof navigator === "undefined" ? "" : navigator.userAgent;
  return !(/linux/i.test(ua) && !/android/i.test(ua));
}

export function attachWebgl(term: Terminal, onLost: () => void): WebglAddon | null {
  if (unavailable || !webglWanted()) return null;
  const addon = new WebglAddon();
  try {
    term.loadAddon(addon);
  } catch {
    // No WebGL2: the addon throws while activating, and xterm keeps drawing
    // with the DOM renderer it already had.
    unavailable = true;
    try {
      addon.dispose();
    } catch {
      /* half-activated; nothing left worth cleaning */
    }
    return null;
  }
  // A lost context — driver reset, GPU process restart, one context too many —
  // leaves a blank canvas behind. Disposing the addon puts the DOM renderer
  // back; the pane asks for WebGL again the next time it comes on screen.
  addon.onContextLoss(() => {
    addon.dispose();
    onLost();
  });
  return addon;
}
