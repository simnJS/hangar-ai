import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "@fontsource-variable/inter/opsz.css";
import "./styles.css";
import { App } from "./App";
import { locale } from "./i18n";
import { bootstrap } from "./remote";

const root = document.documentElement;
root.lang = locale;
// `?theme=light|dark` overrides the phone's, to look at both side by side.
const theme = new URLSearchParams(location.search).get("theme");
if (theme === "light" || theme === "dark") root.dataset.theme = theme;

void bootstrap().then((boot) => {
  createRoot(document.getElementById("root")!).render(
    <StrictMode>
      <App boot={boot} />
    </StrictMode>,
  );
});

// The app shell, cached for a start without the network (src/mobile/sw.js).
if (import.meta.env.PROD && "serviceWorker" in navigator) {
  addEventListener("load", () => {
    navigator.serviceWorker.register("./sw.js").catch(() => {
      // Without it the app still works, it only starts from the network.
    });
  });
}
