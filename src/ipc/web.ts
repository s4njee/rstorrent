/**
 * The web host backend: `fetch`/polling against the `rstorrent-web` server.
 *
 * WE1 implements the **read path** — the snapshot poll that drives the live
 * table. `invoke` answers the handful of read-ish commands the app issues at
 * startup and rejects mutations (they land in WE3). `listen("state://snapshot")`
 * polls `GET /api/state` every ~1s with ETag reuse and pauses while the tab is
 * hidden, refetching immediately on focus — matching the desktop poller's push
 * cadence over HTTP.
 */

import type { Backend, Capabilities, UnlistenFn } from "./backend";
import type { DaemonHealth, DetailPayload, LogEntry, Snapshot } from "./types";
import { webSettings } from "./webSettings";

/** Snapshot poll cadence (ms). */
const POLL_MS = 1000;
/** Detail + log poll cadence (ms). */
const DETAIL_MS = 2000;

/** The browser can only read the clipboard; everything else is the server's. */
const capabilities: Capabilities = {
  localFs: false,
  nativeDialogs: false,
  keychain: false,
  menus: false,
  deepLinks: false,
  clipboardRead: true,
};

/** Last detail watch set by `set_detail_watch`; the detail loop reads it (WE3). */
let detailWatch: { hash: string | null; tab: string | null } = {
  hash: null,
  tab: null,
};

/** Invoked when any request sees a 401 — wired to the login screen in WE5. */
let onUnauthorized: () => void = () => {};
export function setUnauthorizedHandler(fn: () => void): void {
  onUnauthorized = fn;
}

/** Answer the read-ish startup commands; reject everything else (WE3). */
async function dispatch(
  command: string,
  args: Record<string, unknown>,
): Promise<unknown> {
  switch (command) {
    case "get_settings":
      return webSettings();
    case "get_log": {
      // Hydrate the Log tab from the server's ring buffer.
      const { entries } = await fetchLog(0);
      return entries;
    }
    case "take_open_requests":
      // No file/magnet deep links in the browser.
      return [] as string[];
    case "set_detail_watch":
      detailWatch = {
        hash: (args.hash as string | null) ?? null,
        tab: (args.tab as string | null) ?? null,
      };
      return null;
    case "retry_connection":
      // The server polls on its own; a client can't force a reconnect yet.
      return null;
    case "daemon_health": {
      const res = await fetch("/api/health");
      if (res.status === 401) return handle401();
      const body = (await res.json()) as { daemon: DaemonHealth | null };
      return body.daemon ?? {};
    }
    // Desktop-only surface the web shell never renders — reject clearly so a
    // stray call is obvious rather than hitting the server as an unknown command.
    case "open_destination":
    case "set_http_password":
    case "has_http_password":
    case "clear_http_password":
    case "test_connection":
    case "set_turtle":
    case "tuning_preview":
    case "apply_tuning":
    case "rss_fetch":
    case "rss_download":
    case "get_statistics":
    case "save_session":
    case "shutdown_daemon":
      throw new Error(`not available in the web UI: ${command}`);
    default:
      // Everything else is a mutation whose name maps 1:1 to POST /api/cmd/{name}.
      return postCommand(command, args);
  }
}

/** POST a mutation to `/api/cmd/{name}`; resolves with the JSON result. */
async function postCommand(
  name: string,
  args: Record<string, unknown>,
): Promise<unknown> {
  const res = await fetch(`/api/cmd/${name}`, {
    method: "POST",
    headers: { "Content-Type": "application/json", "X-Rstorrent": "1" },
    body: JSON.stringify(args),
  });
  if (res.status === 401) return handle401();
  const body = await res.json().catch(() => null);
  if (!res.ok) {
    const message =
      (body && typeof body === "object" && "error" in body
        ? (body as { error: string }).error
        : null) ?? `request failed (${res.status})`;
    throw new Error(message);
  }
  // A successful mutation should refresh the table promptly.
  refetchSnapshotNow();
  return body;
}

/** Fetch a slice of the log ring buffer after a sequence number. */
async function fetchLog(
  after: number,
): Promise<{ entries: LogEntry[]; seq: number }> {
  const res = await fetch(`/api/log?after=${after}`);
  if (res.status === 401) handle401();
  if (!res.ok) return { entries: [], seq: after };
  return (await res.json()) as { entries: LogEntry[]; seq: number };
}

function handle401(): never {
  onUnauthorized();
  throw new Error("unauthorized");
}

/** Trigger an immediate snapshot refetch (set by the active snapshot poller). */
let activeRefetch: (() => void) | null = null;
function refetchSnapshotNow(): void {
  activeRefetch?.();
}

/** Poll `GET /api/state`, honoring ETags and pausing while the tab is hidden. */
function pollSnapshots(cb: (s: Snapshot) => void): UnlistenFn {
  let etag: string | null = null;
  let stopped = false;
  let timer: ReturnType<typeof setTimeout> | undefined;

  const schedule = () => {
    if (stopped) return;
    timer = setTimeout(tick, POLL_MS);
  };

  const tick = async () => {
    if (stopped) return;
    if (document.hidden) return schedule(); // paused: skip the fetch, keep ticking
    try {
      const headers: Record<string, string> = {};
      if (etag) headers["If-None-Match"] = etag;
      const res = await fetch("/api/state", { headers });
      if (res.status === 401) {
        onUnauthorized();
        return; // stop polling until re-auth
      }
      if (res.status === 200) {
        etag = res.headers.get("ETag");
        cb((await res.json()) as Snapshot);
      }
      // 304: unchanged — keep the ETag and the store's current state.
    } catch {
      // Transient network error; the next tick retries.
    }
    schedule();
  };

  const onVisibility = () => {
    if (!document.hidden && !stopped) {
      clearTimeout(timer);
      void tick(); // refetch immediately on focus
    }
  };
  document.addEventListener("visibilitychange", onVisibility);
  activeRefetch = () => {
    if (!stopped) {
      clearTimeout(timer);
      void tick();
    }
  };

  void tick(); // first fetch immediately
  return () => {
    stopped = true;
    activeRefetch = null;
    clearTimeout(timer);
    document.removeEventListener("visibilitychange", onVisibility);
  };
}

/** Poll `GET /api/detail` for the currently-watched (hash, tab) every ~2s. */
function pollDetail(cb: (d: DetailPayload) => void): UnlistenFn {
  let stopped = false;
  let timer: ReturnType<typeof setTimeout> | undefined;

  const tick = async () => {
    if (stopped) return;
    const { hash, tab } = detailWatch;
    if (hash && tab) {
      try {
        const res = await fetch(
          `/api/detail?hash=${encodeURIComponent(hash)}&tab=${tab}`,
        );
        if (res.status === 401) {
          onUnauthorized();
          return;
        }
        if (res.ok) cb((await res.json()) as DetailPayload);
      } catch {
        // Transient; the next tick retries.
      }
    }
    if (!stopped) timer = setTimeout(tick, DETAIL_MS);
  };

  void tick();
  return () => {
    stopped = true;
    clearTimeout(timer);
  };
}

/** Poll `GET /api/log` every ~2s, delivering only entries newer than the last
 *  seen sequence. The initial batch is skipped (getLog hydrates that). */
function pollLog(cb: (e: LogEntry) => void): UnlistenFn {
  let stopped = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let lastSeq = 0;
  let primed = false;

  const tick = async () => {
    if (stopped) return;
    const { entries, seq } = await fetchLog(primed ? lastSeq : 0);
    if (!primed) {
      primed = true; // first pass just anchors the sequence; getLog hydrated it
    } else {
      for (const e of entries) cb(e);
    }
    lastSeq = seq;
    if (!stopped) timer = setTimeout(tick, DETAIL_MS);
  };

  void tick();
  return () => {
    stopped = true;
    clearTimeout(timer);
  };
}

const NOOP_UNLISTEN: UnlistenFn = () => {};

export const webBackend: Backend = {
  async invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
    return (await dispatch(command, args ?? {})) as T;
  },

  listen<T>(event: string, handler: (payload: T) => void): Promise<UnlistenFn> {
    switch (event) {
      case "state://snapshot":
        return Promise.resolve(pollSnapshots(handler as (s: Snapshot) => void));
      case "state://detail":
        return Promise.resolve(
          pollDetail(handler as (d: DetailPayload) => void),
        );
      case "log://append":
        return Promise.resolve(pollLog(handler as (e: LogEntry) => void));
      // The desktop-only channels (native menus, deep links, notifications)
      // have no browser equivalent.
      default:
        return Promise.resolve(NOOP_UNLISTEN);
    }
  },

  capabilities,
};

/** The current detail watch (hash + tab), for the WE3 detail loop. */
export function currentDetailWatch(): {
  hash: string | null;
  tab: string | null;
} {
  return detailWatch;
}

/** Log in with a password; resolves on success, rejects with the server's
 *  message otherwise. The session cookie is set by the response. */
export async function webLogin(password: string): Promise<void> {
  const res = await fetch("/api/session", {
    method: "POST",
    headers: { "Content-Type": "application/json", "X-Rstorrent": "1" },
    body: JSON.stringify({ password }),
  });
  if (res.ok) return;
  const body = (await res.json().catch(() => null)) as {
    error?: string;
  } | null;
  throw new Error(
    body?.error ??
      (res.status === 429
        ? "too many attempts — wait a minute"
        : "login failed"),
  );
}

/** Log out: revoke the session and clear the cookie. */
export async function webLogout(): Promise<void> {
  await fetch("/api/session", {
    method: "DELETE",
    headers: { "X-Rstorrent": "1" },
  }).catch(() => {});
}

/** Parse an uploaded `.torrent` File into metadata (for the Add dialog tree). */
export async function webInspectTorrent(
  file: File,
): Promise<import("./types").TorrentMeta> {
  const form = new FormData();
  form.append("file", file);
  const res = await fetch("/api/torrents/inspect", {
    method: "POST",
    headers: { "X-Rstorrent": "1" },
    body: form,
  });
  if (res.status === 401) return handle401();
  const body = await res.json().catch(() => null);
  if (!res.ok) throw new Error(body?.error ?? "could not read .torrent");
  return body as import("./types").TorrentMeta;
}

/** Upload a `.torrent` File with add options. */
export async function webUploadTorrent(
  file: File,
  opts: import("./types").AddOptions,
): Promise<void> {
  const form = new FormData();
  form.append("file", file);
  form.append("opts", JSON.stringify(opts));
  const res = await fetch("/api/torrents/file", {
    method: "POST",
    headers: { "X-Rstorrent": "1" },
    body: form,
  });
  if (res.status === 401) return handle401();
  if (!res.ok) {
    const body = await res.json().catch(() => null);
    throw new Error(body?.error ?? "upload failed");
  }
  refetchSnapshotNow();
}
