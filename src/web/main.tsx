// Web entry point. Registers the HTTP/polling backend, gates on auth, then
// mounts the web shell. Tokens load desktop-first, then tokens.web.css overrides
// the metrics that differ for the browser (25px rows, web column template, …).
import React, { useEffect, useState } from "react";
import ReactDOM from "react-dom/client";
import { setBackend } from "../ipc/backend";
import { webBackend, setUnauthorizedHandler, webLogout } from "../ipc/web";
import { WebApp } from "./WebApp";
import { LoginScreen } from "./LoginScreen";
import "../theme/tokens.css";
import "../theme/tokens.web.css";
import "../theme/global.css";

setBackend(webBackend);

/**
 * Auth gate: probe `/api/health` (which sits behind the session middleware) to
 * decide whether to show the login screen or the app. A mid-session 401 from any
 * request flips back to login via the unauthorized handler.
 */
function WebRoot() {
  const [authed, setAuthed] = useState<boolean | null>(null);

  useEffect(() => {
    setUnauthorizedHandler(() => setAuthed(false));
    fetch("/api/health")
      .then((r) => setAuthed(r.status !== 401))
      // A network error isn't an auth failure; let the poller re-detect a 401.
      .catch(() => setAuthed(true));
  }, []);

  if (authed === null) return null; // brief: waiting on the probe
  if (!authed) return <LoginScreen onSuccess={() => setAuthed(true)} />;
  return (
    <WebApp
      onSignOut={() => {
        void webLogout();
        setAuthed(false);
      }}
    />
  );
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <WebRoot />
  </React.StrictMode>,
);
