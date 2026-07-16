/**
 * Root component: wires the live data channels and lays out the main window.
 *
 * On mount it subscribes to the Rust push events (`state://snapshot`,
 * `state://detail`) and keeps selection pruned to existing torrents. A separate
 * effect steers the backend detail poll based on the current selection + tab.
 */

import { useEffect } from "react";
import { onSnapshot, onDetail, onLog, onMenuAction } from "./ipc/events";
import { setDetailWatch, getLog } from "./ipc/commands";
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
import styles from "./App.module.css";

export default function App() {
  const connection = useTorrents((s) => s.connection);

  useKeyboardShortcuts();

  // Subscribe to backend push events once, on mount, and load initial state.
  useEffect(() => {
    const applySnapshot = useTorrents.getState().applySnapshot;
    const prune = useUi.getState().pruneSelection;
    const setDetail = useDetail.getState().setDetail;
    const recordRates = useRateHistory.getState().record;
    void useSettings.getState().load();
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
    ];
    return () => {
      unsubs.forEach((p) => void p.then((un) => un()));
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
          </div>
        )}
      </div>
      <DetailTabs />
      <StatusBar />
      <ContextMenu />
      <DialogHost />
    </div>
  );
}
