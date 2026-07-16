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
    a.name === b.name
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

export const useTorrents = create<TorrentsState>((set, get) => ({
  torrents: [],
  globals: EMPTY_GLOBALS,
  connection: INITIAL_CONN,
  applySnapshot: (s) =>
    set({
      torrents: reconcile(get().torrents, s.torrents),
      globals: s.globals,
      connection: s.connection,
    }),
}));
