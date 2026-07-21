/**
 * Typed wrappers around the host's event channel (Rust/server → frontend).
 * Each helper subscribes through the registered backend and returns its
 * `UnlistenFn` so callers (usually store initializers) can tear the
 * subscription down.
 *
 * Event names are namespaced with `channel://` to keep them grouped; they are
 * the exact strings the desktop `poller`/`log` modules emit, and the web adapter
 * recognizes the same names to drive its polling loops.
 */

import { backend, type UnlistenFn } from "./backend";
import type { Snapshot, DetailPayload, LogEntry } from "./types";

/** Full app state, emitted on every fast poll (~1s). */
export function onSnapshot(cb: (s: Snapshot) => void): Promise<UnlistenFn> {
  return backend().listen<Snapshot>("state://snapshot", cb);
}

/** Detail-tab data for the selected torrent, emitted ~2s while a tab is open. */
export function onDetail(cb: (d: DetailPayload) => void): Promise<UnlistenFn> {
  return backend().listen<DetailPayload>("state://detail", cb);
}

/** A single appended log line. */
export function onLog(cb: (l: LogEntry) => void): Promise<UnlistenFn> {
  return backend().listen<LogEntry>("log://append", cb);
}

/** A native-menu item was clicked (payload is the action id, e.g. "prefs"). */
export function onMenuAction(
  cb: (action: string) => void,
): Promise<UnlistenFn> {
  return backend().listen<string>("menu://action", cb);
}

/** Files or deep links opened after the frontend completed startup. */
export function onOpenRequests(
  cb: (urls: string[]) => void,
): Promise<UnlistenFn> {
  return backend().listen<string[]>("app://open-requests", cb);
}

/** The user clicked a native download-completion notification. */
export function onNotificationClick(
  cb: (hash: string) => void,
): Promise<UnlistenFn> {
  return backend().listen<string>("torrent://notification-clicked", cb);
}
