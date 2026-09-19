/**
 * The console's status cell.
 *
 * rtorrent reports raw flags; the design words them. This is the single place
 * that translation happens, so a row's status reads the same wherever it is
 * rendered — the table cell, the sidebar buckets and the status bar all agree
 * because they all come from here.
 *
 * The design's vocabulary is Downloading · Seeding · Stopped · Queued ·
 * `Checking 42%` · Tracker error. Two additions:
 *
 *   · **Stalled** — rtorrent has the state and the app has always shown it; the
 *     design's list is what its prototype happened to contain, not a rule.
 *   · **Error vs Tracker error** — the design only had tracker failures in its
 *     sample data, but a storage or permission failure is not a tracker
 *     problem, so those read "Error" and only tracker kinds read "Tracker
 *     error".
 */

import type { Status, TorrentDto } from "../ipc/types";

/** Which colour family the status word takes. */
export type StatusTone = "active" | "seeding" | "idle" | "warn" | "error";

export interface StatusWord {
  text: string;
  tone: StatusTone;
}

/** The minimum a status word needs: `TorrentDto` is structurally compatible. */
export interface StatusInput {
  status: Status;
  percent: number;
  errorKind: string;
  isOpen?: boolean;
  isActive?: boolean;
}

/** Error kinds that are the tracker's fault rather than the disk's. */
const TRACKER_KINDS = new Set([
  "unregistered",
  "tracker_timeout",
  "tracker_error",
]);

/**
 * The status cell's word and tone.
 *
 * `Checking 42%` carries the verification sweep, which rtorrent reports as
 * hashed chunks — `percent` already holds that while a torrent is hashing (see
 * `derive::to_dto`).
 */
export function statusWord(torrent: StatusInput): StatusWord {
  switch (torrent.status) {
    case "error":
      return {
        text: TRACKER_KINDS.has(torrent.errorKind) ? "Tracker error" : "Error",
        tone: "error",
      };
    case "checking": {
      const percent = Math.round(torrent.percent);
      return { text: `Checking ${percent}%`, tone: "active" };
    }
    case "paused":
      // Closed in the daemon = stopped; loaded but idle = waiting its turn.
      // `isOpen` is absent on an older snapshot, which reads as stopped.
      return torrent.isOpen
        ? { text: "Queued", tone: "idle" }
        : { text: "Stopped", tone: "idle" };
    case "stalled":
      return { text: "Stalled", tone: "warn" };
    case "seeding":
    case "completed":
      return { text: "Seeding", tone: "seeding" };
    case "downloading":
    default:
      return { text: "Downloading", tone: "active" };
  }
}

/** Convenience for callers holding a full DTO. */
export function statusWordOf(torrent: TorrentDto): StatusWord {
  return statusWord(torrent);
}

/**
 * The Seeds/Peers cell: connected seeds, then connected peers.
 *
 * While seeding there is nothing to download *from*, so the design shows an
 * em-dash for the seed count and only the peers it is uploading to.
 */
export function seedsPeersCell(torrent: TorrentDto): string {
  const seeds =
    torrent.status === "seeding" || torrent.status === "completed"
      ? "—"
      : String(torrent.seedsConnected);
  return `${seeds} / ${torrent.peersConnected}`;
}

/**
 * The Ratio cell's tone: the design dims a ratio below 2.00, so a poor share
 * ratio is visible without reading the number.
 */
export function ratioTone(ratio: number): "good" | "poor" {
  return ratio >= 2 ? "good" : "poor";
}

/**
 * A tracker row's tone, per the design: working is the accent, a failure warns,
 * a disabled tracker is inert, and anything in flight is "working" until the
 * daemon says otherwise.
 *
 * `enabled: false` wins over the reported status — rtorrent keeps announcing a
 * disabled tracker's last status, which would otherwise read as working.
 */
export function trackerTone(status: string, enabled: boolean): StatusTone {
  if (!enabled) return "idle";
  switch (status) {
    case "error":
    case "timeout":
    case "timed out":
      return "error";
    case "updating":
    case "warning":
      return "warn";
    default:
      return "active";
  }
}
