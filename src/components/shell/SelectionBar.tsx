/**
 * C3 — multi-selection summary bar.
 *
 * Sits above the detail tabs and appears only when two or more torrents are
 * selected, where the detail panel can't say anything useful about a single
 * torrent. Shows the aggregate (count, total size, combined rates) and the
 * actions that make sense on a group.
 *
 * Resume/Pause are offered independently rather than as one toggle: a mixed
 * selection has no single "current" state, so guessing which way a toggle
 * should go would be wrong half the time.
 */

import { useMemo } from "react";
import { useTorrents } from "../../store/torrents";
import { useUi } from "../../store/ui";
import { selectionSummary } from "../../store/selectors";
import { formatBytes, formatRate } from "../../utils/format";
import * as actions from "../../actions";
import { PauseIcon, PlayIcon, RemoveIcon } from "../icons";
import styles from "./SelectionBar.module.css";

export function SelectionBar() {
  const selection = useUi((s) => s.selection);
  const torrents = useTorrents((s) => s.torrents);

  const summary = useMemo(
    () => selectionSummary(torrents, selection),
    [torrents, selection],
  );

  // Single selection is the detail panel's job; zero has nothing to say.
  if (summary.count < 2) return null;

  const hashes = [...selection];

  return (
    <div className={styles.bar}>
      <span className={styles.count}>{summary.count} selected</span>
      <span className={styles.sep}>·</span>
      <span className={styles.stat}>{formatBytes(summary.size)}</span>
      {summary.downRate > 0 && (
        <span className={styles.down}>↓ {formatRate(summary.downRate)}</span>
      )}
      {summary.upRate > 0 && (
        <span className={styles.up}>↑ {formatRate(summary.upRate)}</span>
      )}
      {summary.paused > 0 && (
        <span className={styles.stat}>{summary.paused} paused</span>
      )}

      <span className={styles.grow} />

      <button
        className={styles.action}
        onClick={() => actions.resume(hashes)}
        title="Resume selected"
      >
        <PlayIcon size={10} />
        Resume
      </button>
      <button
        className={styles.action}
        onClick={() => actions.pause(hashes)}
        title="Pause selected"
      >
        <PauseIcon size={10} />
        Pause
      </button>
      <button
        className={`${styles.action} ${styles.danger}`}
        onClick={() => actions.requestRemove()}
        title="Remove selected"
      >
        <RemoveIcon size={10} />
        Remove
      </button>
    </div>
  );
}
