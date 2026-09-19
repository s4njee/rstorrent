/**
 * A single torrent row, memoized so it only re-renders when its torrent object
 * identity changes (the snapshot reconciler preserves identity for unchanged
 * rows) or when its zebra/selected flags flip.
 *
 * Mouse-down (not click) drives selection so shift/⌘ modifiers behave like a
 * native list; the parent passes a stable handler.
 *
 * The cells are the console's (design frame 1a): a select box, the name, and
 * right-aligned numerics with the design's formats — the Done cell is a trough
 * with the percentage centred over the whole bar, status is a coloured word
 * (`Checking 42%`), Seeds/Peers drops the seed count while seeding, and idle
 * rates read as an em-dash rather than `0 B/s`.
 */

import { memo } from "react";
import type { TorrentDto } from "../../ipc/types";
import type { ColumnId } from "./columns";
import {
  formatAdded,
  formatBytes,
  formatDownCell,
  formatEta,
  formatRatio,
  formatUpCell,
} from "../../utils/format";
import { ratioTone, seedsPeersCell, statusWord } from "../../utils/status";
import { labelChip } from "../../utils/labels";
import { tagChip } from "../../utils/tags";
import { ProgressBar } from "./ProgressBar";
import styles from "./TorrentTable.module.css";

/** Status tone → text colour. Progress fills are never the accent (see WC1). */
const TONE_CLASS: Record<string, string> = {
  active: styles.toneActive,
  seeding: styles.toneSeeding,
  idle: styles.toneIdle,
  warn: styles.toneWarn,
  error: styles.toneError,
};

interface RowProps {
  torrent: TorrentDto;
  alt: boolean;
  selected: boolean;
  visibleColumnIds: readonly ColumnId[];
  onMouseDown: (hash: string, e: React.MouseEvent) => void;
  onContextMenu: (hash: string, e: React.MouseEvent) => void;
  onToggleSelect: (hash: string) => void;
  /** Position in the grid, counting the header row; the count changes as the
   *  list is filtered, so a screen reader is told where a row sits. */
  rowIndex: number;
  /** True for a torrent that appeared since the previous render (WC9-S5): the
   *  row fades in rather than snapping. */
  entering?: boolean;
}

function RowInner({
  torrent: t,
  alt,
  selected,
  visibleColumnIds,
  onMouseDown,
  onContextMenu,
  onToggleSelect,
  rowIndex,
  entering = false,
}: RowProps) {
  const cls = `${styles.grid} ${styles.row} ${alt ? styles.alt : ""} ${
    selected ? styles.selected : ""
  }`;

  const word = statusWord(t);
  const chip = labelChip(t.label);

  const cells: Record<ColumnId, React.ReactNode> = {
    select: (
      <span key="select" role="gridcell" className={styles.selectCell}>
        <button
          type="button"
          role="checkbox"
          aria-checked={selected}
          aria-label={`Select ${t.name}`}
          className={`${styles.checkbox} ${selected ? styles.checkboxOn : ""}`}
          onClick={(event) => {
            // The row's own mouse-down would otherwise collapse the selection
            // to this row before the toggle lands.
            event.stopPropagation();
            onToggleSelect(t.hash);
          }}
          onMouseDown={(event) => event.stopPropagation()}
        >
          {selected ? "✓" : ""}
        </button>
      </span>
    ),
    name: (
      <span key="name" role="gridcell" className={styles.name} title={t.name}>
        {t.name}
      </span>
    ),
    size: (
      <span key="size" role="gridcell" className={styles.num}>
        {formatBytes(t.size)}
      </span>
    ),
    done: (
      <span key="done" role="gridcell" className={styles.doneCell}>
        <ProgressBar percent={t.percent} status={t.status} />
      </span>
    ),
    status: (
      <span
        key="status"
        role="gridcell"
        className={`${styles.statusCell} ${TONE_CLASS[word.tone] ?? ""}`}
        title={t.statusMsg || undefined}
      >
        {word.text}
      </span>
    ),
    seedsPeers: (
      <span key="seedsPeers" role="gridcell" className={styles.num}>
        {seedsPeersCell(t)}
      </span>
    ),
    down: (
      <span
        key="down"
        role="gridcell"
        className={`${styles.num} ${t.downRate > 0 ? styles.down : styles.idleNum}`}
      >
        {formatDownCell(t.downRate, t.status)}
      </span>
    ),
    up: (
      <span
        key="up"
        role="gridcell"
        className={`${styles.num} ${t.upRate > 0 ? styles.up : styles.idleNum}`}
      >
        {formatUpCell(t.upRate)}
      </span>
    ),
    eta: (
      <span key="eta" role="gridcell" className={styles.num}>
        {formatEta(t.etaSeconds, t.status)}
      </span>
    ),
    ratio: (
      <span
        key="ratio"
        role="gridcell"
        className={`${styles.num} ${
          ratioTone(t.ratio) === "good" ? styles.ratioGood : styles.ratioPoor
        }`}
      >
        {formatRatio(t.ratio)}
      </span>
    ),
    label: (
      <span
        key="label"
        role="gridcell"
        className={styles.labelCell}
        title={
          t.tags && t.tags.length > 0
            ? `${t.label || "unlabeled"} · ${t.tags.join(", ")}`
            : t.label || undefined
        }
      >
        {t.label ? (
          <span
            className={styles.chip}
            style={
              chip.background
                ? { background: chip.background, color: chip.color }
                : undefined
            }
            data-label={chip.text}
          >
            {chip.text}
          </span>
        ) : (
          <span className={styles.chip}>{chip.text}</span>
        )}
        {(t.tags ?? []).map((tag) => {
          const tagStyle = tagChip(tag);
          return (
            <span
              key={tag}
              className={`${styles.chip} ${styles.tagChip}`}
              style={
                tagStyle.background
                  ? { background: tagStyle.background, color: tagStyle.color }
                  : undefined
              }
            >
              {tag}
            </span>
          );
        })}
      </span>
    ),
    added: (
      <span
        key="added"
        role="gridcell"
        className={`${styles.num} ${styles.faint}`}
      >
        {formatAdded(t.addedAt ?? 0)}
      </span>
    ),
    tracker: (
      <span
        key="tracker"
        role="gridcell"
        className={styles.faint}
        title={t.trackerHost}
      >
        {t.trackerHost || "—"}
      </span>
    ),
    started: (
      <span
        key="started"
        role="gridcell"
        className={`${styles.num} ${styles.faint}`}
      >
        {t.startedAt ? formatAdded(t.startedAt) : "—"}
      </span>
    ),
    finished: (
      <span
        key="finished"
        role="gridcell"
        className={`${styles.num} ${styles.faint}`}
      >
        {t.finishedAt ? formatAdded(t.finishedAt) : "—"}
      </span>
    ),
  };

  return (
    <div
      id={`torrent-row-${t.hash}`}
      className={cls}
      data-hash={t.hash}
      data-entering={entering ? "" : undefined}
      role="row"
      aria-selected={selected}
      aria-rowindex={rowIndex}
      onMouseDown={(e) => onMouseDown(t.hash, e)}
      onContextMenu={(e) => onContextMenu(t.hash, e)}
    >
      {visibleColumnIds.map((id) => cells[id])}
    </div>
  );
}

export const Row = memo(RowInner);
