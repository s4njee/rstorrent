/**
 * Web status-cell label (WE2-S7).
 *
 * The web prototype shows error rows as a short lowercase phrase in the Status
 * column — `trk error` for a tracker problem, `disk error` for storage, `error`
 * otherwise — derived from the daemon's `statusMsg`, rather than the bare word
 * "error". Every other status is just its lowercase name.
 *
 * Pure and shell-agnostic; the desktop keeps its own rendering.
 */

import type { Status } from "../ipc/types";

/** Keywords that mark a storage/disk failure in rtorrent's `d.message`. */
const DISK_HINTS = [
  "storage",
  "disk",
  "no space",
  "permission",
  "directory",
  "file",
];

/** The Status-column text for a torrent. */
export function webStatusLabel(status: Status, statusMsg: string): string {
  if (status !== "error") return status;
  const msg = statusMsg.toLowerCase();
  if (DISK_HINTS.some((h) => msg.includes(h))) return "disk error";
  // Tracker problems are by far the common case; treat them as the default
  // error kind unless the message clearly points at storage.
  return "trk error";
}
