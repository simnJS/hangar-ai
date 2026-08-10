/**
 * Entry point for the browser demo.
 *
 * Identical to src/main.tsx: same App, same store, same stylesheet. The only
 * difference is in vite.demo.config.ts, which points every `@tauri-apps/*`
 * import at demo/mock instead. Nothing under src/ knows this build exists.
 */
import { useEffect } from "react";
import ReactDOM from "react-dom/client";
import App from "../src/App";
import { StoreProvider, useStore } from "../src/store";
import { THEMES } from "../src/themes";
import { ptyWrite } from "../src/lib/ipc";
import "../src/styles.css";
import "./demo.css";

/** The pane the seeded workspace starts Claude Code in. See backend/state. */
const FIRST_PANE = "pane-1";

/**
 * Talks to the page holding the iframe.
 *
 * Out: one `ready` message, so the site can drop its loading state when the
 * app has actually mounted rather than when the document merely loaded.
 *
 * In: a theme to switch to, and a line to type into the first pane, from the
 * prompt chips under the frame. Deliberately narrow, and both are checked
 * against what the app already has rather than trusted.
 */
function PageBridge() {
  const { updateSettings } = useStore();

  useEffect(() => {
    const announce = () => window.parent.postMessage({ type: "hangar-demo:ready" }, "*");
    announce();
    // Repeated once the terminals have had a frame to paint, in case the page
    // was not listening yet when the first one went out.
    const again = window.setTimeout(announce, 600);

    const onMessage = (event: MessageEvent) => {
      const data = event.data as { type?: string; id?: string; text?: string } | null;
      if (!data) return;

      if (data.type === "hangar-demo:theme") {
        if (!THEMES.some((theme) => theme.id === data.id)) return;
        updateSettings({ themeId: data.id });
        return;
      }

      if (data.type === "hangar-demo:type" && typeof data.text === "string") {
        // Straight into the pseudo-terminal, exactly as a keystroke would
        // arrive. The agent in that pane sees a line someone typed.
        ptyWrite(FIRST_PANE, `${data.text}\r`).catch(() => undefined);
      }
    };

    window.addEventListener("message", onMessage);
    return () => {
      window.clearTimeout(again);
      window.removeEventListener("message", onMessage);
    };
  }, [updateSettings]);

  return null;
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <StoreProvider>
    <PageBridge />
    <App />
  </StoreProvider>,
);
