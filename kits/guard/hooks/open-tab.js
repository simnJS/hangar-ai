"use strict";
// Started detached by guard.js after a refusal. Asks Hangar for the tab without
// taking the panel or the keyboard, and exits, silently, whatever happens: Hangar
// may be closed, the kit off this workspace, pane control turned off.
process.on("uncaughtException", () => process.exit(0));
process.on("unhandledRejection", () => process.exit(0));

const KIT = "guard";
const TAB = "guard";

/** Never throws. Resolves whether Hangar took the request. */
async function openTab({ url, token, pane, fetchImpl = globalThis.fetch }) {
  if (!url || !token || !pane || typeof fetchImpl !== "function") return false;
  try {
    const response = await fetchImpl(`${url}/api/kit/tab/open?pane=${encodeURIComponent(pane)}`, {
      method: "POST",
      headers: { authorization: `Bearer ${token}`, "content-type": "application/json" },
      body: JSON.stringify({ kit: KIT, tab: TAB, reveal: false }),
      signal: AbortSignal.timeout(2000),
    });
    return response.ok;
  } catch {
    return false;
  }
}

if (require.main === module) {
  const { HANGAR_API_URL: url, HANGAR_API_TOKEN: token, HANGAR_PANE_ID: pane } = process.env;
  openTab({ url, token, pane }).finally(() => process.exit(0));
}

module.exports = { openTab, KIT, TAB };
