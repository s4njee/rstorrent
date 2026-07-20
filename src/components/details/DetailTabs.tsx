/**
 * Detail panel for the selected torrent.
 *
 * The tab strip switches the active tab (persisted in the UI store and used to
 * steer the Rust detail poll). The General tab is derived from the snapshot; the
 * Trackers/Peers/Content tabs render data pushed via `state://detail` (wired in
 * App via `onDetail`); Speed/Log are placeholders until E10-S6/S7.
 */

import { useState, useEffect, type FormEvent } from "react";
import { useTorrents } from "../../store/torrents";
import { useUi } from "../../store/ui";
import { useDetail } from "../../store/detail";
import { useLog } from "../../store/log";
import {
  addTracker,
  removeTracker,
  setFilePriority,
  setTrackerEnabled,
} from "../../ipc/commands";
import type {
  DetailTab,
  FileNode,
  PieceInfo,
  Status,
  TorrentDto,
  TrackerRow,
} from "../../ipc/types";
import {
  formatBytes,
  formatRate,
  formatEta,
  formatRatio,
} from "../../utils/format";
import { SpeedChart } from "./SpeedChart";
import { PieceBar } from "./PieceBar";
import { ProgressBar } from "../table/ProgressBar";
import { PauseIcon, PlayIcon, RemoveIcon } from "../icons";
import menuStyles from "../menu/ContextMenu.module.css";
import styles from "./DetailTabs.module.css";

const TABS: DetailTab[] = [
  "general",
  "trackers",
  "peers",
  "content",
  "speed",
  "log",
];

export function DetailTabs() {
  const activeTab = useUi((s) => s.activeTab);
  const setActiveTab = useUi((s) => s.setActiveTab);
  const selection = useUi((s) => s.selection);
  const torrents = useTorrents((s) => s.torrents);
  const detail = useDetail((s) => s.data);

  // Single-selection drives the detail panel; show the first selected torrent.
  const selectedHash = selection.size === 1 ? [...selection][0] : null;
  const torrent = selectedHash
    ? (torrents.find((t) => t.hash === selectedHash) ?? null)
    : null;

  return (
    <div className={styles.panel}>
      <div className={styles.tabs}>
        {TABS.map((tab) => (
          <span
            key={tab}
            className={`${styles.tab} ${activeTab === tab ? styles.active : ""}`}
            onClick={() => setActiveTab(tab)}
          >
            {tab}
          </span>
        ))}
      </div>
      <div className={styles.content}>
        {!torrent ? (
          <div className={styles.placeholder}>
            {/* The summary bar above already covers the multi-select case. */}
            {selection.size > 1
              ? "select a single torrent to see its details"
              : "select a torrent"}
          </div>
        ) : (
          <TabContent tab={activeTab} torrent={torrent} detail={detail} />
        )}
      </div>
    </div>
  );
}

function TabContent({
  tab,
  torrent,
  detail,
}: {
  tab: DetailTab;
  torrent: TorrentDto;
  detail: ReturnType<typeof useDetail.getState>["data"];
}) {
  const forThis = detail && detail.hash === torrent.hash ? detail : null;

  switch (tab) {
    case "general":
      return <General torrent={torrent} pieces={forThis?.pieces} />;
    case "trackers":
      return (
        <TrackersTable
          key={torrent.hash}
          hash={torrent.hash}
          trackers={forThis?.trackers ?? []}
          message={torrent.statusMsg}
        />
      );
    case "peers":
      return (
        <SimpleTable
          headers={["Address", "Client", "Done", "Down", "Up", "Flags"]}
          rows={(forThis?.peers ?? []).map((p) => [
            p.address,
            p.client,
            `${p.progress.toFixed(0)}%`,
            formatRate(p.downRate),
            formatRate(p.upRate),
            p.flags,
          ])}
          empty="no peers"
        />
      );
    case "content":
      // Key by hash so the optimistic-priority state resets when the selected
      // torrent changes (otherwise an override would bleed onto another torrent).
      return (
        <ContentTable
          key={torrent.hash}
          hash={torrent.hash}
          files={forThis?.files ?? []}
        />
      );
    case "speed":
      return <SpeedChart hash={torrent.hash} />;
    case "log":
      return <LogView hash={torrent.hash} />;
    default:
      return <div className={styles.placeholder}>{tab} — coming soon</div>;
  }
}

interface TrackerMenuState {
  x: number;
  y: number;
  tracker: TrackerRow;
}

/** Tracker detail rows with inline add and row-level management. */
function TrackersTable({
  hash,
  trackers,
  message,
}: {
  hash: string;
  trackers: TrackerRow[];
  // The torrent's d.message. rtorrent reports a tracker/storage failure here
  // (e.g. "Tracker: [network error: ETIMEDOUT]") rather than on the individual
  // tracker row, so without showing it the Trackers tab looks fine while the
  // torrent is flagged in error. Empty when healthy.
  message: string;
}) {
  const [url, setUrl] = useState("");
  const [adding, setAdding] = useState(false);
  const [menu, setMenu] = useState<TrackerMenuState | null>(null);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    const announceUrl = url.trim();
    if (!announceUrl || adding) return;
    setAdding(true);
    try {
      await addTracker(hash, announceUrl);
      setUrl("");
    } catch {
      // The Rust command writes the failure to the app log.
    } finally {
      setAdding(false);
    }
  };

  const runMenuAction = async (action: () => Promise<void>) => {
    setMenu(null);
    try {
      await action();
    } catch {
      // The Rust command writes the failure to the app log.
    }
  };

  return (
    <div className={styles.trackerPane}>
      {message && (
        <div className={styles.trackerError} role="status">
          {message}
        </div>
      )}
      <form
        className={styles.trackerAdd}
        onSubmit={(event) => void submit(event)}
      >
        <input
          aria-label="Tracker announce URL"
          placeholder="announce URL…"
          value={url}
          onChange={(event) => setUrl(event.currentTarget.value)}
        />
        <button type="submit" disabled={!url.trim() || adding}>
          {adding ? "adding…" : "add tracker"}
        </button>
      </form>

      {trackers.length === 0 ? (
        <div className={styles.placeholder}>no tracker data</div>
      ) : (
        <table className={styles.dtable}>
          <thead>
            <tr>
              <th>Tracker</th>
              <th>Status</th>
              <th>Seeds</th>
              <th>Leeches</th>
              <th>Last announce</th>
            </tr>
          </thead>
          <tbody>
            {trackers.map((tracker) => (
              <tr
                key={`${tracker.index}:${tracker.url}`}
                className={tracker.enabled ? "" : styles.trackerDisabled}
                onContextMenu={(event) => {
                  event.preventDefault();
                  setMenu({
                    x: event.clientX,
                    y: event.clientY,
                    tracker,
                  });
                }}
              >
                <td>{tracker.url}</td>
                <td
                  className={
                    tracker.status === "error" ? styles.statusError : ""
                  }
                >
                  {tracker.status}
                </td>
                <td>{tracker.seeds}</td>
                <td>{tracker.leeches}</td>
                <td>{tracker.lastAnnounce || "—"}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      {menu && (
        <>
          <div
            className={menuStyles.overlay}
            onMouseDown={() => setMenu(null)}
            onContextMenu={(event) => event.preventDefault()}
          />
          <div
            className={menuStyles.menu}
            style={{
              left: Math.min(menu.x, window.innerWidth - 220),
              top: Math.min(menu.y, window.innerHeight - 110),
            }}
          >
            <div
              className={menuStyles.item}
              onClick={() =>
                void runMenuAction(() =>
                  setTrackerEnabled(
                    hash,
                    menu.tracker.index,
                    !menu.tracker.enabled,
                  ),
                )
              }
            >
              <span className={menuStyles.icon}>
                {menu.tracker.enabled ? (
                  <PauseIcon size={11} />
                ) : (
                  <PlayIcon size={11} />
                )}
              </span>
              {menu.tracker.enabled ? "Disable" : "Enable"}
            </div>
            <div className={menuStyles.sep} />
            <div
              className={`${menuStyles.item} ${menuStyles.danger}`}
              onClick={() =>
                void runMenuAction(() =>
                  removeTracker(hash, menu.tracker.index),
                )
              }
            >
              <span className={menuStyles.icon}>
                <RemoveIcon size={11} />
              </span>
              Remove
            </div>
          </div>
        </>
      )}
    </div>
  );
}

/** Priority label cycle for the Content tab (0 off → 1 normal → 2 high). */
const PRIORITY_LABELS = ["skip", "normal", "high"];

/** Per-file progress-bar status: skipped files read dim, complete ones green. */
function fileStatus(priority: number, progress: number): Status {
  if (priority === 0) return "paused";
  return progress >= 100 ? "completed" : "downloading";
}

/** Content tab: file list with per-file progress bars and a click-to-cycle
 *  priority cell (skip → normal → high). */
function ContentTable({ hash, files }: { hash: string; files: FileNode[] }) {
  // Optimistic priority overrides keyed by file index, so a click updates the
  // cell instantly instead of waiting for the ~2s detail poll. An entry is
  // dropped once the incoming file data agrees, keeping this from masking a
  // rejected change (the Rust command logs failures).
  const [pending, setPending] = useState<Record<number, number>>({});

  // Prune overrides the polled data has caught up with. In an effect, not
  // during render, so we never setState mid-render.
  useEffect(() => {
    setPending((p) => {
      let changed = false;
      const next: Record<number, number> = {};
      for (const [k, v] of Object.entries(p)) {
        if (files[Number(k)]?.priority === v) changed = true;
        else next[Number(k)] = v;
      }
      return changed ? next : p;
    });
  }, [files]);

  if (files.length === 0)
    return <div className={styles.placeholder}>no file data</div>;

  const priorityOf = (index: number, actual: number) =>
    pending[index] ?? actual;

  const cyclePriority = (index: number, current: number) => {
    const next = (current + 1) % 3;
    setPending((p) => ({ ...p, [index]: next }));
    void setFilePriority(hash, index, next);
  };

  return (
    <table className={styles.dtable}>
      <thead>
        <tr>
          <th>File</th>
          <th>Size</th>
          <th className={styles.fileProgressCol}>Progress</th>
          <th>Priority</th>
        </tr>
      </thead>
      <tbody>
        {files.map((f, i) => {
          const priority = priorityOf(i, f.priority);
          return (
            <tr key={i} className={priority === 0 ? styles.fileSkipped : ""}>
              <td title={f.path}>{f.path}</td>
              <td>{formatBytes(f.size)}</td>
              <td className={styles.fileProgressCol}>
                <ProgressBar
                  percent={f.progress}
                  status={fileStatus(priority, f.progress)}
                />
              </td>
              <td
                className={styles.filePriority}
                title="click to change priority (skip / normal / high)"
                onClick={() => cyclePriority(i, priority)}
              >
                {PRIORITY_LABELS[priority] ?? String(priority)}
              </td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}

/** Log tab: app event log, newest at the bottom; entries for the selected
 *  torrent are highlighted, errors shown in red. */
function LogView({ hash }: { hash: string }) {
  const entries = useLog((s) => s.entries);
  if (entries.length === 0)
    return <div className={styles.placeholder}>no log entries yet</div>;
  return (
    <div style={{ fontSize: "10.5px", lineHeight: 1.6 }}>
      {entries.map((e, i) => {
        const time = new Date(e.time).toLocaleTimeString();
        const color =
          e.level === "error"
            ? "var(--accent-red)"
            : e.level === "warn"
              ? "var(--accent-amber)"
              : "var(--text-body)";
        const mine = e.hash === hash;
        return (
          <div
            key={i}
            style={{
              color,
              background: mine ? "var(--bg-selected)" : undefined,
              padding: "0 4px",
            }}
          >
            <span style={{ color: "var(--text-dim)" }}>{time}</span> {e.message}
          </div>
        );
      })}
    </div>
  );
}

/** General tab: 4-column label/value grid from the snapshot. */
function General({
  torrent: t,
  pieces,
}: {
  torrent: TorrentDto;
  pieces?: PieceInfo;
}) {
  const g = useTorrents((state) => state.globals);
  const downLimit = t.downRateLimit ?? g.downRateLimit;
  const upLimit = t.upRateLimit ?? g.upRateLimit;
  const limitSource = t.throttleName ? " · torrent" : " · global";
  const pairs: Array<[string, string]> = [
    ["downloaded", formatBytes(t.bytesDone)],
    ["size", formatBytes(t.size)],
    ["uploaded", formatRatio(t.ratio) + "×"],
    ["ratio", formatRatio(t.ratio)],
    ["eta", formatEta(t.etaSeconds, t.status)],
    ["conns", String(t.peersConnected)],
    ["dl-limit", (downLimit ? formatRate(downLimit) : "∞") + limitSource],
    ["ul-limit", (upLimit ? formatRate(upLimit) : "∞") + limitSource],
  ];
  // Private flag from the torrent's info dict (C7): DHT/PEX are off and the
  // tracker is the only peer source — worth knowing when it stalls.
  if (t.isPrivate) pairs.push(["flags", "private"]);
  return (
    <div>
      {/* Pieces bar arrives on the detail poll; absent for a moment on select. */}
      {pieces && pieces.sizeChunks > 0 && (
        <PieceBar pieces={pieces} status={t.status} />
      )}
      <div className={styles.general}>
        {pairs.map(([k, v]) => (
          <span key={k}>
            <b>{k}:</b> {v}
          </span>
        ))}
      </div>
    </div>
  );
}

function SimpleTable({
  headers,
  rows,
  empty,
}: {
  headers: string[];
  rows: string[][];
  empty: string;
}) {
  if (rows.length === 0)
    return <div className={styles.placeholder}>{empty}</div>;
  return (
    <table className={styles.dtable}>
      <thead>
        <tr>
          {headers.map((h) => (
            <th key={h}>{h}</th>
          ))}
        </tr>
      </thead>
      <tbody>
        {rows.map((r, i) => (
          <tr key={i}>
            {r.map((c, j) => (
              <td key={j}>{c}</td>
            ))}
          </tr>
        ))}
      </tbody>
    </table>
  );
}
