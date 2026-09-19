/**
 * Duplicate detection before add (V3-13 / LIB-04).
 *
 * A browser cannot call `rtorrent_core`, so this mirrors
 * `crates/rtorrent/src/duplicates.rs` rule for rule; the tests on both sides pin
 * the same behaviour. The add flows run this before submitting so nothing is
 * added silently.
 */

import type { TorrentDto } from "../ipc/types";

/** What is being added, as far as it is known before the daemon resolves it. */
export interface AddCandidate {
  /** The info-hash, when known (an inspected `.torrent` or a magnet). */
  hash?: string | null;
  name?: string | null;
  size?: number | null;
  /** The chosen save directory (the torrent's own folder is `name` under it). */
  destination?: string | null;
}

/** The strongest reason not to add silently. */
export type Duplicate =
  | { kind: "exact"; hash: string; name: string }
  | { kind: "sameNameAndSize"; hash: string; name: string; size: number }
  | { kind: "destinationInUse"; hash: string; name: string; path: string };

/** Trailing slashes and case do not distinguish a path. */
function normalisePath(path: string): string {
  return path.trim().replace(/\/+$/, "").toLowerCase();
}

/**
 * The strongest duplicate finding, or null when the add is clean. Order: exact
 * info-hash, then name+size, then destination collision.
 */
export function detectDuplicate(
  candidate: AddCandidate,
  library: readonly TorrentDto[],
): Duplicate | null {
  const hash = candidate.hash?.trim();
  if (hash) {
    const existing = library.find(
      (torrent) => torrent.hash.toLowerCase() === hash.toLowerCase(),
    );
    if (existing) {
      return { kind: "exact", hash: existing.hash, name: existing.name };
    }
  }

  const name = candidate.name?.trim() ?? "";
  const size = candidate.size ?? 0;
  if (name && size > 0) {
    const existing = library.find(
      (torrent) =>
        torrent.size === size &&
        torrent.name.toLowerCase() === name.toLowerCase(),
    );
    if (existing) {
      return {
        kind: "sameNameAndSize",
        hash: existing.hash,
        name: existing.name,
        size,
      };
    }
  }

  const destination = candidate.destination?.trim() ?? "";
  if (destination && name) {
    const target = normalisePath(`${destination.replace(/\/+$/, "")}/${name}`);
    const existing = library.find(
      (torrent) => normalisePath(torrent.savePath ?? "") === target,
    );
    if (existing) {
      return {
        kind: "destinationInUse",
        hash: existing.hash,
        name: existing.name,
        path: existing.savePath,
      };
    }
  }

  return null;
}
