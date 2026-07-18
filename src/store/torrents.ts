/**
 * Torrents store — holds the latest snapshot pushed from Rust.
 *
 * On each snapshot we *reconcile by hash*: an incoming torrent that is
 * byte-for-byte identical to the one we already hold keeps its previous object
 * identity. That lets the table's memoized `Row` skip re-rendering rows that
 * didn't change, even though a new snapshot arrives every second.
 */

import { create } from "zustand";
import type {
  ConnState,
  GlobalStats,
  Snapshot,
  TorrentDto,
} from "../ipc/types";

const EMPTY_GLOBALS: GlobalStats = {
  downRate: 0,
  upRate: 0,
  downRateLimit: 0,
  upRateLimit: 0,
  dhtNodes: 0,
  freeSpace: null,
};

const INITIAL_CONN: ConnState = {
  phase: "connecting",
  endpoint: "",
  daemonVersion: null,
  error: null,
  retryInSeconds: null,
};

interface TorrentsState {
  torrents: TorrentDto[];
  globals: GlobalStats;
  connection: ConnState;
  /** Replace state from a poll snapshot, reusing unchanged row objects. */
  applySnapshot: (s: Snapshot) => void;
}

/** Shallow field equality for a torrent row (all fields are primitives). */
function sameTorrent(a: TorrentDto, b: TorrentDto): boolean {
  return (
    a.bytesDone === b.bytesDone &&
    a.percent === b.percent &&
    a.status === b.status &&
    a.statusMsg === b.statusMsg &&
    a.downRate === b.downRate &&
    a.upRate === b.upRate &&
    a.seedsConnected === b.seedsConnected &&
    a.peersConnected === b.peersConnected &&
    a.seedsSwarm === b.seedsSwarm &&
    a.peersSwarm === b.peersSwarm &&
    a.etaSeconds === b.etaSeconds &&
    a.ratio === b.ratio &&
    a.label === b.label &&
    a.trackerHost === b.trackerHost &&
    a.priority === b.priority &&
    a.name === b.name &&
    a.throttleName === b.throttleName &&
    a.downRateLimit === b.downRateLimit &&
    a.upRateLimit === b.upRateLimit
  );
}

/** Reconcile incoming rows against the current ones, preserving identity. */
export function reconcile(
  prev: TorrentDto[],
  next: TorrentDto[],
): TorrentDto[] {
  const byHash = new Map(prev.map((t) => [t.hash, t]));
  return next.map((t) => {
    const old = byHash.get(t.hash);
    return old && sameTorrent(old, t) ? old : t;
  });
}

/** Per-hash EMA state for rate smoothing (C6). */
export type EmaState = Map<string, { down: number; up: number }>;

// ≈ an average over the last ~5 one-second samples: enough to stop the
// Down/Up/ETA columns flickering every tick, small enough to track a real
// change within a couple of seconds.
const EMA_ALPHA = 1 / 3;

/**
 * Smooth each torrent's displayed rates with an EMA, and recompute its ETA
 * from the smoothed rate so both stop jumping on every poll (C6).
 *
 * Display-only: rows are copied, never mutated, so the rate-history store —
 * which samples the same raw snapshot array — still charts real values on the
 * Speed tab. `state` is mutated (it's the caller's accumulator across
 * snapshots).
 *
 * A raw rate of zero resets the EMA instead of decaying: a stopped torrent
 * should read 0 immediately, not fade out over several seconds.
 */
export function smoothRates(state: EmaState, next: TorrentDto[]): TorrentDto[] {
  const seen = new Set<string>();
  const out = next.map((t) => {
    seen.add(t.hash);
    const prev = state.get(t.hash);
    const ema = (raw: number, old: number | undefined) =>
      raw === 0 || old === undefined
        ? raw
        : Math.round(EMA_ALPHA * raw + (1 - EMA_ALPHA) * old);
    const down = ema(t.downRate, prev?.down);
    const up = ema(t.upRate, prev?.up);
    state.set(t.hash, { down, up });

    // Only replace an ETA the backend considered real (null means ∞/—), and
    // only when there's a smoothed rate to divide by.
    const remaining = t.size - t.bytesDone;
    const etaSeconds =
      t.etaSeconds !== null && down > 0
        ? Math.round(remaining / down)
        : t.etaSeconds;

    if (down === t.downRate && up === t.upRate && etaSeconds === t.etaSeconds) {
      return t;
    }
    return { ...t, downRate: down, upRate: up, etaSeconds };
  });
  // Keep the accumulator bounded: drop state for torrents that are gone.
  for (const hash of state.keys()) {
    if (!seen.has(hash)) state.delete(hash);
  }
  return out;
}

/** Module-level EMA accumulator used by the live store. */
const emaState: EmaState = new Map();

export const useTorrents = create<TorrentsState>((set, get) => ({
  torrents: [],
  globals: EMPTY_GLOBALS,
  connection: INITIAL_CONN,
  applySnapshot: (s) =>
    set({
      torrents: reconcile(get().torrents, smoothRates(emaState, s.torrents)),
      globals: s.globals,
      connection: s.connection,
    }),
}));
