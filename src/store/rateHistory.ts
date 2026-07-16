/**
 * Rate-history store — a per-torrent ring buffer of recent down/up rates, fed
 * from each snapshot, that powers the Speed detail tab's chart.
 *
 * We keep history only for a bounded set of hashes (whatever has been observed
 * recently) and cap each series length, so memory stays flat regardless of
 * session length. There's no rtorrent call involved — this is derived purely
 * from the 1s snapshots the poller already sends.
 */

import { create } from "zustand";
import type { TorrentDto } from "../ipc/types";

/** One sampled point. */
export interface RatePoint {
  down: number;
  up: number;
}

/** ~10 minutes of 1s samples. */
const MAX_POINTS = 600;

interface RateHistoryState {
  /** hash → recent [down, up] samples, oldest first. */
  series: Map<string, RatePoint[]>;
  /** Append the current rates for every torrent in a snapshot. */
  record: (torrents: TorrentDto[]) => void;
  get: (hash: string) => RatePoint[];
}

export const useRateHistory = create<RateHistoryState>((set, get) => ({
  series: new Map(),
  record: (torrents) =>
    set((state) => {
      const next = new Map(state.series);
      const seen = new Set<string>();
      for (const t of torrents) {
        seen.add(t.hash);
        const prev = next.get(t.hash) ?? [];
        const appended = [...prev, { down: t.downRate, up: t.upRate }];
        next.set(t.hash, appended.slice(-MAX_POINTS));
      }
      // Drop history for torrents that no longer exist.
      for (const hash of next.keys()) {
        if (!seen.has(hash)) next.delete(hash);
      }
      return { series: next };
    }),
  get: (hash) => get().series.get(hash) ?? [],
}));
