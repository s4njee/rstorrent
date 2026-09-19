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
  SnapshotDelta,
  Status,
  TorrentDto,
} from "../ipc/types";

const EMPTY_GLOBALS: GlobalStats = {
  downRate: 0,
  upRate: 0,
  downRateLimit: 0,
  upRateLimit: 0,
  dhtNodes: 0,
  freeSpace: null,
  diskSize: null,
  turtleActive: false,
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
  /** True once any snapshot or delta has arrived. The table shows its
   *  placeholder rows until then, rather than an empty pane. */
  hasLoaded: boolean;
  revision: number;
  /** Optimistic transport status overrides, applied until the daemon agrees or
   *  the entry ages out (WC9-S2). Keyed by hash. */
  optimistic: Map<string, OptimisticOverride>;
  /** Replace state from a poll snapshot, reusing unchanged row objects. */
  applySnapshot: (s: Snapshot) => void;
  /**
   * Apply an incremental delta. Returns true if applied, false if the delta's
   * baseRevision does not match the current revision (caller must re-sync via
   * a full snapshot). Mirrors the Rust delta::apply logic (FND-02).
   */
  applyDelta: (d: SnapshotDelta) => boolean;
  /** Show `updates` immediately, before the daemon confirms them. */
  markOptimistic: (updates: Record<string, Status>) => void;
  /** Drop overrides (an RPC failed, so the daemon's truth stands). */
  clearOptimistic: (hashes: string[]) => void;
}

/** An optimistic status and when it was set, so a stale one can age out. */
interface OptimisticOverride {
  status: Status;
  at: number;
}

/** Past this, an override is dropped even if the daemon never agreed. */
const OPTIMISTIC_TTL_MS = 5000;

/**
 * Layer optimistic statuses over the daemon's rows. An override clears as soon
 * as the daemon reports the expected status (the action took), the row is gone,
 * or it ages out — so the table never shows a state the daemon did not report
 * for longer than the round trip plus the TTL.
 */
function applyOptimistic(
  rows: TorrentDto[],
  overrides: Map<string, OptimisticOverride>,
): TorrentDto[] {
  if (overrides.size === 0) return rows;
  const now = Date.now();
  const byHash = new Map(rows.map((row) => [row.hash, row]));
  for (const [hash, override] of overrides) {
    const row = byHash.get(hash);
    if (
      !row ||
      row.status === override.status ||
      now - override.at > OPTIMISTIC_TTL_MS
    ) {
      overrides.delete(hash);
    }
  }
  if (overrides.size === 0) return rows;
  return rows.map((row) => {
    const override = overrides.get(row.hash);
    return override ? { ...row, status: override.status } : row;
  });
}

/** Shallow field equality for a torrent row (all fields are primitives). */
function sameTorrent(a: TorrentDto, b: TorrentDto): boolean {
  return (
    a.bytesDone === b.bytesDone &&
    a.percent === b.percent &&
    a.status === b.status &&
    a.statusMsg === b.statusMsg &&
    (a.errorKind ?? "") === (b.errorKind ?? "") &&
    a.downRate === b.downRate &&
    a.upRate === b.upRate &&
    a.seedsConnected === b.seedsConnected &&
    a.peersConnected === b.peersConnected &&
    a.seedsSwarm === b.seedsSwarm &&
    a.peersSwarm === b.peersSwarm &&
    a.etaSeconds === b.etaSeconds &&
    a.ratio === b.ratio &&
    a.label === b.label &&
    (a.tags ?? []).join(",") === (b.tags ?? []).join(",") &&
    a.trackerHost === b.trackerHost &&
    a.priority === b.priority &&
    a.name === b.name &&
    a.throttleName === b.throttleName &&
    a.downRateLimit === b.downRateLimit &&
    a.upRateLimit === b.upRateLimit &&
    a.startedAt === b.startedAt &&
    a.finishedAt === b.finishedAt
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
  hasLoaded: false,
  revision: 0,
  optimistic: new Map(),
  applySnapshot: (s) =>
    set((state) => ({
      torrents: applyOptimistic(
        reconcile(state.torrents, smoothRates(emaState, s.torrents)),
        state.optimistic,
      ),
      globals: s.globals,
      connection: s.connection,
      hasLoaded: true,
      revision: s.revision ?? 0,
    })),
  applyDelta: (d) => {
    const cur = get().revision ?? 0;
    if (d.baseRevision !== cur) return false;
    const prev = get().torrents;
    const removed = new Set(d.removed);
    const updatedMap = new Map(d.updated.map((t) => [t.hash, t]));
    const next: TorrentDto[] = [];
    for (const t of prev) {
      if (removed.has(t.hash)) continue;
      const u = updatedMap.get(t.hash);
      if (u) next.push(u);
      else next.push(t);
    }
    for (const t of d.added) next.push(t);
    // Deterministic order to keep ETag stable; UI re-sorts via selectors.
    next.sort((a, b) => a.hash.localeCompare(b.hash));
    set((state) => ({
      torrents: applyOptimistic(
        reconcile(state.torrents, smoothRates(emaState, next)),
        state.optimistic,
      ),
      globals: d.globals,
      connection: d.connection,
      revision: d.revision,
    }));
    return true;
  },
  markOptimistic: (updates) =>
    set((state) => {
      const now = Date.now();
      for (const [hash, status] of Object.entries(updates)) {
        state.optimistic.set(hash, { status, at: now });
      }
      // Nudge subscribers: apply the override immediately against the rows we
      // already hold, so the row changes without waiting for the next poll.
      return {
        torrents: applyOptimistic(state.torrents, state.optimistic),
      };
    }),
  clearOptimistic: (hashes) =>
    set((state) => {
      let changed = false;
      for (const hash of hashes)
        changed = state.optimistic.delete(hash) || changed;
      if (!changed) return {};
      return { torrents: [...state.torrents] };
    }),
}));
