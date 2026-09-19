/**
 * Space by label (WC8-S6) — the Stats route's aggregation of torrent sizes.
 *
 * Pure: the page holds the snapshot already, so grouping it costs nothing and
 * needs no server round-trip. An empty label is a real bucket (`unlabeled`),
 * exactly as the sidebar treats it.
 */

import type { TorrentDto } from "../ipc/types";

export const UNLABELED = "unlabeled";

export interface LabelSpace {
  label: string;
  bytes: number;
  count: number;
}

/** Torrent sizes grouped by label, largest first (name breaks ties). */
export function spaceByLabel(torrents: readonly TorrentDto[]): LabelSpace[] {
  const byLabel = new Map<string, LabelSpace>();
  for (const torrent of torrents) {
    const label = torrent.label || UNLABELED;
    const entry = byLabel.get(label) ?? { label, bytes: 0, count: 0 };
    entry.bytes += torrent.size;
    entry.count += 1;
    byLabel.set(label, entry);
  }
  return [...byLabel.values()].sort(
    (a, b) => b.bytes - a.bytes || a.label.localeCompare(b.label),
  );
}

/** A bar's width as a fraction of the largest bucket, clamped to 0..1. */
export function barFraction(bytes: number, largest: number): number {
  if (largest <= 0) return 0;
  return Math.max(0, Math.min(1, bytes / largest));
}
