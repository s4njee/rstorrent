/**
 * Browser demo entry (dev-only). Installs a mock Tauri IPC layer backed by the
 * fixtures, then mounts the *real* App so the UI renders in a plain browser with
 * no daemon. A `?screen=` query param sets up each state for screenshots:
 *
 *   demo.html?screen=main    — a torrent selected, sidebar + detail
 *   demo.html?screen=pieces  — General tab with the pieces bar
 *   demo.html?screen=smart   — a saved smart filter + a multi-select
 *   demo.html?screen=prefs   — Preferences → Connection
 *
 * This module is never imported by the Tauri build (index.html → src/main.tsx).
 */

import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import ReactDOM from "react-dom/client";
import "../theme/tokens.css";
import "../theme/global.css";
import * as fx from "./fixtures";

const screen =
  new URLSearchParams(window.location.search).get("screen") ?? "main";

mockWindows("main");

mockIPC(
  (cmd, args) => {
    switch (cmd) {
      case "get_settings":
        return fx.settings;
      case "get_log":
        return fx.log;
      case "get_statistics":
        return fx.statistics;
      case "daemon_health":
        return fx.daemonHealth;
      case "xmlrpc_call":
        return fx.xmlrpcResult((args as { method: string }).method);
      case "take_open_requests":
        return [];
      case "set_detail_watch": {
        const a = args as { hash: string | null; tab: string | null };
        if (a.hash && a.tab === "general") {
          const hash = a.hash;
          setTimeout(
            () => void emit("state://detail", fx.piecesDetail(hash)),
            20,
          );
        }
        return null;
      }
      default:
        // Every mutation / unhandled command is a no-op in the demo.
        return null;
    }
  },
  { shouldMockEvents: true },
);

// Smart filters are read from localStorage when the UI store initializes, so
// seed them before the store module is imported below.
if (screen === "smart") {
  localStorage.setItem(
    "rstorrent.view",
    JSON.stringify({
      sortColumn: "name",
      sortDir: "asc",
      filter: null,
      activeTab: "general",
      columns: "",
      smartFilters: [
        { id: "sf_demo", name: "Blender 4K", tracker: "tracker.blender.org" },
      ],
    }),
  );
} else {
  localStorage.removeItem("rstorrent.view");
}

const [{ default: App }, { useUi }] = await Promise.all([
  import("../App"),
  import("../store/ui"),
]);

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <App />,
);

/** Click a Preferences left-nav item by its visible label. */
function clickNav(label: string) {
  const el = Array.from(document.querySelectorAll("div")).find(
    (d) =>
      typeof d.className === "string" &&
      d.className.includes("navItem") &&
      d.textContent?.includes(label),
  );
  (el as HTMLElement | undefined)?.click();
}

/** Click a button by its exact visible label (for scripted demo screens). */
function clickButton(label: string) {
  const el = Array.from(document.querySelectorAll("button")).find(
    (b) => b.textContent?.trim() === label,
  );
  (el as HTMLElement | undefined)?.click();
}

function drive() {
  void emit("state://snapshot", fx.snapshot);
  const ui = useUi.getState();
  switch (screen) {
    case "pieces":
      ui.select("G7");
      break;
    case "smart":
      ui.selectAll(["C3", "G7"]);
      break;
    case "prefs":
      ui.openDialog("prefs");
      setTimeout(() => clickNav("Connection"), 80);
      break;
    case "xmlrpc":
      ui.select("C3");
      ui.openDialog("xmlrpc");
      // Fire the default method so the result pane is populated for the shot.
      setTimeout(() => clickButton("Run"), 160);
      break;
    default:
      ui.select("C3");
  }
  // Re-emit once more in case the first raced the App's subscription.
  setTimeout(() => void emit("state://snapshot", fx.snapshot), 150);
}

// Let React mount + subscribe, then feed data and set up the screen.
setTimeout(drive, 80);
