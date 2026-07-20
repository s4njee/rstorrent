/**
 * Pure parsing for every "add these torrents" entry point: macOS
 * open-file/deep-link events, drag & drop onto the window, and paste.
 *
 * All of them boil down to turning opaque strings (URLs, paths, clipboard text)
 * into `AddSource`s, so they share one value parser and silently drop anything
 * that isn't a torrent — an unrelated dropped file or pasted link must be a
 * no-op, never an error dialog.
 */

import type { AddSource } from "./ipc/commands";
import type { AddOptions, Settings } from "./ipc/types";

/** Does this path/pathname name a `.torrent`? */
function isTorrentPath(value: string): boolean {
  return value.toLowerCase().endsWith(".torrent");
}

/**
 * Parse one value into at most one add source.
 *
 * `allowHttp` enables `http(s)://…/x.torrent` URLs, which only user-pasted or
 * dropped text can carry (LaunchServices never delivers them). Such URLs are
 * handed to rtorrent as a "magnet" source because its `load.start` takes a URL
 * or a magnet interchangeably. We require the `.torrent` suffix: pasting an
 * ordinary web link must not add anything. The add-magnet dialog stays more
 * permissive — an explicit paste into that field is an explicit intent.
 */
function parseValue(raw: string, allowHttp: boolean): AddSource[] {
  const value = raw.trim();
  if (!value) return [];

  if (/^magnet:/i.test(value)) {
    return [{ kind: "magnet" as const, uri: value }];
  }

  if (allowHttp && /^https?:\/\//i.test(value)) {
    try {
      // pathname excludes any query string, so ?id=1 suffixes don't defeat it.
      return isTorrentPath(new URL(value).pathname)
        ? [{ kind: "magnet" as const, uri: value }]
        : [];
    } catch {
      return [];
    }
  }

  // Supporting an absolute path as well as file:// makes local/dev event
  // injection convenient without weakening the accepted file type. Windows
  // delivers file associations as bare `C:\…` argv entries, so both an absolute
  // POSIX path and a drive-letter path have to count as absolute here.
  if (isAbsolutePath(value) && isTorrentPath(value)) {
    return [{ kind: "file" as const, path: value }];
  }

  try {
    const url = new URL(value);
    if (
      url.protocol !== "file:" ||
      (url.hostname && url.hostname !== "localhost") ||
      !isTorrentPath(url.pathname)
    ) {
      return [];
    }
    return [{ kind: "file" as const, path: filePathFromUrl(url) }];
  } catch {
    return [];
  }
}

/** POSIX (`/x`), Windows drive (`C:\x`), or UNC (`\\host\x`). */
function isAbsolutePath(value: string): boolean {
  return (
    value.startsWith("/") ||
    /^[a-z]:[\\/]/i.test(value) ||
    value.startsWith("\\\\")
  );
}

/**
 * Turn a `file:` URL into a native path.
 *
 * `pathname` is always POSIX-shaped, so on Windows it arrives as
 * `/C:/Users/you/x.torrent` — the leading slash has to go and the separators
 * have to be flipped, or the path reaches the backend unopenable.
 */
function filePathFromUrl(url: URL): string {
  const decoded = decodeURIComponent(url.pathname);
  if (/^\/[a-z]:/i.test(decoded)) {
    return decoded.slice(1).replace(/\//g, "\\");
  }
  return decoded;
}

/** Convert LaunchServices URLs into add sources, ignoring unrelated inputs. */
export function parseOpenRequests(urls: string[]): AddSource[] {
  return urls.flatMap((raw) => parseValue(raw, false));
}

/**
 * Convert filesystem paths from a window drop into add sources. Tauri hands us
 * real paths (unlike an HTML5 `File`), so these go down the same path-based
 * route as a Finder open. Non-torrent files in the same drop are ignored.
 */
export function parseDroppedPaths(paths: string[]): AddSource[] {
  return paths
    .filter((path) => isTorrentPath(path.trim()))
    .map((path) => ({ kind: "file" as const, path: path.trim() }));
}

/**
 * Convert pasted (or text-dropped) clipboard content into add sources. Split on
 * newlines rather than all whitespace so a pasted path containing spaces stays
 * intact; magnets and URLs never contain raw spaces.
 */
export function parsePastedText(text: string): AddSource[] {
  return text.split(/[\r\n]+/).flatMap((line) => parseValue(line, true));
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
