/**
 * Root component: wires the live data channels and lays out the main window.
 *
 * On mount it subscribes to the Rust push events (`state://snapshot`,
 * `state://detail`) and keeps selection pruned to existing torrents. A separate
 * effect steers the backend detail poll based on the current selection + tab.
 */

import { useEffect } from "react";
import {
  onSnapshot,
  onDetail,
  onLog,
  onMenuAction,
  onNotificationClick,
  onOpenRequests,
} from "./ipc/events";
import {
  addTorrent,
  setDetailWatch,
  getLog,
  retryConnection,
  takeOpenRequests,
} from "./ipc/commands";
import {
  defaultAddOptions,
  OpenRequestQueue,
  parseOpenRequests,
} from "./externalOpen";
import { useKeyboardShortcuts } from "./hooks/useKeyboard";
import { useTorrents } from "./store/torrents";
import { useUi } from "./store/ui";
import { useDetail } from "./store/detail";
import { useSettings } from "./store/settings";
import { useLog } from "./store/log";
import { useRateHistory } from "./store/rateHistory";
import { TitleBar } from "./components/shell/TitleBar";
import { Toolbar } from "./components/shell/Toolbar";
import { StatusBar } from "./components/shell/StatusBar";
import { FilterSidebar } from "./components/sidebar/FilterSidebar";
import { TorrentTable } from "./components/table/TorrentTable";
import { DetailTabs } from "./components/details/DetailTabs";
import { DialogHost } from "./components/dialogs/DialogHost";
import { ContextMenu } from "./components/menu/ContextMenu";
import { ColumnMenu } from "./components/menu/ColumnMenu";
import styles from "./App.module.css";

/** Minimal `~/.rtorrent.rc` shown on the disconnected card (see docs/rtorrent-setup.md). */
const RTORRENT_RC_SNIPPET = `# ~/.rtorrent.rc  (absolute paths; replace /Users/you)
session.path.set = /Users/you/.rtorrent/session
network.scgi.open_local = /Users/you/.rtorrent/rpc.socket

# then, in a terminal:
#   mkdir -p ~/.rtorrent/session
#   tmux new-session -d -s rtorrent 'rtorrent'`;

export default function App() {
  const connection = useTorrents((s) => s.connection);

  useKeyboardShortcuts();

  // Subscribe to backend push events once, on mount, and load initial state.
  useEffect(() => {
    const applySnapshot = useTorrents.getState().applySnapshot;
    const prune = useUi.getState().pruneSelection;
    const setDetail = useDetail.getState().setDetail;
    const recordRates = useRateHistory.getState().record;
    // Hydrate the log from the ring buffer, then keep it live.
    void getLog().then((entries) => useLog.getState().hydrate(entries));

    const unsubs = [
      onSnapshot((s) => {
        applySnapshot(s);
        prune(new Set(s.torrents.map((t) => t.hash)));
        recordRates(s.torrents);
      }),
      onDetail((d) => setDetail(d)),
      onLog((entry) => useLog.getState().append(entry)),
      // Native menu items open the matching dialog.
      onMenuAction((action) =>
        useUi
          .getState()
          .openDialog(action as "prefs" | "add-file" | "add-magnet" | "stats"),
      ),
      onNotificationClick((hash) => {
        const ui = useUi.getState();
        ui.closeDialog();
        ui.setFilter(null);
        ui.setSearch("");
        ui.select(hash);
        requestAnimationFrame(() => {
          document
            .getElementById(`torrent-row-${hash}`)
            ?.scrollIntoView({ block: "nearest" });
        });
      }),
    ];
    return () => {
      unsubs.forEach((p) => void p.then((un) => un()));
    };
  }, []);

  // Serialize Finder-opened .torrent files and magnet: deep links. The Rust
  // handoff does not emit warm events until takeOpenRequests marks us ready,
  // so subscribing before that command closes the startup race completely.
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    const queue = new OpenRequestQueue(
      async (source) => {
        let settings = useSettings.getState().settings;
        if (!settings) {
          await useSettings.getState().load();
          settings = useSettings.getState().settings;
        }
        if (!settings) throw new Error("settings did not load");

        if (settings.showAddDialog) {
          await new Promise<void>((resolve) =>
            useUi.getState().openExternalAdd(source, resolve),
          );
        } else {
          await addTorrent(source, defaultAddOptions(settings));
        }
      },
      (error, source) => {
        console.error("could not handle external add request", source, error);
      },
    );

    void (async () => {
      if (!useSettings.getState().settings) {
        await useSettings.getState().load();
      }
      if (cancelled) return;

      unlisten = await onOpenRequests((urls) =>
        queue.enqueue(parseOpenRequests(urls)),
      );
      if (cancelled) {
        unlisten();
        return;
      }

      queue.enqueue(parseOpenRequests(await takeOpenRequests()));
    })();

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  // Steer the detail poll: watch the single selected torrent + active tab.
  const selection = useUi((s) => s.selection);
  const activeTab = useUi((s) => s.activeTab);
  useEffect(() => {
    const hash = selection.size === 1 ? [...selection][0] : null;
    void setDetailWatch(hash, hash ? activeTab : null);
  }, [selection, activeTab]);

  const connected = connection.phase === "connected";

  return (
    <div className={styles.app}>
      <TitleBar />
      <Toolbar />
      <div className={styles.body}>
        <FilterSidebar />
        {connected ? (
          <TorrentTable />
        ) : (
          <div className={styles.disconnected}>
            <h2>
              {connection.phase === "connecting"
                ? "connecting to rtorrent…"
                : "can't reach rtorrent"}
            </h2>
            <span className={styles.endpoint}>{connection.endpoint}</span>
            {connection.error && (
              <span className={styles.endpoint}>{connection.error}</span>
            )}
            {connection.retryInSeconds != null && (
              <span className={styles.retry}>
                retrying in {connection.retryInSeconds}s…
              </span>
            )}
            {connection.phase === "disconnected" && (
              <>
                <div className={styles.actions}>
                  <button onClick={() => void retryConnection()}>
                    Retry now
                  </button>
                  <button onClick={() => useUi.getState().openDialog("prefs")}>
                    Open Preferences
                  </button>
                </div>
                <details className={styles.hint}>
                  <summary>rtorrent not running? Show setup snippet</summary>
                  <pre>{RTORRENT_RC_SNIPPET}</pre>
                </details>
              </>
            )}
          </div>
        )}
      </div>
      <DetailTabs />
      <StatusBar />
      <ContextMenu />
      <ColumnMenu />
      <DialogHost />
    </div>
  );
}
