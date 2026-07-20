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
  /** Unix seconds first started / finished, 0 when unknown. Drive the Started
   *  and Finished columns; durable across daemon restarts. */
  startedAt: number;
  finishedAt: number;
  /** Native rtorrent views this torrent belongs to (D12); empty until resolved. */
  views: string[];
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
  /** Whether turtle mode is currently in effect (manual or scheduled) (B14). */
  turtleActive: boolean;
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
  /** Tracker protocol: "http" / "udp" / "dht", empty if unknown. */
  kind: string;
  /** Unix seconds of the next scheduled announce; 0 or past → shown as —. */
  nextAnnounce: number;
  /** Unix seconds of the last successful announce; 0 = never. */
  lastAnnounce: number;
}

/** A peer row for the Peers detail tab. */
export interface PeerRow {
  /** rtorrent peer id (hex), used to target actions; not displayed. */
  id: string;
  address: string;
  client: string;
  progress: number; // 0..100
  downRate: number;
  upRate: number;
  /** Compact flags (C16): E encrypted · I incoming · O obfuscated · P preferred · U unwanted. */
  flags: string;
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
  | { kind: "tcp"; host: string; port: number }
  /**
   * XML-RPC over HTTP(S) for a remote daemon behind nginx (B9). No password
   * field: it lives in the macOS Keychain, never in the settings file. Use the
   * `*HttpPassword` commands to manage it.
   */
  | { kind: "http"; url: string; username: string };

/** Ratio/time limits for completed torrents. Zero disables either rule. */
export interface SeedGoal {
  stopRatio: number;
  seedHours: number;
}

/** A label-specific replacement for the global seed goal. */
export interface LabelSeedGoal extends SeedGoal {
  label: string;
}

/** What to do when a torrent reaches its seed goal (C14). */
export type SeedGoalAction = "stop" | "remove" | "removeData";

/** A per-label default save path (C11). */
export interface LabelDefault {
  label: string;
  savePath: string;
}

/** One watched folder (C12); empty label/savePath fall back to defaults. */
export interface WatchFolder {
  path: string;
  label: string;
  savePath: string;
}

/** Daily window that auto-engages turtle mode (B14). */
export interface TurtleSchedule {
  enabled: boolean;
  /** Window start, minutes since local midnight [0,1440). */
  startMin: number;
  /** Window end; if endMin <= startMin the window wraps past midnight. */
  endMin: number;
  /** Active weekdays, 0=Sunday..6=Saturday. Empty = every day. */
  days: number[];
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
  /** Legacy single watch folder; migrated into watchFolders on load (C12). */
  watchFolder: string;
  /** Labels whose completed torrents should not produce a notification. */
  completionNotificationExcludedLabels: string[];
  /** App-owned throttle definitions replayed after daemon restarts. */
  torrentThrottles: NamedThrottle[];
  globalSeedGoal: SeedGoal;
  labelSeedGoals: LabelSeedGoal[];

  // --- Network pane (v1.6) ---
  /** Protocol-encryption preset (D7); write-only, see EncryptionMode. */
  encryption: EncryptionMode;
  /** Peer exchange on/off (D7). */
  pexEnabled: boolean;
  /** HTTP proxy host:port for tracker announces (D8); empty = none. */
  proxyAddress: string;
  /** Route tracker HTTP requests through proxyAddress (D8). */
  proxyTrackerHttp: boolean;
  /** Bind listen/outgoing to this address, e.g. a VPN interface (D9); empty = default. */
  bindAddress: string;
  /** Address reported to trackers/peers (D9); empty = default. */
  localAddress: string;
  /** Global peer cap per torrent (D11); 0 = daemon default. */
  maxPeers: number;
  /** Global simultaneous upload slots (D11); 0 = unlimited. */
  maxUploadsGlobal: number;
  /** Global simultaneous download slots (D11); 0 = unlimited. */
  maxDownloadsGlobal: number;

  // --- Automation (v1.7) ---
  /** Keep at most this many torrents downloading; queue the rest (C9). 0 = off. */
  maxActiveDownloads: number;
  /** Per-label default save paths (C11). */
  labelDefaults: LabelDefault[];
  /** Watched folders for auto-add (C12). */
  watchFolders: WatchFolder[];
  /** Command run on completion with %N/%F/%H tokens (C13); empty = disabled. */
  runOnComplete: string;
  /** What to do when a torrent reaches its seed goal (C14). */
  seedGoalAction: SeedGoalAction;
  /** Turtle download limit, KiB/s; 0 = unlimited (B14). */
  turtleDownKb: number;
  /** Turtle upload limit, KiB/s; 0 = unlimited (B14). */
  turtleUpKb: number;
  /** Manual turtle-mode toggle (B14). */
  turtleEnabled: boolean;
  /** Optional daily schedule that auto-engages turtle mode (B14). */
  turtleSchedule: TurtleSchedule;
  /** Saved daemon connections (B10); the active one is mirrored in transport. */
  connectionProfiles: ConnectionProfile[];

  mock: boolean;
}

export interface NamedThrottle {
  name: string;
  downKb: number;
  upKb: number;
}

/**
 * Protocol-encryption preset (D7). rtorrent 0.16.17 has no getter for the
 * current mode, so this reflects the last preset rstorrent applied.
 */
export type EncryptionMode = "disabled" | "allow" | "prefer" | "require";

/** What the 1 Gbps tuner would do, for the confirmation dialog. */
export interface TuningPreview {
  /** Where the block would be written; null for a remote daemon (unreachable). */
  rcPath: string | null;
  /** The exact block that would be written to .rtorrent.rc. */
  block: string;
  /** True when the daemon is local, so the rc file can be edited. */
  canWriteFile: boolean;
}

/** The outcome of applying the 1 Gbps tuner. */
export interface TuningResult {
  rcPath: string | null;
  fileWritten: boolean;
  fileError: string | null;
  /** How many directives the running daemon accepted, out of `liveTotal`. */
  liveApplied: number;
  liveTotal: number;
  liveError: string | null;
}

/** A saved daemon connection (B10). */
export interface ConnectionProfile {
  name: string;
  transport: Transport;
}

/** What the daemon reports about itself, for the Daemon tab (D16). */
export interface DaemonHealth {
  clientVersion: string;
  apiVersion: string;
  sessionPath: string;
  memoryMax: number;
  memoryCurrent: number;
  openSockets: number;
  maxOpenSockets: number;
  maxOpenFiles: number;
  httpMaxOpen: number;
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
