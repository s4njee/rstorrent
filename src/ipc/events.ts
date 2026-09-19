/**
 * Typed wrappers around the server → frontend event channel. Each helper
 * subscribes through the active backend and returns its `UnlistenFn` so
 * callers (usually store initializers) can tear the subscription down.
 *
 * Event names are namespaced with `channel://` to keep them grouped; the web
 * backend maps each name onto its polling loop.
 */

import { backend, type UnlistenFn } from "./backend";
import type { Snapshot, SnapshotDelta, DetailPayload, LogEntry } from "./types";

/** Full app state, emitted periodically and on reconnect (~1s or on delta miss). */
export function onSnapshot(cb: (s: Snapshot) => void): Promise<UnlistenFn> {
  return backend().listen<Snapshot>("state://snapshot", cb);
}

/** Incremental delta since `baseRevision`; applied via `applyDelta` (FND-02). */
export function onDelta(cb: (d: SnapshotDelta) => void): Promise<UnlistenFn> {
  return backend().listen<SnapshotDelta>("state://delta", cb);
}

/** Detail-tab data for the selected torrent, emitted ~2s while a tab is open. */
export function onDetail(cb: (d: DetailPayload) => void): Promise<UnlistenFn> {
  return backend().listen<DetailPayload>("state://detail", cb);
}

/** A single appended log line. */
export function onLog(cb: (l: LogEntry) => void): Promise<UnlistenFn> {
  return backend().listen<LogEntry>("log://append", cb);
}

/** Move-on-complete statuses changed — a nudge; refetch via `getMoves`. */
export function onMoves(cb: () => void): Promise<UnlistenFn> {
  return backend().listen<void>("moves://update", cb);
}
