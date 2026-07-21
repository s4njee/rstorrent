// Frontend entry point. Loads the theme (tokens first, then global base styles)
// and mounts the React app into #root.
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { setBackend } from "./ipc/backend";
import { tauriBackend } from "./ipc/tauri";
import "./theme/tokens.css";
import "./theme/global.css";

// Register the host backend before anything renders or subscribes.
setBackend(tauriBackend);

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
