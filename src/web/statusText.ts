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

/** The Status-column text for a torrent — D19 taxonomy aware.
 *  Prefers the classified `errorKind` when present; falls back to the
 *  pre-D19 substring heuristic for older payloads / demo fixtures. */
export function webStatusLabel(
  status: Status,
  statusMsg: string,
  errorKind?: string,
): string {
  if (status !== "error") return status;
  if (errorKind) {
    switch (errorKind) {
      case "unregistered":
        return "unregistered";
      case "tracker_timeout":
        return "timeout";
      case "tracker_error":
        return "trk error";
      case "missing_files":
        return "missing files";
      case "no_space":
        return "no space";
      case "permission":
        return "permission";
      case "disk_error":
        return "disk error";
      default:
        return "error";
    }
  }
  const msg = statusMsg.toLowerCase();
  const DISK_HINTS = [
    "storage",
    "disk",
    "no space",
    "permission",
    "directory",
    "file",
  ];
  if (DISK_HINTS.some((h) => msg.includes(h))) return "disk error";
  return "trk error";
}
