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
  DetailTab,
  LogEntry,
  Settings,
  Statistics,
  TorrentMeta,
  Transport,
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

/** Hydrate the Log tab with the current ring-buffer contents. */
export function getLog(): Promise<LogEntry[]> {
  return invoke("get_log");
}

/** Ask the poller to attempt a reconnect immediately. */
export function retryConnection(): Promise<void> {
  return invoke("retry_connection");
}
