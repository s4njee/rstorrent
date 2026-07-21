/**
 * The web shell (WE2).
 *
 * Composes the web chrome — AppBar, ActionStrip, Footer, and the sidebar disk
 * card — around the *shared* components (FilterSidebar, TorrentTable, DetailTabs,
 * ContextMenu, DialogHost). The live-data wiring mirrors the desktop App, minus
 * the desktop-only channels (native menus, deep links, notifications) and the
 * Tauri drag-drop hook; browser add flows land in WE4.
 */

import { useEffect, useState } from "react";
import { onSnapshot, onDetail, onLog } from "../ipc/events";
import { getLog, setDetailWatch } from "../ipc/commands";
import { useTorrents } from "../store/torrents";
import { useUi } from "../store/ui";
import { useDetail } from "../store/detail";
import { useLog } from "../store/log";
import { useRateHistory } from "../store/rateHistory";
import { useKeyboardShortcuts } from "../hooks/useKeyboard";
import { FilterSidebar } from "../components/sidebar/FilterSidebar";
import { TorrentTable } from "../components/table/TorrentTable";
import { DetailTabs } from "../components/details/DetailTabs";
import { ContextMenu } from "../components/menu/ContextMenu";
import { DialogHost } from "../components/dialogs/DialogHost";
import { AppBar } from "./AppBar";
import { ActionStrip } from "./ActionStrip";
import { Footer } from "./Footer";
import { DiskCard } from "./DiskCard";
import { StatusDialog } from "./StatusDialog";

export function WebApp({ onSignOut }: { onSignOut: () => void }) {
  const connection = useTorrents((s) => s.connection);
  const globals = useTorrents((s) => s.globals);
  const [displayName, setDisplayName] = useState("rt");
  const [statusOpen, setStatusOpen] = useState(false);

  // Space pause/resume, Delete → remove confirm, arrows + modifiers for
  // selection, ⌘/Ctrl-A select-all (`/` focus-search is handled in AppBar).
  useKeyboardShortcuts();

  // Live data channels (see App.tsx for the desktop counterpart).
  useEffect(() => {
    const applySnapshot = useTorrents.getState().applySnapshot;
    const prune = useUi.getState().pruneSelection;
    const setDetail = useDetail.getState().setDetail;
    const recordRates = useRateHistory.getState().record;
    void getLog().then((entries) => useLog.getState().hydrate(entries));

    const unsubs = [
      onSnapshot((s) => {
        applySnapshot(s);
        prune(new Set(s.torrents.map((t) => t.hash)));
        recordRates(s.torrents);
      }),
      onDetail((d) => setDetail(d)),
      onLog((entry) => useLog.getState().append(entry)),
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
    void setDetailWatch(hash, hash ? activeTab : null);
  }, [selection, activeTab]);

  const connected = connection.phase === "connected";

  return (
    <div style={S.app}>
      <AppBar
        displayName={displayName}
        onOpenStatus={() => setStatusOpen(true)}
      />
      <div style={S.body}>
        <FilterSidebar
          footer={
            <DiskCard
              freeSpace={globals.freeSpace}
              diskSize={globals.diskSize}
            />
          }
        />
        {connected ? (
          <div style={S.main}>
            <ActionStrip />
            <div style={S.tableArea}>
              <TorrentTable />
            </div>
            <DetailTabs />
          </div>
        ) : (
          <div style={S.disconnected}>
            <h2 style={{ margin: 0, fontWeight: 600 }}>
              {connection.phase === "connecting"
                ? "connecting to rtorrent…"
                : "can't reach rtorrent"}
            </h2>
            <span style={{ color: "var(--text-dim)" }}>
              {connection.endpoint}
            </span>
            {connection.error && (
              <span style={{ color: "var(--accent-red)" }}>
                {connection.error}
              </span>
            )}
            {connection.retryInSeconds != null && (
              <span style={{ color: "var(--text-dim)" }}>
                retrying in {connection.retryInSeconds}s…
              </span>
            )}
          </div>
        )}
      </div>
      <Footer />
      <ContextMenu />
      <DialogHost />
      {statusOpen && (
        <StatusDialog
          onClose={() => setStatusOpen(false)}
          onSignOut={onSignOut}
        />
      )}
    </div>
  );
}

const S = {
  app: {
    display: "flex",
    flexDirection: "column",
    height: "100vh",
    minWidth: 1000,
    background: "var(--bg-field)",
    color: "var(--text-body)",
    fontFamily: "var(--font-mono)",
    fontSize: "var(--fs-base)",
  } as const,
  body: { display: "flex", flex: 1, minHeight: 0 } as const,
  main: {
    flex: 1,
    minWidth: 0,
    display: "flex",
    flexDirection: "column",
  } as const,
  tableArea: {
    flex: 1,
    minHeight: 0,
    display: "flex",
    flexDirection: "column",
  } as const,
  disconnected: {
    flex: 1,
    display: "flex",
    flexDirection: "column",
    alignItems: "center",
    justifyContent: "center",
    gap: 8,
    background: "var(--bg-app)",
  } as const,
};
