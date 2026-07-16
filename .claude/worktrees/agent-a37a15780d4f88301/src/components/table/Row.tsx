/**
 * A single torrent row, memoized so it only re-renders when its torrent object
 * identity changes (the snapshot reconciler preserves identity for unchanged
 * rows) or when its zebra/selected flags flip.
 *
 * Mouse-down (not click) drives selection so shift/⌘ modifiers behave like a
 * native list; the parent passes a stable handler.
 */

import { memo } from "react";
import type { Status, TorrentDto } from "../../ipc/types";
import type { ColumnId } from "./columns";
import {
  formatBytes,
  formatDownCell,
  formatUpCell,
  formatEta,
  formatRatio,
} from "../../utils/format";
import { ProgressBar } from "./ProgressBar";
import styles from "./TorrentTable.module.css";

/** Status → text color variable (mirrors the progress-fill palette). */
const STATUS_COLOR: Record<Status, string> = {
  downloading: "var(--status-downloading)",
  seeding: "var(--status-seeding)",
  completed: "var(--status-seeding)",
  paused: "var(--status-paused)",
  stalled: "var(--status-stalled)",
  checking: "var(--status-checking)",
  error: "var(--status-error)",
};

/** Short lowercase status label (design shows "trk error" for tracker errors). */
function statusLabel(t: TorrentDto): string {
  if (t.status === "error") return "trk error";
  return t.status;
}

interface RowProps {
  torrent: TorrentDto;
  alt: boolean;
  selected: boolean;
  visibleColumnIds: readonly ColumnId[];
  onMouseDown: (hash: string, e: React.MouseEvent) => void;
  onContextMenu: (hash: string, e: React.MouseEvent) => void;
}

function RowInner({
  torrent: t,
  alt,
  selected,
  visibleColumnIds,
  onMouseDown,
  onContextMenu,
}: RowProps) {
  const cls = `${styles.grid} ${styles.row} ${alt ? styles.alt : ""} ${
    selected ? styles.selected : ""
  }`;
  const cells: Record<ColumnId, React.ReactNode> = {
    name: (
      <span key="name" className={styles.name} title={t.name}>
        {t.name}
      </span>
    ),
    size: (
      <span key="size" className={styles.num}>
        {formatBytes(t.size)}
      </span>
    ),
    done: (
      <span key="done">
        <ProgressBar percent={t.percent} status={t.status} />
      </span>
    ),
    status: (
      <span
        key="status"
        style={{ color: STATUS_COLOR[t.status] }}
        title={t.statusMsg || undefined}
      >
        {statusLabel(t)}
      </span>
    ),
    seeds: (
      <span key="seeds" className={styles.num}>
        {t.seedsSwarm}
      </span>
    ),
    peers: (
      <span key="peers" className={styles.num}>
        {t.peersSwarm}
      </span>
    ),
    down: (
      <span key="down" className={styles.down}>
        {formatDownCell(t.downRate, t.status)}
      </span>
    ),
    up: (
      <span key="up" className={styles.up}>
        {formatUpCell(t.upRate)}
      </span>
    ),
    eta: (
      <span key="eta" className={styles.num}>
        {formatEta(t.etaSeconds, t.status)}
      </span>
    ),
    ratio: (
      <span key="ratio" className={styles.num}>
        {formatRatio(t.ratio)}
      </span>
    ),
    label: (
      <span key="label" className={styles.dim} title={t.label}>
        {t.label}
      </span>
    ),
    tracker: (
      <span key="tracker" className={styles.dim} title={t.trackerHost}>
        {t.trackerHost}
      </span>
    ),
  };

  return (
    <div
      className={cls}
      onMouseDown={(e) => onMouseDown(t.hash, e)}
      onContextMenu={(e) => onContextMenu(t.hash, e)}
    >
      {visibleColumnIds.map((id) => cells[id])}
    </div>
  );
}

export const Row = memo(RowInner);
