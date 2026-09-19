/**
 * The Done cell's progress bar (design frame 1a).
 *
 * A 12px trough with the percentage centred *over the whole bar*, not beside it.
 * The fill is blue below 100%, green at 100% and the disabled grey when a
 * torrent is stopped and incomplete — progress is never the accent.
 */

import type { Status } from "../../ipc/types";
import styles from "./ProgressBar.module.css";

/** Keep daemon/mock values safe for both CSS width and ARIA attributes. */
export function clampPercent(percent: number): number {
  if (!Number.isFinite(percent)) return 0;
  return Math.min(100, Math.max(0, percent));
}

/**
 * The fill colour: blue while incomplete, green when done, grey when stopped
 * and incomplete (nothing is happening, so it reads as inert).
 */
export function fillColor(percent: number, status: Status): string {
  const value = clampPercent(percent);
  if (value >= 100) return "var(--progress-complete)";
  // `paused` is the stopped/queued state; `error` keeps the error colour.
  if (status === "paused") return "var(--state-disabled)";
  if (status === "error") return "var(--status-error)";
  return "var(--progress-active)";
}

/** The label over the bar: `100%` when complete, otherwise one decimal. */
export function percentLabel(percent: number): string {
  const value = clampPercent(percent);
  return value >= 100 ? "100%" : `${value.toFixed(1)}%`;
}

export function ProgressBar({
  percent,
  status,
  showLabel = true,
  compact = false,
}: {
  percent: number;
  status: Status;
  /** The Add dialog's file rows use a bare bar. */
  showLabel?: boolean;
  /** The detail panel's file rows: 10px, where the table's bar is 12px. */
  compact?: boolean;
}) {
  const value = clampPercent(percent);
  return (
    <span
      className={`${styles.track} ${compact ? styles.compact : ""}`}
      role="progressbar"
      aria-label={`${value}% complete`}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(value)}
    >
      <span
        className={styles.fill}
        style={{ width: `${value}%`, background: fillColor(value, status) }}
      />
      {showLabel && <span className={styles.label}>{percentLabel(value)}</span>}
    </span>
  );
}
