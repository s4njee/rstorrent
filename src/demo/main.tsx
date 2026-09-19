/**
 * Browser demo entry (dev-only). Registers an in-memory backend that answers
 * from the fixtures, then mounts the *real* web shell so the UI renders in a
 * plain browser with no server or daemon. A `?screen=` query param sets up each
 * state for screenshots (serve with `npm run dev`, then open):
 *
 *   /demo.html?screen=main    — a torrent selected, sidebar + detail
 *   /demo.html?screen=pieces  — the Pieces pane with the pieces bar
 *   /demo.html?screen=smart   — a saved smart filter + a multi-select
 *
 * Never part of the web console build (web.html → src/web/main.tsx).
 */

import ReactDOM from "react-dom/client";
import { setBackend, type Backend, type UnlistenFn } from "../ipc/backend";
import { initTheme } from "../theme/theme";
import "../theme/palette.css";
import "../theme/themes.css";
import "../theme/tokens.css";
import "../theme/tokens.web.css";
import "../theme/global.css";
import * as fx from "./fixtures";

const screen =
  new URLSearchParams(window.location.search).get("screen") ?? "main";

/** Event subscribers by channel name, fed by {@link emit}. */
const listeners = new Map<string, Set<(payload: unknown) => void>>();

function emit(event: string, payload: unknown): void {
  listeners.get(event)?.forEach((handler) => handler(payload));
}

const demoBackend: Backend = {
  async invoke<T>(command: string, args?: Record<string, unknown>) {
    switch (command) {
      case "get_settings":
        return fx.settings as T;
      case "get_snapshot":
        return fx.snapshot as T;
      case "get_log":
        return fx.log as T;
      case "get_moves":
        return [] as T;
      case "get_statistics":
        return fx.statistics as T;
      case "daemon_health":
        return fx.daemonHealth as T;
      case "set_detail_watch": {
        const a = args as { hash: string | null; tab: string | null };
        if (a.hash && a.tab === "general") {
          const hash = a.hash;
          setTimeout(() => emit("state://detail", fx.piecesDetail(hash)), 20);
        }
        return null as T;
      }
      default:
        // Every mutation / unhandled command is a no-op in the demo.
        return null as T;
    }
  },
  async listen<T>(
    event: string,
    handler: (payload: T) => void,
  ): Promise<UnlistenFn> {
    const set = listeners.get(event) ?? new Set();
    listeners.set(event, set);
    const h = handler as (payload: unknown) => void;
    set.add(h);
    return () => set.delete(h);
  },
};

setBackend(demoBackend);
initTheme();

// Smart filters are read from localStorage when the UI store initializes, so
// seed them before the store module is imported below.
if (screen === "smart") {
  localStorage.setItem(
    "rstorrent.view",
    JSON.stringify({
      sortColumn: "name",
      sortDir: "asc",
      filter: null,
      activeTab: "transfer",
      columns: "",
      smartFilters: [
        { id: "sf_demo", name: "Blender 4K", tracker: "tracker.blender.org" },
      ],
    }),
  );
} else {
  localStorage.removeItem("rstorrent.view");
}

const [{ WebApp }, { useUi }] = await Promise.all([
  import("../web/WebApp"),
  import("../store/ui"),
]);

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <WebApp onSignOut={() => {}} />,
);

function drive() {
  emit("state://snapshot", fx.snapshot);
  const ui = useUi.getState();
  switch (screen) {
    case "pieces":
      ui.setActiveTab("pieces");
      ui.select("G7");
      break;
    case "smart":
      ui.selectAll(["C3", "G7"]);
      break;
    default:
      ui.select("C3");
  }
  // Re-emit once more in case the first raced the shell's subscription.
  setTimeout(() => emit("state://snapshot", fx.snapshot), 150);
}

// Let React mount + subscribe, then feed data and set up the screen.
setTimeout(drive, 80);
