/**
 * The web backend: `fetch`/polling against the `rstorrent-web` server.
 *
 * `invoke` answers the read-ish commands locally (settings, snapshot, log,
 * moves, health) and maps every mutation 1:1 onto `POST /api/cmd/{name}`.
 * `listen("state://snapshot")` polls `GET /api/state` every ~1s with ETag
 * reuse and pauses while the tab is hidden, refetching immediately on focus;
 * the detail, log and moves channels poll the same way.
 */

import type { Backend, UnlistenFn } from "./backend";
import type {
  BandwidthRule,
  DaemonHealth,
  DetailPayload,
  LogEntry,
  MoveStatus,
  Snapshot,
  SnapshotDelta,
} from "./types";
import { webSettings } from "./webSettings";

/** Snapshot poll cadence (ms). */
const POLL_MS = 1000;
/** Detail + log poll cadence (ms). */
const DETAIL_MS = 2000;

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

/** Answer the read-ish commands locally; POST every mutation to the server. */
async function dispatch(
  command: string,
  args: Record<string, unknown>,
): Promise<unknown> {
  switch (command) {
    case "get_settings": {
      const base = webSettings();
      // The server owns the bandwidth rules on the web host (TOML); merge
      // them so the precedence display works there too.
      try {
        const res = await fetch("/api/settings");
        if (res.ok) {
          const body = (await res.json()) as {
            server?: { bandwidthRules?: BandwidthRule[] };
          };
          if (Array.isArray(body?.server?.bandwidthRules)) {
            base.bandwidthRules = body.server.bandwidthRules ?? [];
          }
        }
      } catch {
        // Offline judges display only; the empty default stands.
      }
      return base;
    }
    case "get_snapshot": {
      const res = await fetch("/api/state");
      if (res.status === 401) return handle401();
      if (!res.ok) return null;
      return (await res.json()) as Snapshot;
    }
    case "get_log": {
      // Hydrate the Log tab from the server's ring buffer.
      const { entries } = await fetchLog(0);
      return entries;
    }
    case "get_moves": {
      // Live move-on-complete statuses (V3-14).
      const res = await fetch("/api/moves");
      if (res.status === 401) return handle401();
      if (!res.ok) return [];
      return (await res.json()) as MoveStatus[];
    }
    case "set_detail_watch":
      detailWatch = {
        hash: (args.hash as string | null) ?? null,
        tab: (args.tab as string | null) ?? null,
      };
      return null;
    case "daemon_health": {
      const res = await fetch("/api/health");
      if (res.status === 401) return handle401();
      const body = (await res.json()) as { daemon: DaemonHealth | null };
      return body.daemon ?? {};
    }
    // No server endpoint (the web console has its own Stats page) — reject
    // clearly rather than hitting the server as an unknown command.
    case "get_statistics":
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
export function refetchSnapshotNow(): void {
  activeRefetch?.();
}

/** Shared poll state for snapshot+delta (FND-02). */
const snapshotHandlers = new Set<(s: Snapshot) => void>();
const deltaHandlers = new Set<(d: SnapshotDelta) => void>();
let pollRevision: number | null = null;
let pollEtag: string | null = null;
let pollDeltaEtag: string | null = null;
let pollStopped = false;
let pollTimer: ReturnType<typeof setTimeout> | undefined;
let pollLoopActive = false;

function schedulePoll(): void {
  if (pollStopped) return;
  pollTimer = setTimeout(pollTick, POLL_MS);
}

async function pollTick(): Promise<void> {
  if (pollStopped) return;
  if (document.hidden) return schedulePoll();
  try {
    if (pollRevision === null) {
      // No revision yet — fetch full state.
      const headers: Record<string, string> = {};
      if (pollEtag) headers["If-None-Match"] = pollEtag;
      const res = await fetch("/api/state", { headers });
      if (res.status === 401) {
        onUnauthorized();
        return;
      }
      if (res.status === 200) {
        pollEtag = res.headers.get("ETag");
        const snap = (await res.json()) as Snapshot;
        pollRevision = snap.revision ?? 0;
        for (const cb of snapshotHandlers) cb(snap);
      }
      // 304: unchanged
    } else {
      // Try incremental delta.
      const headers: Record<string, string> = {};
      if (pollDeltaEtag) headers["If-None-Match"] = pollDeltaEtag;
      const res = await fetch(`/api/delta?since=${pollRevision}`, {
        headers,
      });
      if (res.status === 401) {
        onUnauthorized();
        return;
      }
      if (res.status === 200) {
        pollDeltaEtag = res.headers.get("ETag");
        const delta = (await res.json()) as SnapshotDelta;
        pollRevision = delta.revision;
        // Server's snapshot ETag stays for fallback; keep pollEtag as is.
        for (const cb of deltaHandlers) cb(delta);
        // If no delta handler is registered (older App code), also try to
        // apply via snapshot path by fetching full state as fallback.
        if (deltaHandlers.size === 0) {
          // No direct delta consumer — fetch full state to keep snapshot handlers alive.
          const sRes = await fetch("/api/state", {
            headers: pollEtag ? { "If-None-Match": pollEtag } : {},
          });
          if (sRes.status === 200) {
            pollEtag = sRes.headers.get("ETag");
            const snap = (await sRes.json()) as Snapshot;
            pollRevision = snap.revision ?? 0;
            for (const cb of snapshotHandlers) cb(snap);
          }
        }
      } else if (res.status === 304) {
        // No change.
      } else if (res.status === 409) {
        // Missed revision or periodic full — server returned full snapshot.
        pollDeltaEtag = null;
        const snap = (await res.json()) as Snapshot;
        pollEtag = res.headers.get("ETag");
        pollRevision = snap.revision ?? 0;
        for (const cb of snapshotHandlers) cb(snap);
      } else if (res.status === 404 || res.status === 400) {
        // Fallback to full state poll.
        const headers2: Record<string, string> = {};
        if (pollEtag) headers2["If-None-Match"] = pollEtag;
        const sRes = await fetch("/api/state", { headers: headers2 });
        if (sRes.status === 401) {
          onUnauthorized();
          return;
        }
        if (sRes.status === 200) {
          pollEtag = sRes.headers.get("ETag");
          const snap = (await sRes.json()) as Snapshot;
          pollRevision = snap.revision ?? 0;
          pollDeltaEtag = null;
          for (const cb of snapshotHandlers) cb(snap);
        }
      } else {
        // Other status — treat as transient.
        throw new Error(`delta poll failed ${res.status}`);
      }
    }
  } catch {
    // Transient network error; the next tick retries.
  }
  schedulePoll();
}

function ensurePollLoop(): void {
  if (pollLoopActive) return;
  pollLoopActive = true;
  pollStopped = false;

  const onVisibility = () => {
    if (!document.hidden && !pollStopped) {
      clearTimeout(pollTimer);
      void pollTick();
    }
  };
  document.addEventListener("visibilitychange", onVisibility);
  activeRefetch = () => {
    if (!pollStopped) {
      clearTimeout(pollTimer);
      void pollTick();
    }
  };
  // Store cleanup for later.
  (ensurePollLoop as unknown as { _cleanup?: () => void })._cleanup = () => {
    document.removeEventListener("visibilitychange", onVisibility);
    activeRefetch = null;
    pollLoopActive = false;
  };
  void pollTick();
}

function stopPollLoopIfIdle(): void {
  if (snapshotHandlers.size === 0 && deltaHandlers.size === 0) {
    pollStopped = true;
    clearTimeout(pollTimer);
    const cleanup = (ensurePollLoop as unknown as { _cleanup?: () => void })
      ._cleanup;
    if (cleanup) cleanup();
  }
}

/** Reset poll state for unit tests. */
export function __resetPollForTests(): void {
  snapshotHandlers.clear();
  deltaHandlers.clear();
  pollRevision = null;
  pollEtag = null;
  pollDeltaEtag = null;
  pollStopped = false;
  pollLoopActive = false;
  clearTimeout(pollTimer);
  pollTimer = undefined;
  activeRefetch = null;
  const cleanup = (ensurePollLoop as unknown as { _cleanup?: () => void })
    ._cleanup;
  if (cleanup) {
    // Remove the visibility listener if present.
    try {
      // The closure captured inside ensurePollLoop is not directly reachable,
      // but we stored it; invoking it removes the listener.
      cleanup();
    } catch {
      // Already torn down; nothing left to release.
    }
    (ensurePollLoop as unknown as { _cleanup?: () => void })._cleanup =
      undefined;
  }
}

function pollSnapshots(cb: (s: Snapshot) => void): UnlistenFn {
  snapshotHandlers.add(cb);
  ensurePollLoop();
  return () => {
    snapshotHandlers.delete(cb);
    stopPollLoopIfIdle();
  };
}

function pollDeltas(cb: (d: SnapshotDelta) => void): UnlistenFn {
  deltaHandlers.add(cb);
  ensurePollLoop();
  return () => {
    deltaHandlers.delete(cb);
    stopPollLoopIfIdle();
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

/** Poll `GET /api/moves` every ~2s, nudging only when the list changed.
 *  Byte progress arrives through the same channel; the pill refetches it. */
function pollMoves(cb: () => void): UnlistenFn {
  let stopped = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let last = "";

  const tick = async () => {
    if (stopped) return;
    try {
      const res = await fetch("/api/moves");
      if (res.status === 401) {
        onUnauthorized();
        return;
      }
      if (res.ok) {
        const text = await res.text();
        if (text !== last) {
          last = text;
          cb();
        }
      }
    } catch {
      // Transient; the next tick retries.
    }
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
      case "state://delta":
        return Promise.resolve(
          pollDeltas(handler as (d: SnapshotDelta) => void),
        );
      case "state://detail":
        return Promise.resolve(
          pollDetail(handler as (d: DetailPayload) => void),
        );
      case "log://append":
        return Promise.resolve(pollLog(handler as (e: LogEntry) => void));
      case "moves://update":
        return Promise.resolve(pollMoves(handler as () => void));
      // Unknown channels have nothing to poll.
      default:
        return Promise.resolve(NOOP_UNLISTEN);
    }
  },
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

/**
 * Sign out everywhere: revoke **every** server session (WEB-05). This browser's
 * cookie is cleared too, so the caller can drop back to the login screen.
 */
export async function webRevokeAllSessions(): Promise<void> {
  await fetch("/api/sessions", {
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
