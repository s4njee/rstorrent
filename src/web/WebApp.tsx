/**
 * The web shell.
 *
 * Same chrome and layout as the desktop app — top bar, action toolbar, workspace
 * (sidebar + table / detail panel / status bar) — with the browser-only additions:
 * the account chip in the top bar (sign out) and the server-supplied display name.
 * The live-data wiring mirrors the desktop App minus its native channels (menus,
 * deep links, notifications).
 */

import { useEffect, useState } from "react";
import { onDelta, onDetail, onLog, onMoves, onSnapshot } from "../ipc/events";
import { getLog, getMoves, getSnapshot, setDetailWatch } from "../ipc/commands";
import { useMoves } from "../store/moves";
import { useTorrents } from "../store/torrents";
import { useUi } from "../store/ui";
import { daemonTabFor } from "../utils/panes";
import { useDetail } from "../store/detail";
import { useLog } from "../store/log";
import { useSettings } from "../store/settings";
import { useRateHistory } from "../store/rateHistory";
import { useTransferHistory } from "../store/transferHistory";
import { useKeyboardShortcuts } from "../hooks/useKeyboard";
import { usePasteToAdd } from "../hooks/usePasteToAdd";
import { useFileSearch } from "../hooks/useFileSearch";
import { useWebDragDrop } from "../hooks/useWebDragDrop";
import { TopBar } from "../components/shell/TopBar";
import { ActionToolbar } from "../components/shell/ActionToolbar";
import { StatusBar } from "../components/shell/StatusBar";
import { Notices, LostConnectionBanner } from "../components/Notices";
import { FilterSidebar } from "../components/sidebar/FilterSidebar";
import { DiskCard } from "../components/sidebar/DiskCard";
import { TorrentTable } from "../components/table/TorrentTable";
import { DetailTabs } from "../components/details/DetailTabs";
import { ContextMenu } from "../components/menu/ContextMenu";
import { DialogHost } from "../components/dialogs/DialogHost";
import { StatusDialog } from "./StatusDialog";
import { SettingsPage } from "./SettingsPage";
import { StatsPage } from "./StatsPage";
import { navigate, useRoute } from "./router";
import styles from "../App.module.css";

/** The account chip: initials, opening the server/session dialog. */
function AccountChip({
  initials,
  onClick,
}: {
  initials: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      title="Server and session"
      aria-label="Server and session"
      style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        width: 24,
        height: 24,
        flex: "none",
        padding: 0,
        border: "1px solid var(--border-control)",
        borderRadius: "50%",
        background: "var(--bg-control)",
        color: "var(--text-secondary)",
        fontSize: "var(--fs-chip)",
        fontWeight: 600,
        cursor: "default",
      }}
    >
      {initials}
    </button>
  );
}

export function WebApp({ onSignOut }: { onSignOut: () => void }) {
  const route = useRoute();
  const connection = useTorrents((s) => s.connection);
  const globals = useTorrents((s) => s.globals);
  const [displayName, setDisplayName] = useState("rt");
  const [statusOpen, setStatusOpen] = useState(false);

  useKeyboardShortcuts();
  usePasteToAdd();
  useFileSearch();
  const dragOver = useWebDragDrop();

  // Live data channels (see App.tsx for the desktop counterpart).
  useEffect(() => {
    const applySnapshot = useTorrents.getState().applySnapshot;
    const prune = useUi.getState().pruneSelection;
    const setDetail = useDetail.getState().setDetail;
    const recordRates = useRateHistory.getState().record;
    const recordTransfer = useTransferHistory.getState().record;
    void getLog().then((entries) => useLog.getState().hydrate(entries));
    void getMoves().then((moves) => useMoves.getState().set(moves));
    // Server facts (incl. bandwidth rules for the precedence display).
    void useSettings.getState().load().catch(() => {});

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
    ];
    return () => unsubs.forEach((p) => void p.then((un) => un()));
  }, []);

  // Avatar initials come from the server config.
  useEffect(() => {
    let cancelled = false;
    fetch("/api/health")
      .then((r) => (r.ok ? r.json() : null))
      .then((h) => {
        if (!cancelled && h?.server?.displayName)
          setDisplayName(h.server.displayName);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, []);

  // Steer the detail poll to the single selected torrent + active tab.
  const selection = useUi((s) => s.selection);
  const activeTab = useUi((s) => s.activeTab);
  useEffect(() => {
    const hash = selection.size === 1 ? [...selection][0] : null;
    void setDetailWatch(hash, hash ? daemonTabFor(activeTab) : null);
  }, [selection, activeTab]);

  const connected = connection.phase === "connected";
  const sidebarOpen = useUi((s) => s.sidebarOpen);
  const savePath = useSettings((s) => s.settings?.defaultSavePath ?? null);
  const initials = displayName.slice(0, 2).toLowerCase();

  if (route === "/settings") {
    return (
      <div className={styles.app}>
        <TopBar
          account={
            <AccountChip
              initials={initials}
              onClick={() => setStatusOpen(true)}
            />
          }
          onSettings={() => navigate("/settings")}
          onStats={() => navigate("/stats")}
        />
        <LostConnectionBanner />
        <SettingsPage onBack={() => navigate("/")} />
        {statusOpen && (
          <StatusDialog
            onClose={() => setStatusOpen(false)}
            onSignOut={onSignOut}
          />
        )}
        <Notices />
      </div>
    );
  }

  if (route === "/stats") {
    return (
      <div className={styles.app}>
        <TopBar
          account={
            <AccountChip
              initials={initials}
              onClick={() => setStatusOpen(true)}
            />
          }
          onSettings={() => navigate("/settings")}
          onStats={() => navigate("/stats")}
        />
        <LostConnectionBanner />
        <StatsPage onBack={() => navigate("/")} />
        {statusOpen && (
          <StatusDialog
            onClose={() => setStatusOpen(false)}
            onSignOut={onSignOut}
          />
        )}
        <Notices />
      </div>
    );
  }

  return (
    <div
      className={styles.app}
      data-sidebar-open={sidebarOpen ? "" : undefined}
    >
      <TopBar
        account={
          <AccountChip
            initials={initials}
            onClick={() => setStatusOpen(true)}
          />
        }
        onSettings={() => navigate("/settings")}
        onStats={() => navigate("/stats")}
      />
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
            </div>
          )}
          <StatusBar />
        </div>
      </div>
      <ContextMenu />
      <DialogHost />
      <Notices />
      {statusOpen && (
        <StatusDialog
          onClose={() => setStatusOpen(false)}
          onSignOut={onSignOut}
        />
      )}
      {dragOver && (
        <div className={styles.dropOverlay}>
          <div className={styles.dropCard}>drop .torrent files to add</div>
        </div>
      )}
    </div>
  );
}
