/**
 * Filter sidebar. Clicking a Status/Label/Tracker row sets the active filter
 * (clicking the active row again clears back to "all"). Counts are global
 * (computed over the unfiltered list) per the design.
 */

import { useMemo } from "react";
import { useTorrents } from "../../store/torrents";
import { useUi, type ActiveFilter } from "../../store/ui";
import { sidebarCounts } from "../../store/selectors";
import styles from "./FilterSidebar.module.css";

/** The fixed Status rows, in the design's order. */
const STATUS_ROWS: Array<{ key: string; label: string }> = [
  { key: "all", label: "all" },
  { key: "downloading", label: "downloading" },
  { key: "seeding", label: "seeding" },
  { key: "completed", label: "completed" },
  { key: "paused", label: "paused" },
  { key: "stalled", label: "stalled" },
  { key: "error", label: "error" },
];

export function FilterSidebar() {
  const torrents = useTorrents((s) => s.torrents);
  const filter = useUi((s) => s.filter);
  const setFilter = useUi((s) => s.setFilter);

  const counts = useMemo(() => sidebarCounts(torrents), [torrents]);

  /** Toggle a filter: re-clicking the active one clears it. */
  const choose = (next: ActiveFilter) => {
    const same =
      next &&
      filter &&
      next.type === filter.type &&
      next.value === filter.value;
    setFilter(same ? null : next);
  };

  const isActive = (type: string, value: string) =>
    (value === "all" && !filter) ||
    (filter?.type === type && filter.value === value);

  return (
    <div className={styles.sidebar}>
      <div className={styles.group} style={{ paddingTop: 2 }}>
        Status
      </div>
      {STATUS_ROWS.map((row) => (
        <div
          key={row.key}
          className={`${styles.row} ${isActive("status", row.key) ? styles.active : ""}`}
          onClick={() =>
            choose(
              row.key === "all" ? null : { type: "status", value: row.key },
            )
          }
        >
          <span className={styles.label}>{row.label}</span>
          <span className={styles.count}>{counts.status[row.key] ?? 0}</span>
        </div>
      ))}

      {counts.labels.length > 0 && <div className={styles.group}>Labels</div>}
      {counts.labels.map((l) => (
        <div
          key={l.value}
          className={`${styles.row} ${isActive("label", l.value) ? styles.active : ""}`}
          onClick={() => choose({ type: "label", value: l.value })}
        >
          <span className={styles.label}>{l.value}</span>
          <span className={styles.count}>{l.count}</span>
        </div>
      ))}

      {counts.trackers.length > 0 && (
        <div className={styles.group}>Trackers</div>
      )}
      {counts.trackers.map((t) => (
        <div
          key={t.value}
          className={`${styles.row} ${isActive("tracker", t.value) ? styles.active : ""}`}
          onClick={() => choose({ type: "tracker", value: t.value })}
          title={t.value}
        >
          <span className={styles.label}>{t.value}</span>
          <span className={styles.count}>{t.count}</span>
        </div>
      ))}
    </div>
  );
}
