/**
 * Thin progress bar for the Done column. The fill width is the percent complete
 * and the fill color follows the torrent status via a CSS custom property, so
 * it always tracks the design's status palette.
 */

import type { Status } from "../../ipc/types";
import styles from "./ProgressBar.module.css";

/** Map a status to its progress-fill CSS variable. */
const FILL: Record<Status, string> = {
  downloading: "var(--status-downloading)",
  seeding: "var(--status-seeding)",
  completed: "var(--status-seeding)",
  paused: "var(--status-paused)",
  stalled: "var(--status-stalled)",
  checking: "var(--status-checking)",
  error: "var(--status-error)",
};

export function ProgressBar({
  percent,
  status,
}: {
  percent: number;
  status: Status;
}) {
  return (
    <span className={styles.track}>
      <span
        className={styles.fill}
        style={{
          width: `${Math.min(100, Math.max(0, percent))}%`,
          background: FILL[status],
        }}
      />
    </span>
  );
}
