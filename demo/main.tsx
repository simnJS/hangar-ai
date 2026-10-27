/**
 * Entry point for the browser demo.
 *
 * Identical to src/main.tsx: same App, same store, same stylesheets. The only
 * difference is in vite.demo.config.ts, which points every `@tauri-apps/*`
 * import at demo/mock instead. Nothing under src/ knows this build exists.
 */
import { useEffect } from "react";
import ReactDOM from "react-dom/client";
import App from "../src/App";
import { StoreProvider, useStore } from "../src/store";
import { THEMES } from "../src/themes";
import { ptyWrite } from "../src/lib/ipc";
import { playOpeningScene } from "./backend/scene";
import "../src/stylesheet";
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

/**
 * The one place the demo says what it is, when it stands on its own page. In
 * the site's frame, the bar above the frame says it instead, so the app keeps
 * the whole frame. Nothing in the panes repeats it.
 */
function DemoNotice() {
  if (window.self !== window.top) return null;
  return (
    <p className="demo-notice">
      <span>
        Browser demo of Hangar.AI: the window is the real app, the agents are scripted. No model
        runs, and nothing is saved.
      </span>
      <a href="https://hangar.simnjs.fr/#download">Get the app</a>
    </p>
  );
}

/**
 * Starts what the agents off screen are busy with (backend/scene) once the
 * app has its state, a moment after the workspace on screen has started its
 * own panes.
 */
function OpeningScene() {
  const { hydrated } = useStore();

  useEffect(() => {
    if (!hydrated) return;
    const timer = window.setTimeout(playOpeningScene, 800);
    return () => window.clearTimeout(timer);
  }, [hydrated]);

  return null;
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <StoreProvider>
    <PageBridge />
    <OpeningScene />
    <DemoNotice />
    <App />
  </StoreProvider>,
);
