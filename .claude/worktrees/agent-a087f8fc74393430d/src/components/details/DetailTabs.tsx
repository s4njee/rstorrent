/**
 * Detail panel for the selected torrent.
 *
 * The tab strip switches the active tab (persisted in the UI store and used to
 * steer the Rust detail poll). The General tab is derived from the snapshot; the
 * Trackers/Peers/Content tabs render data pushed via `state://detail` (wired in
 * App via `onDetail`); Speed/Log are placeholders until E10-S6/S7.
 */

import { useTorrents } from "../../store/torrents";
import { useUi } from "../../store/ui";
import { useDetail } from "../../store/detail";
import { useLog } from "../../store/log";
import { setFilePriority } from "../../ipc/commands";
import type { DetailTab, FileNode, TorrentDto } from "../../ipc/types";
import {
  formatBytes,
  formatRate,
  formatEta,
  formatRatio,
} from "../../utils/format";
import { SpeedChart } from "./SpeedChart";
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
            {selection.size > 1
              ? "multiple torrents selected"
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
      return <General torrent={torrent} />;
    case "trackers":
      return (
        <SimpleTable
          headers={["Tracker", "Status", "Seeds", "Leeches"]}
          rows={(forThis?.trackers ?? []).map((t) => [
            t.url,
            t.status,
            String(t.seeds),
            String(t.leeches),
          ])}
          empty="no tracker data"
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
      return <ContentTable hash={torrent.hash} files={forThis?.files ?? []} />;
    case "speed":
      return <SpeedChart hash={torrent.hash} />;
    case "log":
      return <LogView hash={torrent.hash} />;
    default:
      return <div className={styles.placeholder}>{tab} — coming soon</div>;
  }
}

/** Priority label cycle for the Content tab (0 off → 1 normal → 2 high). */
const PRIORITY_LABELS = ["off", "normal", "high"];

/** Content tab: file list with a clickable priority cell. */
function ContentTable({ hash, files }: { hash: string; files: FileNode[] }) {
  if (files.length === 0)
    return <div className={styles.placeholder}>no file data</div>;
  const cyclePriority = (index: number, current: number) => {
    const next = (current + 1) % 3;
    void setFilePriority(hash, index, next);
  };
  return (
    <table className={styles.dtable}>
      <thead>
        <tr>
          <th>File</th>
          <th>Size</th>
          <th>Progress</th>
          <th>Priority</th>
        </tr>
      </thead>
      <tbody>
        {files.map((f, i) => (
          <tr key={i}>
            <td>{f.path}</td>
            <td>{formatBytes(f.size)}</td>
            <td>{f.progress.toFixed(0)}%</td>
            <td
              style={{
                cursor: "default",
                color:
                  f.priority === 0
                    ? "var(--text-dim)"
                    : f.priority === 2
                      ? "var(--accent-cyan-bright)"
                      : "var(--text-body)",
              }}
              title="click to change priority"
              onClick={() => cyclePriority(i, f.priority)}
            >
              {PRIORITY_LABELS[f.priority] ?? String(f.priority)}
            </td>
          </tr>
        ))}
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
function General({ torrent: t }: { torrent: TorrentDto }) {
  const g = useTorrents.getState().globals;
  const pairs: Array<[string, string]> = [
    ["downloaded", formatBytes(t.bytesDone)],
    ["size", formatBytes(t.size)],
    ["uploaded", formatRatio(t.ratio) + "×"],
    ["ratio", formatRatio(t.ratio)],
    ["eta", formatEta(t.etaSeconds, t.status)],
    ["conns", String(t.peersConnected)],
    ["dl-limit", g.downRateLimit ? formatRate(g.downRateLimit) : "∞"],
    ["ul-limit", g.upRateLimit ? formatRate(g.upRateLimit) : "∞"],
  ];
  return (
    <div className={styles.general}>
      {pairs.map(([k, v]) => (
        <span key={k}>
          <b>{k}:</b> {v}
        </span>
      ))}
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
