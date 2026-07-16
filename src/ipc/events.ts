/**
 * Typed wrappers around Tauri's event system for the Rust → frontend push
 * channels. Each helper returns the `UnlistenFn` promise from `listen` so
 * callers (usually store initializers) can tear the subscription down.
 *
 * Event names are namespaced with `channel://` to keep them grouped and are the
 * exact strings the Rust `poller`/`log` modules emit with.
 */

import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { Snapshot, DetailPayload, LogEntry } from "./types";

/** Full app state, emitted on every fast poll (~1s). */
export function onSnapshot(cb: (s: Snapshot) => void): Promise<UnlistenFn> {
  return listen<Snapshot>("state://snapshot", (e) => cb(e.payload));
}

/** Detail-tab data for the selected torrent, emitted ~2s while a tab is open. */
export function onDetail(cb: (d: DetailPayload) => void): Promise<UnlistenFn> {
  return listen<DetailPayload>("state://detail", (e) => cb(e.payload));
}

/** A single appended log line. */
export function onLog(cb: (l: LogEntry) => void): Promise<UnlistenFn> {
  return listen<LogEntry>("log://append", (e) => cb(e.payload));
}

/** A native-menu item was clicked (payload is the action id, e.g. "prefs"). */
export function onMenuAction(
  cb: (action: string) => void,
): Promise<UnlistenFn> {
  return listen<string>("menu://action", (e) => cb(e.payload));
}

/** The user clicked a native download-completion notification. */
export function onNotificationClick(
  cb: (hash: string) => void,
): Promise<UnlistenFn> {
  return listen<string>("torrent://notification-clicked", (e) => cb(e.payload));
}
