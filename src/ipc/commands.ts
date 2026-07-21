/**
 * Typed wrappers around Tauri's `invoke` for every frontend → Rust command.
 *
 * The command *names* (first arg to `invoke`) must match the `#[tauri::command]`
 * function names registered in `src-tauri/src/lib.rs`. Argument objects are
 * serialized to the Rust command parameters (camelCase). Keeping every call in
 * this one module means the IPC surface is auditable in a single place.
 */

import { invoke } from "@tauri-apps/api/core";
import type {
  AddOptions,
  DaemonHealth,
  DetailTab,
  FeedItem,
  LogEntry,
  Settings,
  Statistics,
  TorrentMeta,
  Transport,
  TuningPreview,
  TuningResult,
} from "./types";

/** Source for an add request: a local .torrent file or a magnet/URL string. */
export type AddSource =
  { kind: "file"; path: string } | { kind: "magnet"; uri: string };

/** Parse a .torrent file's metadata to populate the Add dialog. */
export function readTorrentMetadata(path: string): Promise<TorrentMeta> {
  return invoke("read_torrent_metadata", { path });
}

/** Drain file/deep-link requests that arrived before the webview was ready. */
export function takeOpenRequests(): Promise<string[]> {
  return invoke("take_open_requests");
}

/** Add a torrent (file or magnet) with the given options. */
export function addTorrent(source: AddSource, opts: AddOptions): Promise<void> {
  return invoke("add_torrent", { source, opts });
}

export function start(hashes: string[]): Promise<void> {
  return invoke("start", { hashes });
}

export function stop(hashes: string[]): Promise<void> {
  return invoke("stop", { hashes });
}

export function recheck(hashes: string[]): Promise<void> {
  return invoke("recheck", { hashes });
}

/** Ask selected torrents to announce to their trackers immediately. */
export function forceReannounce(hashes: string[]): Promise<void> {
  return invoke("force_reannounce", { hashes });
}

/** Append an announce URL to one torrent's tracker list. */
export function addTracker(hash: string, url: string): Promise<void> {
  return invoke("add_tracker", { hash, url });
}

/** Remove a tracker, or disable it when the daemon lacks true removal. */
export function removeTracker(
  hash: string,
  trackerIndex: number,
): Promise<void> {
  return invoke("remove_tracker", { hash, trackerIndex });
}

/** Enable or disable one tracker by its list index. */
export function setTrackerEnabled(
  hash: string,
  trackerIndex: number,
  enabled: boolean,
): Promise<void> {
  return invoke("set_tracker_enabled", { hash, trackerIndex, enabled });
}

/** Remove torrents; when `deleteData`, their files are moved to the Trash. */
export function remove(hashes: string[], deleteData: boolean): Promise<void> {
  return invoke("remove", { hashes, deleteData });
}

export function setLabel(hashes: string[], label: string): Promise<void> {
  return invoke("set_label", { hashes, label });
}

/** Apply a per-torrent named throttle (KiB/s); two zeroes clear it. */
export function setTorrentLimits(
  hashes: string[],
  downKb: number,
  upKb: number,
): Promise<void> {
  return invoke("set_torrent_limits", { hashes, downKb, upKb });
}

export function setLocation(hash: string, path: string): Promise<void> {
  return invoke("set_location", { hash, path });
}

/** Nudge queue order via rtorrent priority (see plan.md §10 caveat). */
export function queueMove(
  hashes: string[],
  direction: "up" | "down",
): Promise<void> {
  return invoke("queue_move", { hashes, direction });
}

/** Build a magnet URI for the torrent and return it (frontend copies it). */
export function copyMagnet(hash: string): Promise<string> {
  return invoke("copy_magnet", { hash });
}

/** Reveal the torrent's data in Finder (localhost daemons only). */
export function openDestination(hash: string): Promise<void> {
  return invoke("open_destination", { hash });
}

/** Ban a peer and drop the connection (B16). */
export function banPeer(hash: string, peerId: string): Promise<void> {
  return invoke("ban_peer", { hash, peerId });
}

/** Snub a peer — stop uploading to it, without disconnecting (B16). */
export function snubPeer(hash: string, peerId: string): Promise<void> {
  return invoke("snub_peer", { hash, peerId });
}

/** Disconnect a peer now, without banning it (B16). */
export function disconnectPeer(hash: string, peerId: string): Promise<void> {
  return invoke("disconnect_peer", { hash, peerId });
}

/** Change a single file's download priority (0 off / 1 normal / 2 high). */
export function setFilePriority(
  hash: string,
  fileIndex: number,
  priority: number,
): Promise<void> {
  return invoke("set_file_priority", { hash, fileIndex, priority });
}

export function getSettings(): Promise<Settings> {
  return invoke("get_settings");
}

export function applySettings(patch: Partial<Settings>): Promise<Settings> {
  return invoke("apply_settings", { patch });
}

/** Flip turtle mode's manual switch (B14); resolves with the saved settings. */
export function setTurtle(enabled: boolean): Promise<Settings> {
  return invoke("set_turtle", { enabled });
}

/** Probe a candidate connection; resolves with the daemon version or rejects. */
export function testConnection(
  transport: Transport,
  password?: string,
): Promise<string> {
  // A password typed but not yet saved must be what gets probed; omitting it
  // falls back to the Keychain.
  return invoke("test_connection", { transport, password });
}

/** Save a remote daemon password to the Keychain (never to settings.json). */
export function setHttpPassword(
  url: string,
  username: string,
  password: string,
): Promise<void> {
  return invoke("set_http_password", { url, username, password });
}

/** Is a password saved for this endpoint? The secret itself is never returned. */
export function hasHttpPassword(
  url: string,
  username: string,
): Promise<boolean> {
  return invoke("has_http_password", { url, username });
}

export function clearHttpPassword(
  url: string,
  username: string,
): Promise<void> {
  return invoke("clear_http_password", { url, username });
}

/** Steer the detail poll: which torrent + tab to watch (null to stop). */
export function setDetailWatch(
  hash: string | null,
  tab: DetailTab | null,
): Promise<void> {
  return invoke("set_detail_watch", { hash, tab });
}

export function getStatistics(): Promise<Statistics> {
  return invoke("get_statistics");
}

/** Daemon self-report for the Statistics dialog's Daemon tab (D16). */
export function daemonHealth(): Promise<DaemonHealth> {
  return invoke("daemon_health");
}

/** Ask the daemon to write its session now (D13). */
export function saveSession(): Promise<void> {
  return invoke("save_session");
}

/** Ask the daemon to shut down cleanly (D13). */
export function shutdownDaemon(): Promise<void> {
  return invoke("shutdown_daemon");
}

/** Fetch and parse an RSS/Atom feed for the preview (B11). */
export function rssFetch(url: string): Promise<FeedItem[]> {
  return invoke("rss_fetch", { url });
}

/** Manually add one feed item to rtorrent (B11). */
export function rssDownload(
  link: string,
  label: string,
  savePath: string,
): Promise<void> {
  return invoke("rss_download", { link, label, savePath });
}

/** Hydrate the Log tab with the current ring-buffer contents. */
export function getLog(): Promise<LogEntry[]> {
  return invoke("get_log");
}

/** Ask the poller to attempt a reconnect immediately. */
export function retryConnection(): Promise<void> {
  return invoke("retry_connection");
}

/** Preview the 1 Gbps tuning block and where it would be written. */
export function tuningPreview(): Promise<TuningPreview> {
  return invoke("tuning_preview");
}

/** Write the 1 Gbps tuning to .rtorrent.rc and apply it to the running daemon. */
export function applyTuning(): Promise<TuningResult> {
  return invoke("apply_tuning");
}
