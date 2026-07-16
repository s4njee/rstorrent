/** Pure parsing and serialization logic for macOS open-file/deep-link events. */

import type { AddSource } from "./ipc/commands";
import type { AddOptions, Settings } from "./ipc/types";

/** Convert LaunchServices URLs into add sources, ignoring unrelated inputs. */
export function parseOpenRequests(urls: string[]): AddSource[] {
  return urls.flatMap<AddSource>((raw) => {
    const value = raw.trim();
    if (/^magnet:/i.test(value)) {
      return [{ kind: "magnet" as const, uri: value }];
    }

    // Supporting an absolute path as well as file:// makes local/dev event
    // injection convenient without weakening the accepted file type.
    if (value.startsWith("/") && value.toLowerCase().endsWith(".torrent")) {
      return [{ kind: "file" as const, path: value }];
    }

    try {
      const url = new URL(value);
      if (
        url.protocol !== "file:" ||
        (url.hostname && url.hostname !== "localhost") ||
        !url.pathname.toLowerCase().endsWith(".torrent")
      ) {
        return [];
      }
      return [
        {
          kind: "file" as const,
          path: decodeURIComponent(url.pathname),
        },
      ];
    } catch {
      return [];
    }
  });
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
