/**
 * The single FIFO that every "add these torrents" entry point feeds: drag &
 * drop onto the window, and paste.
 *
 * Sharing one queue is the point. Each source is handled to completion before
 * the next starts, so dropping three `.torrent` files (or dropping one while an
 * add dialog is already up) walks through the add dialogs one at a time
 * instead of racing them into the same slot.
 *
 * Honours the `showAddDialog` preference: on when set (dialog per source,
 * resolving when it closes), otherwise an instant add with the same defaults
 * the dialog would have applied.
 */

import { addTorrent } from "./ipc/commands";
import type { AddSource } from "./ipc/commands";
import { webUploadTorrent } from "./ipc/web";
import { defaultAddOptions, OpenRequestQueue } from "./externalOpen";
import { useSettings } from "./store/settings";
import { useUi } from "./store/ui";

/** Add one source, via the dialog or instantly depending on preferences. */
async function handleSource(source: AddSource): Promise<void> {
  let settings = useSettings.getState().settings;
  if (!settings) {
    await useSettings.getState().load();
    settings = useSettings.getState().settings;
  }
  if (!settings) throw new Error("settings did not load");

  if (settings.showAddDialog) {
    await new Promise<void>((resolve) =>
      useUi.getState().openExternalAdd(source, resolve),
    );
    return;
  }

  // Instant-add (showAddDialog off): magnets go through the shared command;
  // File uploads hit the multipart endpoint.
  const opts = defaultAddOptions(settings);
  if (source.kind === "upload") {
    await webUploadTorrent(source.file, opts);
  } else {
    await addTorrent(source, opts);
  }
}

let queue: OpenRequestQueue | null = null;

function getQueue(): OpenRequestQueue {
  queue ??= new OpenRequestQueue(handleSource, (error, source) => {
    console.error("could not handle add request", source, error);
  });
  return queue;
}

/** Queue sources for adding. A no-op for an empty list. */
export function enqueueAddSources(sources: AddSource[]): void {
  if (sources.length) getQueue().enqueue(sources);
}

/** Resolves once the queue has drained (used by tests). */
export function addQueueIdle(): Promise<void> {
  return getQueue().whenIdle();
}
