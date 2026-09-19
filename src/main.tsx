// Frontend entry point. Loads the theme layers in order — palette (raw values),
// per-theme overrides, semantic aliases, base styles — then mounts the React app.
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { setBackend } from "./ipc/backend";
import { tauriBackend } from "./ipc/tauri";
import { initTheme } from "./theme/theme";
import "./theme/palette.css";
import "./theme/themes.css";
import "./theme/tokens.css";
import "./theme/global.css";

// Register the host backend before anything renders or subscribes.
setBackend(tauriBackend);

// Resolve and apply the theme before the first render. The HTML shell already
// ran the same resolution inline, so this normally re-applies what is on screen.
initTheme();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
