import ReactDOM from "react-dom/client";
import App from "./App";
import { StoreProvider } from "./store";
import "./styles.css";

// No StrictMode: its double-invoked effects would spawn every pseudo-terminal
// twice in development.
ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <StoreProvider>
    <App />
  </StoreProvider>,
);
