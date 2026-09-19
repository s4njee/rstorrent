/**
 * Action helpers shared by the toolbar, context menu, and keyboard shortcuts.
 *
 * Each takes an explicit list of hashes (usually the current selection) and
 * calls the server command layer. Errors are swallowed here and surfaced through
 * the app log (the server logs failures); callers that need to react to
 * failure can await and catch.
 */

import * as cmd from "./ipc/commands";
import { useUi } from "./store/ui";
import { useTorrents } from "./store/torrents";
import { useNotices } from "./store/notices";
import type { Status } from "./ipc/types";

/** Current selection as an array. */
export function selectedHashes(): string[] {
  return [...useUi.getState().selection];
}

/**
 * Run a transport action optimistically (WC9-S2): the rows take the expected
 * status at once, and revert with one toast if the daemon refuses. The store
 * clears the override itself the moment the daemon reports the new status.
 */
function transport(
  verb: string,
  run: (hashes: string[]) => Promise<unknown>,
  hashes: string[],
  statusFor: (row: { percent: number }) => Status,
): void {
  if (!hashes.length) return;
  const rows = useTorrents.getState().torrents;
  const updates: Record<string, Status> = {};
  for (const hash of hashes) {
    const row = rows.find((t) => t.hash === hash);
    updates[hash] = statusFor(row ?? { percent: 0 });
  }
  useTorrents.getState().markOptimistic(updates);
  void run(hashes).catch((error: unknown) => {
    useTorrents.getState().clearOptimistic(hashes);
    useNotices.getState().push("error", `could not ${verb} the torrent`, {
      detail: String(error),
    });
  });
}

export function resume(hashes = selectedHashes()) {
  transport(
    "start",
    (h) => cmd.start(h),
    hashes,
    (row) => (row.percent >= 100 ? "seeding" : "downloading"),
  );
}

/**
 * Pause: stop transferring but stay loaded (rtorrent's `d.pause`). Separate
 * from `stop`, which closes the torrent — the console's three transport verbs.
 */
export function pause(hashes = selectedHashes()) {
  transport(
    "pause",
    (h) => cmd.pause(h),
    hashes,
    () => "paused",
  );
}

export function stop(hashes = selectedHashes()) {
  transport(
    "stop",
    (h) => cmd.stop(h),
    hashes,
    () => "paused",
  );
}

/**
 * Force a recheck. A recheck of an actively downloading torrent throws away
 * work, so the console asks first (WC9-S3); a stopped set rechecks at once.
 */
export function recheck(hashes = selectedHashes()) {
  if (!hashes.length) return;
  const rows = useTorrents.getState().torrents;
  const downloading = hashes.some(
    (hash) => rows.find((t) => t.hash === hash)?.status === "downloading",
  );
  if (downloading) {
    useUi.getState().openDialog("recheck");
    return;
  }
  void cmd.recheck(hashes);
}

export function forceReannounce(hashes = selectedHashes()) {
  if (hashes.length)
    void cmd.forceReannounce(hashes).catch(() => {
      // The server records the failure in the app log.
    });
}

export function queueUp(hashes = selectedHashes()) {
  if (hashes.length) void cmd.queueMove(hashes, "up");
}

export function queueDown(hashes = selectedHashes()) {
  if (hashes.length) void cmd.queueMove(hashes, "down");
}

export function queueTop(hashes = selectedHashes()) {
  if (hashes.length) void cmd.queueMove(hashes, "top");
}

export function queueBottom(hashes = selectedHashes()) {
  if (hashes.length) void cmd.queueMove(hashes, "bottom");
}

export function toggleForceStart(hashes = selectedHashes()) {
  if (hashes.length) void cmd.toggleForceStart(hashes);
}

/** Copy a torrent's magnet link to the clipboard. */
export async function copyMagnet(hash: string) {
  const uri = await cmd.copyMagnet(hash);
  await navigator.clipboard.writeText(uri);
}

/**
 * Open the remove-confirmation dialog for the current selection.
 *
 * `deleteData` pre-selects the "also delete the files" box, which is what the
 * design's ⇧Delete implies (plain Delete leaves it off).
 */
export function requestRemove(options: { deleteData?: boolean } = {}) {
  if (!useUi.getState().selection.size) return;
  if (options.deleteData) useUi.getState().requestRemoveData();
  useUi.getState().openDialog("remove");
}
