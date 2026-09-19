/**
 * Typed wrappers around Tauri's `invoke` for every frontend → Rust command.
 *
 * The command *names* (first arg to `invoke`) must match the `#[tauri::command]`
 * function names registered in `src-tauri/src/lib.rs`. Argument objects are
 * serialized to the Rust command parameters (camelCase). Keeping every call in
 * this one module means the IPC surface is auditable in a single place.
 */

import { backend } from "./backend";
import type {
  AddOptions,
  CreateTorrentParams,
  CreateTorrentResult,
  DaemonHealth,
  DetailTab,
  FeedItem,
  LogEntry,
  MoveStatus,
  RssRule,
  Settings,
  Snapshot,
  Statistics,
  TorrentMeta,
  Transport,
  TuningPreview,
  TuningResult,
} from "./types";

/**
 * Source for an add request.
 *
 * - `file` — desktop path (Tauri picker / Finder open / native drop).
 * - `upload` — browser `File` (web picker / DOM drop); never serialized over IPC.
 * - `magnet` — magnet URI or http(s) `.torrent` URL.
 */
export type AddSource =
  | { kind: "file"; path: string }
  | { kind: "upload"; file: File }
  | { kind: "magnet"; uri: string };

/** Parse a .torrent file's metadata to populate the Add dialog. */
export function readTorrentMetadata(path: string): Promise<TorrentMeta> {
  return backend().invoke("read_torrent_metadata", { path });
}

/** Create a new .torrent file from local source files/directories. */
export function createTorrent(
  params: CreateTorrentParams,
): Promise<CreateTorrentResult> {
  return backend().invoke("create_torrent", { params });
}

/** Drain file/deep-link requests that arrived before the webview was ready. */
export function takeOpenRequests(): Promise<string[]> {
  return backend().invoke("take_open_requests");
}

/** Add a torrent (file or magnet) with the given options. */
export function addTorrent(source: AddSource, opts: AddOptions): Promise<void> {
  return backend().invoke("add_torrent", { source, opts });
}

export function start(hashes: string[]): Promise<void> {
  return backend().invoke("start", { hashes });
}

export function stop(hashes: string[]): Promise<void> {
  return backend().invoke("stop", { hashes });
}

/** Stop transferring but stay loaded (rtorrent's `d.pause`), as opposed to
 * `stop`, which closes the torrent. */
export function pause(hashes: string[]): Promise<void> {
  return backend().invoke("pause", { hashes });
}

export function recheck(hashes: string[]): Promise<void> {
  return backend().invoke("recheck", { hashes });
}

/** Ask selected torrents to announce to their trackers immediately. */
export function forceReannounce(hashes: string[]): Promise<void> {
  return backend().invoke("force_reannounce", { hashes });
}

/** Append an announce URL to one torrent's tracker list. */
export function addTracker(hash: string, url: string): Promise<void> {
  return backend().invoke("add_tracker", { hash, url });
}

/** Remove a tracker, or disable it when the daemon lacks true removal. */
export function removeTracker(
  hash: string,
  trackerIndex: number,
): Promise<void> {
  return backend().invoke("remove_tracker", { hash, trackerIndex });
}

/** Enable or disable one tracker by its list index. */
export function setTrackerEnabled(
  hash: string,
  trackerIndex: number,
  enabled: boolean,
): Promise<void> {
  return backend().invoke("set_tracker_enabled", {
    hash,
    trackerIndex,
    enabled,
  });
}

/** Remove torrents; when `deleteData`, their files are moved to the Trash. */
export function remove(hashes: string[], deleteData: boolean): Promise<void> {
  return backend().invoke("remove", { hashes, deleteData });
}

export function setLabel(hashes: string[], label: string): Promise<void> {
  return backend().invoke("set_label", { hashes, label });
}

/** Replace the tags on each hash (V3-10). An empty list clears them. */
export function setTags(hashes: string[], tags: string[]): Promise<void> {
  return backend().invoke("set_tags", { hashes, tags });
}

/** Add tags to each hash, keeping the ones it already has (V3-10). */
export function addTags(hashes: string[], tags: string[]): Promise<void> {
  return backend().invoke("add_tags", { hashes, tags });
}

/** Remove tags from each hash (V3-10). */
export function removeTags(hashes: string[], tags: string[]): Promise<void> {
  return backend().invoke("remove_tags", { hashes, tags });
}

/** Apply a per-torrent named throttle (KiB/s); two zeroes clear it. */
export function setTorrentLimits(
  hashes: string[],
  downKb: number,
  upKb: number,
): Promise<void> {
  return backend().invoke("set_torrent_limits", { hashes, downKb, upKb });
}

export function setLocation(
  hash: string,
  path: string,
  moveData: boolean = true,
): Promise<void> {
  return backend().invoke("set_location", { hash, path, moveData });
}

/** Reorder via (priority, queue_pos) swaps (QUE-02): bands, never slots. */
export function queueMove(
  hashes: string[],
  direction: "top" | "up" | "down" | "bottom",
): Promise<void> {
  return backend().invoke("queue_move", { hashes, direction });
}

/** Toggle force-start (QUE-01): exempt from the client queue scheduler. */
export function toggleForceStart(hashes: string[]): Promise<void> {
  return backend().invoke("toggle_force_start", { hashes });
}

/** Build a magnet URI for the torrent and return it (frontend copies it). */
export function copyMagnet(hash: string): Promise<string> {
  return backend().invoke("copy_magnet", { hash });
}

/** Reveal the torrent's data in Finder (localhost daemons only). */
export function openDestination(hash: string): Promise<void> {
  return backend().invoke("open_destination", { hash });
}

/** Ban a peer and drop the connection (B16). */
export function banPeer(hash: string, peerId: string): Promise<void> {
  return backend().invoke("ban_peer", { hash, peerId });
}

/** Snub a peer — stop uploading to it, without disconnecting (B16). */
export function snubPeer(hash: string, peerId: string): Promise<void> {
  return backend().invoke("snub_peer", { hash, peerId });
}

/** Disconnect a peer now, without banning it (B16). */
export function disconnectPeer(hash: string, peerId: string): Promise<void> {
  return backend().invoke("disconnect_peer", { hash, peerId });
}

/** Change a single file's download priority (0 off / 1 normal / 2 high). */
export function setFilePriority(
  hash: string,
  fileIndex: number,
  priority: number,
): Promise<void> {
  return backend().invoke("set_file_priority", { hash, fileIndex, priority });
}

/** Set per-torrent peer/upload connection caps; zero means daemon default. */
export function setConnectionLimits(
  hash: string,
  peersMax: number,
  peersMin: number,
  uploadsMax: number,
): Promise<void> {
  return backend().invoke("set_connection_limits", {
    hash,
    peersMax,
    peersMin,
    uploadsMax,
  });
}

/** Toggle rtorrent's initial-seed connection type (D5). */
export function setSuperSeeding(hash: string, enabled: boolean): Promise<void> {
  return backend().invoke("set_super_seeding", { hash, enabled });
}

export function getSettings(): Promise<Settings> {
  return backend().invoke("get_settings");
}

export function applySettings(patch: Partial<Settings>): Promise<Settings> {
  return backend().invoke("apply_settings", { patch });
}

/** Flip turtle mode's manual switch (B14); resolves with the saved settings. */
export function setTurtle(enabled: boolean): Promise<Settings> {
  return backend().invoke("set_turtle", { enabled });
}

/** Probe a candidate connection; resolves with the daemon version or rejects. */
export function testConnection(
  transport: Transport,
  password?: string,
): Promise<string> {
  // A password typed but not yet saved must be what gets probed; omitting it
  // falls back to the Keychain.
  return backend().invoke("test_connection", { transport, password });
}

/** Save a remote daemon password to the Keychain (never to settings.json). */
export function setHttpPassword(
  url: string,
  username: string,
  password: string,
): Promise<void> {
  return backend().invoke("set_http_password", { url, username, password });
}

/** Is a password saved for this endpoint? The secret itself is never returned. */
export function hasHttpPassword(
  url: string,
  username: string,
): Promise<boolean> {
  return backend().invoke("has_http_password", { url, username });
}

export function clearHttpPassword(
  url: string,
  username: string,
): Promise<void> {
  return backend().invoke("clear_http_password", { url, username });
}

/** Steer the detail poll: which torrent + tab to watch (null to stop). */
export function setDetailWatch(
  hash: string | null,
  tab: DetailTab | null,
): Promise<void> {
  return backend().invoke("set_detail_watch", { hash, tab });
}

export function getStatistics(): Promise<Statistics> {
  return backend().invoke("get_statistics");
}

/** Daemon self-report for the Statistics dialog's Daemon tab (D16). */
export function daemonHealth(): Promise<DaemonHealth> {
  return backend().invoke("daemon_health");
}

/** Ask the daemon to write its session now (D13). */
export function saveSession(): Promise<void> {
  return backend().invoke("save_session");
}

/** Start a local rtorrent daemon (C20). Rejects for a remote transport. */
export function startDaemon(): Promise<string> {
  return backend().invoke("start_daemon");
}

/** Ask the daemon to shut down cleanly (D13). */
export function shutdownDaemon(): Promise<void> {
  return backend().invoke("shutdown_daemon");
}

/** Fetch and parse an RSS/Atom feed for the preview (B11). */
export function rssFetch(url: string): Promise<FeedItem[]> {
  return backend().invoke("rss_fetch", { url });
}

/** Manually add one feed item to rtorrent (B11). */
export function rssDownload(
  link: string,
  label: string,
  savePath: string,
): Promise<void> {
  return backend().invoke("rss_download", { link, label, savePath });
}

/** One explained preview row (V3-23). */
export interface RssTestClause {
  label: string;
  passed: boolean;
  detail: string;
}

export interface RssTestRow {
  title: string;
  link: string;
  guid: string;
  size: number | null;
  matched: boolean;
  clauses: RssTestClause[];
}

/** Test one rule against a live feed, with per-clause verdicts (V3-23). */
export function rssTest(
  rule: RssRule,
  url: string,
): Promise<RssTestRow[]> {
  return backend().invoke("rss_test", { rule, url });
}

/** Export the seen-set as JSON (V3-23). */
export function rssExportSeen(): Promise<string> {
  return backend().invoke("rss_export_seen");
}

/** Import guids into the seen-set; resolves with how many were new (V3-23). */
export function rssImportSeen(json: string): Promise<number> {
  return backend().invoke("rss_import_seen", { json });
}

/** Hydrate the Log tab with the current ring-buffer contents. */
export function getLog(): Promise<LogEntry[]> {
  return backend().invoke("get_log");
}

/** Live move-on-complete statuses with byte progress (V3-14). */
export function getMoves(): Promise<MoveStatus[]> {
  return backend().invoke("get_moves");
}

/** Cancel a running move by op id (or torrent hash); the torrent resumes in place. */
export function cancelMove(id: string): Promise<boolean> {
  return backend().invoke("cancel_move", { id });
}

/** Drop a failed/cancelled entry so the next tick re-plans from daemon truth. */
export function retryMove(hash: string): Promise<boolean> {
  return backend().invoke("retry_move", { hash });
}

/** Session manifest text for download (V3-22 / LIB-09). */
export function exportSessionText(): Promise<string> {
  return backend().invoke("export_session_text");
}

/** Counts returned by a desktop file export (V3-22 / LIB-09). */
export interface SessionExportReport {
  torrents: number;
  withSources: number;
}

/** Export the session manifest to a file (desktop only; web downloads text). */
export function exportSession(path: string): Promise<SessionExportReport> {
  return backend().invoke("export_session", { path });
}

/** One validation finding in the import preview (V3-22). */
export interface SessionProblem {
  index: number | null;
  hash: string;
  message: string;
}

/** One planned restore entry in the import preview (V3-22). */
export interface SessionPlanItem {
  hash: string;
  name: string;
  /** `add` | `have` | `skip` | `invalid`. */
  action: string;
  dstDir: string;
  reason: string;
}

/** Dry-run validation + restore preview for a manifest (V3-22). */
export interface SessionValidation {
  torrentCount: number;
  restorableCount: number;
  errors: SessionProblem[];
  warnings: SessionProblem[];
  items: SessionPlanItem[];
}

export interface SessionImportArgs {
  /** Desktop file path (native picker). Exactly one of path/text. */
  path?: string;
  /** Manifest text (web upload, paste, foreign-scan result). */
  manifestText?: string;
  /** Hashes to import; omitted means everything restorable. */
  selected?: string[];
  remapFrom?: string;
  remapTo?: string;
  resume?: boolean;
}

/** Dry-run validation + restore preview (V3-22). */
export function validateSession(args: SessionImportArgs): Promise<SessionValidation> {
  return backend().invoke("validate_session", args as Record<string, unknown>);
}

/** Live import status for the dialog to poll (V3-22). */
export interface SessionImportStatus {
  running: boolean;
  added: number;
  resumed: number;
  skipped: number;
  failed: string[];
  done: boolean;
}

/** Start a detached, journalised import; poll `importStatus` (V3-22). */
export function importSession(args: SessionImportArgs): Promise<void> {
  return backend().invoke("import_session", args as Record<string, unknown>);
}

/** Live import status for the dialog to poll (V3-22). */
export function importStatus(): Promise<SessionImportStatus> {
  return backend().invoke("import_status");
}

/** Cancel a running import after the current torrent finishes (V3-22). */
export function cancelImport(): Promise<void> {
  return backend().invoke("cancel_import");
}

/** One scanned foreign entry for the dialog preview (V3-22 / LIB-10). */
export interface ForeignScanEntry {
  hash: string;
  name: string;
  hasSource: boolean;
  stopped: boolean;
}

/** Read-only discovery of another client's resume data (V3-22 / LIB-10). */
export interface ForeignScanReport {
  client: string;
  manifestText: string;
  entryCount: number;
  restorableCount: number;
  entries: ForeignScanEntry[];
  problems: { file: string; message: string }[];
}

/** Scan qBittorrent (`BT_backup`) or Transmission resume data (V3-22 / LIB-10). */
export function scanForeign(
  client: "qbittorrent" | "transmission",
  dir: string,
): Promise<ForeignScanReport> {
  return backend().invoke("scan_foreign", { client, dir });
}

/** Ask the poller to attempt a reconnect immediately. */
export function retryConnection(): Promise<void> {
  return backend().invoke("retry_connection");
}

/** Fetch the current full snapshot (FND-02 delta heal). */
export function getSnapshot(): Promise<Snapshot | null> {
  return backend().invoke("get_snapshot");
}

/** Preview the 1 Gbps tuning block and where it would be written. */
export function tuningPreview(): Promise<TuningPreview> {
  return backend().invoke("tuning_preview");
}

/** Write the 1 Gbps tuning to .rtorrent.rc and apply it to the running daemon. */
export function applyTuning(): Promise<TuningResult> {
  return backend().invoke("apply_tuning");
}
