/**
 * IPC contract — the single source of truth for every value that crosses the
 * Tauri boundary between the Rust backend and this React frontend.
 *
 * The Rust side mirrors these shapes with `#[derive(Serialize/Deserialize)]`
 * structs using `#[serde(rename_all = "camelCase")]`, so field names here and in
 * `src-tauri/src/ipc.rs` must stay identical. When you change a type here,
 * change its Rust twin in the same commit.
 */

/** Lifecycle status of a torrent, derived in Rust from rtorrent's raw flags. */
export type Status =
  | "downloading"
  | "seeding"
  | "completed"
  | "paused"
  | "stalled"
  | "checking"
  | "error";

/**
 * One torrent as shown in the table. Rates are bytes/second, sizes are bytes;
 * all formatting happens in the frontend (`utils/format.ts`).
 */
export interface TorrentDto {
  /** Uppercase hex info-hash; stable primary key for a torrent. */
  hash: string;
  name: string;
  /** Total size in bytes. */
  size: number;
  /** Bytes downloaded and verified so far. */
  bytesDone: number;
  /** 0..100 completion percentage. */
  percent: number;
  status: Status;
  /** rtorrent's `d.message` (tracker/storage error text), empty when none. */
  statusMsg: string;
  /** Peers we're connected to that have the complete file (seeds). */
  seedsConnected: number;
  /** Total peers we're connected to (incl. seeds). */
  peersConnected: number;
  /** Swarm seed count (from tracker scrape), for the "S" column tooltip. */
  seedsSwarm: number;
  /** Swarm peer count (from tracker scrape). */
  peersSwarm: number;
  downRate: number;
  upRate: number;
  /** Seconds remaining, or null for ∞ (stalled/seeding) / — (paused/done). */
  etaSeconds: number | null;
  ratio: number;
  label: string;
  /** Primary tracker hostname (from the slow poll), empty until resolved. */
  trackerHost: string;
  savePath: string;
  /** rtorrent priority 0..3, reused as the queue-order approximation. */
  priority: number;
  isPrivate: boolean;
  /** App-owned named throttle, empty when global limits apply. */
  throttleName: string;
  /** Named limits in bytes/s; null means inherit the corresponding global. */
  downRateLimit: number | null;
  upRateLimit: number | null;
}

/** Global counters shown in the status bar and General detail tab. */
export interface GlobalStats {
  downRate: number;
  upRate: number;
  downRateLimit: number; // bytes/s, 0 = unlimited (∞)
  upRateLimit: number;
  dhtNodes: number;
  /** Free bytes on the default save-path volume, or null if unknown/remote. */
  freeSpace: number | null;
}

/** Connection lifecycle to the rtorrent daemon. */
export type ConnPhase = "connecting" | "connected" | "disconnected";

export interface ConnState {
  phase: ConnPhase;
  /** Human-readable endpoint, e.g. "unix:/…/rpc.socket" or "tcp:127.0.0.1:5000". */
  endpoint: string;
  /** rtorrent version string when connected (e.g. "0.9.8"). */
  daemonVersion: string | null;
  /** Last error message when disconnected. */
  error: string | null;
  /** Seconds until the next reconnect attempt, when disconnected. */
  retryInSeconds: number | null;
}

/** The full state pushed on every fast poll via the `state://snapshot` event. */
export interface Snapshot {
  torrents: TorrentDto[];
  globals: GlobalStats;
  connection: ConnState;
}

/** Which detail tab is being watched, driving the 2s detail poll. */
export type DetailTab =
  "general" | "trackers" | "peers" | "content" | "speed" | "log";

/** A tracker row for the Trackers detail tab. */
export interface TrackerRow {
  /** Zero-based position in rtorrent's tracker list (`HASH:tINDEX`). */
  index: number;
  url: string;
  enabled: boolean;
  status: string; // "working" | "updating" | "disabled" | "error" | ...
  seeds: number;
  leeches: number;
  lastAnnounce: string;
}

/** A peer row for the Peers detail tab. */
export interface PeerRow {
  address: string;
  client: string;
  progress: number; // 0..100
  downRate: number;
  upRate: number;
  flags: string; // e.g. "EI" (encrypted, incoming)
}

/** A file/folder node for the Content tab and Add-torrent file tree. */
export interface FileNode {
  path: string;
  size: number;
  /** 0 = don't download, 1 = normal, 2 = high. */
  priority: number;
  /** 0..100, only meaningful in the Content tab (0 in the Add dialog). */
  progress: number;
  isDir: boolean;
}

/**
 * Per-piece download state for the General tab's pieces bar.
 *
 * `bitfield` is rtorrent's `d.bitfield` hex string: each byte (two hex chars)
 * covers 8 pieces, most-significant bit first. It's forwarded verbatim (rather
 * than as a bool array) to keep the payload small for large torrents; use
 * `utils/bitfield.ts` to read it.
 */
export interface PieceInfo {
  sizeChunks: number;
  completedChunks: number;
  chunkSize: number;
  bitfield: string;
}

/** Payload of the `state://detail` event. */
export interface DetailPayload {
  hash: string;
  tab: DetailTab;
  trackers?: TrackerRow[];
  peers?: PeerRow[];
  files?: FileNode[];
  pieces?: PieceInfo;
}

/** One entry in the app event log (Log tab). */
export interface LogEntry {
  /** Epoch milliseconds. */
  time: number;
  level: "info" | "warn" | "error";
  message: string;
  /** Info-hash this entry relates to, if any (for selection highlighting). */
  hash: string | null;
}

/** Metadata parsed from a .torrent file for the Add dialog. */
export interface TorrentMeta {
  name: string;
  size: number;
  infoHash: string;
  isPrivate: boolean;
  files: FileNode[];
  trackers: string[];
}

/** Options passed with an add request. */
export interface AddOptions {
  savePath: string;
  label: string;
  start: boolean;
  topOfQueue: boolean;
  sequential: boolean;
  skipHashCheck: boolean;
  /** Indexes of files to NOT download (priority 0), by their order in `files`. */
  unselectedIndexes: number[];
}

/** Connection transport settings, persisted on the Rust side. */
export type Transport =
  | { kind: "unixSocket"; path: string }
  | { kind: "tcp"; host: string; port: number };

/** Ratio/time limits for completed torrents. Zero disables either rule. */
export interface SeedGoal {
  stopRatio: number;
  seedHours: number;
}

/** A label-specific replacement for the global seed goal. */
export interface LabelSeedGoal extends SeedGoal {
  label: string;
}

/** App settings shared with the frontend Preferences UI. */
export interface Settings {
  transport: Transport;
  pollMs: number;
  stallWindowS: number;
  defaultSavePath: string;
  showAddDialog: boolean;
  confirmOnRemove: boolean;
  downLimitKb: number; // 0 = unlimited
  upLimitKb: number;
  portRange: string; // e.g. "6881-6899"
  dhtEnabled: boolean;
  watchFolder: string; // auto-add .torrent files from here; empty = disabled
  /** Labels whose completed torrents should not produce a notification. */
  completionNotificationExcludedLabels: string[];
  /** App-owned throttle definitions replayed after daemon restarts. */
  torrentThrottles: NamedThrottle[];
  globalSeedGoal: SeedGoal;
  labelSeedGoals: LabelSeedGoal[];
  mock: boolean;
}

export interface NamedThrottle {
  name: string;
  downKb: number;
  upKb: number;
}

/** Aggregate figures for the Statistics dialog; nulls render as "—". */
export interface Statistics {
  sessionDown: number;
  sessionUp: number;
  allTimeDown: number;
  allTimeUp: number;
  allTimeRatio: number | null;
  sessionWaste: number;
  connectedPeers: number;
  cacheHitPct: number | null;
  bufferSize: number | null;
  cacheOverloadPct: number | null;
  queuedIo: number | null;
}
