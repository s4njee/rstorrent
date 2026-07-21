/**
 * Action helpers shared by the toolbar, context menu, and keyboard shortcuts.
 *
 * Each takes an explicit list of hashes (usually the current selection) and
 * calls the Rust command layer. Errors are swallowed here and surfaced through
 * the app log (the Rust side logs failures); callers that need to react to
 * failure can await and catch.
 */

import * as cmd from "./ipc/commands";
import { useUi } from "./store/ui";
import { capabilities } from "./ipc/backend";

/** Write to the clipboard through whichever host provides it. */
async function writeClipboard(text: string): Promise<void> {
  if (capabilities().nativeDialogs) {
    // Desktop: the Tauri clipboard plugin (loaded only in that shell).
    const { writeText } = await import("@tauri-apps/plugin-clipboard-manager");
    await writeText(text);
  } else {
    await navigator.clipboard.writeText(text);
  }
}

/** Current selection as an array. */
export function selectedHashes(): string[] {
  return [...useUi.getState().selection];
}

export function resume(hashes = selectedHashes()) {
  if (hashes.length) void cmd.start(hashes);
}

export function pause(hashes = selectedHashes()) {
  if (hashes.length) void cmd.stop(hashes);
}

export function recheck(hashes = selectedHashes()) {
  if (hashes.length) void cmd.recheck(hashes);
}

export function forceReannounce(hashes = selectedHashes()) {
  if (hashes.length)
    void cmd.forceReannounce(hashes).catch(() => {
      // The Rust command records the failure in the app log.
    });
}

export function queueUp(hashes = selectedHashes()) {
  if (hashes.length) void cmd.queueMove(hashes, "up");
}

export function queueDown(hashes = selectedHashes()) {
  if (hashes.length) void cmd.queueMove(hashes, "down");
}

/** Copy a torrent's magnet link to the clipboard. */
export async function copyMagnet(hash: string) {
  const uri = await cmd.copyMagnet(hash);
  await writeClipboard(uri);
}

export function openDestination(hash: string) {
  void cmd.openDestination(hash);
}

/** Open the remove-confirmation dialog for the current selection. */
export function requestRemove() {
  if (useUi.getState().selection.size) useUi.getState().openDialog("remove");
}
