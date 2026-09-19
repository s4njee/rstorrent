/**
 * Root component: wires the live data channels and lays out the main window.
 *
 * On mount it subscribes to the Rust push events (`state://snapshot`,
 * `state://detail`) and keeps selection pruned to existing torrents. A separate
 * effect steers the backend detail poll based on the current selection + tab.
 *
 * The window is the console's layout: top bar, action toolbar, workspace
 * (sidebar + main column of table / detail panel / status bar), with notices and
 * the connection banner over it.
 */

import { useEffect } from "react";
import {
  onDelta,
  onDetail,
  onLog,
  onMenuAction,
  onMoves,
  onNotificationClick,
  onOpenRequests,
  onSnapshot,
} from "./ipc/events";
import {
  getLog,
  getMoves,
  getSnapshot,
  retryConnection,
  saveSession,
  setDetailWatch,
  startDaemon,
  takeOpenRequests,
} from "./ipc/commands";
import { parseOpenRequests } from "./externalOpen";
import { enqueueAddSources } from "./addQueue";
import { useKeyboardShortcuts } from "./hooks/useKeyboard";
import { useDragDrop } from "./hooks/useDragDrop";
import { usePasteToAdd } from "./hooks/usePasteToAdd";
import { useTorrents } from "./store/torrents";
import { useUi } from "./store/ui";
import { daemonTabFor } from "./utils/panes";
import { useDetail } from "./store/detail";
import { useSettings } from "./store/settings";
import { useLog } from "./store/log";
import { useMoves } from "./store/moves";
import { useRateHistory } from "./store/rateHistory";
import { useTransferHistory } from "./store/transferHistory";
import { TopBar } from "./components/shell/TopBar";
import { ActionToolbar } from "./components/shell/ActionToolbar";
import { StatusBar } from "./components/shell/StatusBar";
import { Notices, LostConnectionBanner } from "./components/Notices";
import { FilterSidebar } from "./components/sidebar/FilterSidebar";
import { DiskCard } from "./components/sidebar/DiskCard";
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
  usePasteToAdd();
  const dragOver = useDragDrop();

  // Subscribe to backend push events once, on mount, and load initial state.
  useEffect(() => {
    const applySnapshot = useTorrents.getState().applySnapshot;
    const prune = useUi.getState().pruneSelection;
    const setDetail = useDetail.getState().setDetail;
    const recordRates = useRateHistory.getState().record;
    const recordTransfer = useTransferHistory.getState().record;
    // Hydrate the log from the ring buffer, then keep it live.
    void getLog().then((entries) => useLog.getState().hydrate(entries));
    // Hydrate move statuses, then keep them live on the moves nudge.
    void getMoves().then((moves) => useMoves.getState().set(moves));

    const unsubs = [
      onSnapshot((s) => {
        applySnapshot(s);
        prune(new Set(s.torrents.map((t) => t.hash)));
        recordRates(s.torrents);
        recordTransfer(s.globals);
      }),
      onDelta((d) => {
        const ok = useTorrents.getState().applyDelta(d);
        if (!ok) {
          // Missed revision — heal via full snapshot.
          void getSnapshot().then((s) => {
            if (s) {
              applySnapshot(s);
              prune(new Set(s.torrents.map((t) => t.hash)));
              recordRates(s.torrents);
              recordTransfer(s.globals);
            }
          });
          return;
        }
        const torrents = useTorrents.getState().torrents;
        prune(new Set(torrents.map((t) => t.hash)));
        recordRates(torrents);
        recordTransfer(d.globals);
      }),
      onDetail((d) => setDetail(d)),
      onLog((entry) => useLog.getState().append(entry)),
      onMoves(() => useMoves.getState().refresh()),
      // Native menu items: most open a dialog; the daemon actions are handled
      // directly (save runs now, shutdown asks for confirmation first).
      onMenuAction((action) => {
        if (action === "save-session") {
          void saveSession();
          return;
        }
        if (action === "start-daemon") {
          void startDaemon().catch(() => {});
          return;
        }
        useUi
          .getState()
          .openDialog(
            action as
              | "prefs"
              | "add-file"
              | "add-magnet"
              | "create-torrent"
              | "stats"
              | "tune-network"
              | "shutdown"
              | "set-location",
          );
      }),
      onNotificationClick((hash) => {
        const ui = useUi.getState();
        ui.closeDialog();
        ui.clearFacets();
        ui.setSearch("");
        ui.select(hash);
        requestAnimationFrame(() => {
          window.dispatchEvent(
            new CustomEvent("scrollToTorrent", { detail: hash }),
          );
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
  // Drag-drop and paste feed the same queue (see addQueue.ts).
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;

    void (async () => {
      if (!useSettings.getState().settings) {
        await useSettings.getState().load();
      }
      if (cancelled) return;

      unlisten = await onOpenRequests((urls) =>
        enqueueAddSources(parseOpenRequests(urls)),
      );
      if (cancelled) {
        unlisten();
        return;
      }

      enqueueAddSources(parseOpenRequests(await takeOpenRequests()));
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
    void setDetailWatch(hash, hash ? daemonTabFor(activeTab) : null);
  }, [selection, activeTab]);

  const connected = connection.phase === "connected";
  const sidebarOpen = useUi((s) => s.sidebarOpen);
  const globals = useTorrents((s) => s.globals);
  const savePath = useSettings((s) => s.settings?.defaultSavePath ?? null);

  return (
    <div
      className={styles.app}
      data-sidebar-open={sidebarOpen ? "" : undefined}
    >
      <TopBar trafficLights />
      <LostConnectionBanner />
      <ActionToolbar />
      <div className={styles.workspace}>
        <aside className={styles.sidebar}>
          <FilterSidebar
            footer={
              <DiskCard
                freeSpace={globals.freeSpace}
                diskSize={globals.diskSize}
                path={savePath}
              />
            }
          />
        </aside>
        <div className={styles.main}>
          {connected ? (
            <>
              <div className={styles.tableArea}>
                <TorrentTable />
              </div>
              <DetailTabs />
            </>
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
                    <button
                      onClick={() =>
                        void startDaemon()
                          .then(() => void retryConnection())
                          .catch(() => {})
                      }
                    >
                      Start rtorrent
                    </button>
                    <button onClick={() => void retryConnection()}>
                      Retry now
                    </button>
                    <button
                      onClick={() => useUi.getState().openDialog("prefs")}
                    >
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
          <StatusBar />
        </div>
      </div>
      <ContextMenu />
      <ColumnMenu />
      <DialogHost />
      <Notices />
      {dragOver && (
        <div className={styles.dropOverlay}>
          <div className={styles.dropCard}>drop .torrent files to add</div>
        </div>
      )}
    </div>
  );
}
