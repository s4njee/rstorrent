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
  onMouseDown: (hash: string, e: React.MouseEvent) => void;
  onContextMenu: (hash: string, e: React.MouseEvent) => void;
}

function RowInner({
  torrent: t,
  alt,
  selected,
  onMouseDown,
  onContextMenu,
}: RowProps) {
  const cls = `${styles.grid} ${styles.row} ${alt ? styles.alt : ""} ${
    selected ? styles.selected : ""
  }`;
  return (
    <div
      className={cls}
      onMouseDown={(e) => onMouseDown(t.hash, e)}
      onContextMenu={(e) => onContextMenu(t.hash, e)}
    >
      <span className={styles.name} title={t.name}>
        {t.name}
      </span>
      <span className={styles.num}>{formatBytes(t.size)}</span>
      <span>
        <ProgressBar percent={t.percent} status={t.status} />
      </span>
      <span
        style={{ color: STATUS_COLOR[t.status] }}
        title={t.statusMsg || undefined}
      >
        {statusLabel(t)}
      </span>
      <span className={styles.num}>{t.seedsSwarm}</span>
      <span className={styles.num}>{t.peersSwarm}</span>
      <span className={styles.down}>
        {formatDownCell(t.downRate, t.status)}
      </span>
      <span className={styles.up}>{formatUpCell(t.upRate)}</span>
      <span className={styles.num}>{formatEta(t.etaSeconds, t.status)}</span>
      <span className={styles.num}>{formatRatio(t.ratio)}</span>
      <span className={styles.dim} title={t.label}>
        {t.label}
      </span>
      <span className={styles.dim} title={t.trackerHost}>
        {t.trackerHost}
      </span>
    </div>
  );
}

export const Row = memo(RowInner);
