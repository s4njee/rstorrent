/**
 * Pure parsing for every "add these torrents" entry point: drag & drop onto
 * the window and paste.
 *
 * All of them boil down to turning opaque inputs (dropped files, clipboard
 * text) into `AddSource`s, so they share one value parser and silently drop
 * anything that isn't a torrent — an unrelated dropped file or pasted link must
 * be a no-op, never an error dialog.
 */

import type { AddSource } from "./ipc/commands";
import type { AddOptions, Settings } from "./ipc/types";

/** Does this file name / URL pathname name a `.torrent`? */
function isTorrentPath(value: string): boolean {
  return value.toLowerCase().endsWith(".torrent");
}

/**
 * Parse one line of text into at most one add source.
 *
 * `http(s)://…/x.torrent` URLs are handed to rtorrent as a "magnet" source
 * because its `load.start` takes a URL or a magnet interchangeably. We require
 * the `.torrent` suffix: pasting an ordinary web link must not add anything.
 * The add-magnet dialog stays more permissive — an explicit paste into that
 * field is an explicit intent.
 */
function parseValue(raw: string): AddSource[] {
  const value = raw.trim();
  if (!value) return [];

  if (/^magnet:/i.test(value)) {
    return [{ kind: "magnet" as const, uri: value }];
  }

  if (/^https?:\/\//i.test(value)) {
    try {
      // pathname excludes any query string, so ?id=1 suffixes don't defeat it.
      return isTorrentPath(new URL(value).pathname)
        ? [{ kind: "magnet" as const, uri: value }]
        : [];
    } catch {
      return [];
    }
  }

  return [];
}

/**
 * Convert browser `File` objects from a DOM drop into upload sources (WE4-S3).
 * Non-torrent files in the same drop are ignored.
 */
export function parseDroppedFiles(files: Iterable<File>): AddSource[] {
  return [...files]
    .filter((file) => isTorrentPath(file.name))
    .map((file) => ({ kind: "upload" as const, file }));
}

/**
 * Convert pasted (or text-dropped) clipboard content into add sources, one
 * per line; magnets and URLs never contain raw spaces.
 */
export function parsePastedText(text: string): AddSource[] {
  return text.split(/[\r\n]+/).flatMap(parseValue);
}

/** The same defaults used when a user accepts either add dialog unchanged. */
export function defaultAddOptions(settings: Settings): AddOptions {
  return {
    savePath: settings.defaultSavePath,
    label: "",
    start: true,
    topOfQueue: false,
    sequential: false,
    skipHashCheck: false,
    unselectedIndexes: [],
  };
}

/**
 * A small FIFO that awaits each handler before advancing. Dialog-backed
 * handlers resolve when the dialog closes; instant-add handlers resolve when
 * the backend command completes.
 */
export class OpenRequestQueue {
  private readonly pending: AddSource[] = [];
  private draining: Promise<void> | null = null;

  constructor(
    private readonly handle: (source: AddSource) => Promise<void>,
    private readonly onError: (
      error: unknown,
      source: AddSource,
    ) => void = () => {},
  ) {}

  enqueue(sources: AddSource[]): void {
    this.pending.push(...sources);
    if (!this.draining && this.pending.length) {
      this.draining = this.drain().finally(() => {
        this.draining = null;
        // An enqueue can land in the narrow window after drain's loop exits.
        if (this.pending.length) this.enqueue([]);
      });
    }
  }

  whenIdle(): Promise<void> {
    return this.draining ?? Promise.resolve();
  }

  private async drain(): Promise<void> {
    while (this.pending.length) {
      const source = this.pending.shift()!;
      try {
        await this.handle(source);
      } catch (error) {
        this.onError(error, source);
      }
    }
  }
}
