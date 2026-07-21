import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { webBackend, setUnauthorizedHandler } from "./web";
import type { Snapshot } from "./types";

const SNAP: Snapshot = {
  torrents: [],
  globals: {
    downRate: 1,
    upRate: 2,
    downRateLimit: 0,
    upRateLimit: 0,
    dhtNodes: 0,
    freeSpace: null,
    diskSize: null,
    turtleActive: false,
  },
  connection: {
    phase: "connected",
    endpoint: "unix:/x",
    daemonVersion: "0.9.8",
    error: null,
    retryInSeconds: null,
  },
};

/** A minimal `Response`-like for the parts web.ts reads. */
function res(
  body: unknown,
  { status = 200, etag }: { status?: number; etag?: string } = {},
) {
  return {
    status,
    ok: status >= 200 && status < 300,
    headers: {
      get: (h: string) => (h.toLowerCase() === "etag" ? (etag ?? null) : null),
    },
    json: async () => body,
  };
}

let hidden = false;

beforeEach(() => {
  vi.useFakeTimers();
  hidden = false;
  Object.defineProperty(document, "hidden", {
    configurable: true,
    get: () => hidden,
  });
});

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
  setUnauthorizedHandler(() => {});
});

describe("web backend — snapshot polling", () => {
  it("delivers the first snapshot and reuses the ETag on the next poll", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValue(res(SNAP, { status: 200, etag: '"abc"' }));
    vi.stubGlobal("fetch", fetchMock);

    const cb = vi.fn();
    const un = await webBackend.listen<Snapshot>("state://snapshot", cb);

    // First tick fires immediately.
    await vi.advanceTimersByTimeAsync(0);
    expect(cb).toHaveBeenCalledWith(SNAP);
    expect(fetchMock).toHaveBeenLastCalledWith("/api/state", { headers: {} });

    // Next poll one interval later carries If-None-Match with the stored ETag.
    await vi.advanceTimersByTimeAsync(1000);
    expect(fetchMock).toHaveBeenLastCalledWith("/api/state", {
      headers: { "If-None-Match": '"abc"' },
    });
    un();
  });

  it("does not re-deliver on a 304", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(res(SNAP, { status: 200, etag: '"abc"' }))
      .mockResolvedValue(res(null, { status: 304 }));
    vi.stubGlobal("fetch", fetchMock);

    const cb = vi.fn();
    const un = await webBackend.listen<Snapshot>("state://snapshot", cb);
    await vi.advanceTimersByTimeAsync(0);
    await vi.advanceTimersByTimeAsync(1000);

    expect(cb).toHaveBeenCalledTimes(1); // only the initial 200
    un();
  });

  it("pauses fetching while the tab is hidden", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValue(res(SNAP, { status: 200, etag: '"abc"' }));
    vi.stubGlobal("fetch", fetchMock);

    const cb = vi.fn();
    const un = await webBackend.listen<Snapshot>("state://snapshot", cb);
    await vi.advanceTimersByTimeAsync(0);
    expect(fetchMock).toHaveBeenCalledTimes(1);

    hidden = true;
    await vi.advanceTimersByTimeAsync(3000); // several intervals, tab hidden
    expect(fetchMock).toHaveBeenCalledTimes(1); // no further fetches

    // Becoming visible again refetches immediately.
    hidden = false;
    document.dispatchEvent(new Event("visibilitychange"));
    await vi.advanceTimersByTimeAsync(0);
    expect(fetchMock).toHaveBeenCalledTimes(2);
    un();
  });

  it("stops polling after unlisten", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValue(res(SNAP, { status: 200, etag: '"abc"' }));
    vi.stubGlobal("fetch", fetchMock);

    const un = await webBackend.listen<Snapshot>("state://snapshot", vi.fn());
    await vi.advanceTimersByTimeAsync(0);
    const calls = fetchMock.mock.calls.length;
    un();
    await vi.advanceTimersByTimeAsync(5000);
    expect(fetchMock.mock.calls.length).toBe(calls);
  });

  it("invokes the unauthorized handler on a 401", async () => {
    const fetchMock = vi.fn().mockResolvedValue(res(null, { status: 401 }));
    vi.stubGlobal("fetch", fetchMock);
    const onAuth = vi.fn();
    setUnauthorizedHandler(onAuth);

    const un = await webBackend.listen<Snapshot>("state://snapshot", vi.fn());
    await vi.advanceTimersByTimeAsync(0);
    expect(onAuth).toHaveBeenCalled();
    un();
  });
});

describe("web backend — commands", () => {
  it("answers get_settings with a type-complete default", async () => {
    const settings = await webBackend.invoke<{ pollMs: number; mock: boolean }>(
      "get_settings",
    );
    expect(settings.pollMs).toBe(1000);
    expect(settings.mock).toBe(false);
  });

  it("hydrates the log via /api/log and returns [] for open-requests", async () => {
    const entries = [{ message: "connected" }];
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(res({ entries, seq: 1 }, { status: 200 })),
    );
    expect(await webBackend.invoke("get_log")).toEqual(entries);
    expect(await webBackend.invoke("take_open_requests")).toEqual([]);
  });

  it("posts a mutation to /api/cmd/{name}", async () => {
    const fetchMock = vi.fn().mockResolvedValue(res(null, { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);
    const result = await webBackend.invoke("start", { hashes: ["A"] });
    expect(result).toBeNull();
    expect(fetchMock).toHaveBeenCalledWith(
      "/api/cmd/start",
      expect.objectContaining({ method: "POST" }),
    );
  });

  it("surfaces a mutation error message from the server", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValue(
        res({ error: "rtorrent fault 5: nope" }, { status: 502 }),
      );
    vi.stubGlobal("fetch", fetchMock);
    await expect(webBackend.invoke("stop", { hashes: ["A"] })).rejects.toThrow(
      /rtorrent fault 5/,
    );
  });

  it("rejects desktop-only commands without hitting the server", async () => {
    const fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);
    await expect(webBackend.invoke("test_connection", {})).rejects.toThrow(
      /not available in the web UI/,
    );
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("non-blank capabilities: browser can read clipboard, not the local FS", () => {
    expect(webBackend.capabilities.clipboardRead).toBe(true);
    expect(webBackend.capabilities.localFs).toBe(false);
    expect(webBackend.capabilities.nativeDialogs).toBe(false);
  });
});

describe("web backend — detail + log loops", () => {
  it("polls /api/detail for the watched hash + tab", async () => {
    await webBackend.invoke("set_detail_watch", { hash: "A", tab: "peers" });
    const payload = { hash: "A", tab: "peers", peers: [] };
    const fetchMock = vi.fn().mockResolvedValue(res(payload, { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);

    const cb = vi.fn();
    const un = await webBackend.listen("state://detail", cb);
    await vi.advanceTimersByTimeAsync(0);
    expect(fetchMock).toHaveBeenCalledWith("/api/detail?hash=A&tab=peers");
    expect(cb).toHaveBeenCalledWith(payload);
    un();
  });

  it("delivers only log entries newer than the primed sequence", async () => {
    const fetchMock = vi
      .fn()
      // First tick anchors the sequence (getLog already hydrated these).
      .mockResolvedValueOnce(res({ entries: [{ message: "old" }], seq: 5 }))
      // Later ticks deliver what's new.
      .mockResolvedValue(res({ entries: [{ message: "new" }], seq: 6 }));
    vi.stubGlobal("fetch", fetchMock);

    const cb = vi.fn();
    const un = await webBackend.listen("log://append", cb);
    await vi.advanceTimersByTimeAsync(0);
    expect(cb).not.toHaveBeenCalled(); // primed batch is skipped
    await vi.advanceTimersByTimeAsync(2000);
    expect(cb).toHaveBeenCalledWith({ message: "new" });
    un();
  });
});
