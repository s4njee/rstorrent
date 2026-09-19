// The notice stack's rules: a keyed notice is raised once, errors stay until
// dismissed, and the stack stays bounded.
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  DEFAULT_TIMEOUT_MS,
  MAX_NOTICES,
  resetNoticeKeys,
  useNotices,
} from "./notices";

beforeEach(() => {
  useNotices.setState({ notices: [] });
  resetNoticeKeys();
  vi.useRealTimers();
});

describe("raising notices", () => {
  it("keeps an error until it is dismissed", () => {
    useNotices.getState().push("error", "Could not start the torrent");
    const [notice] = useNotices.getState().notices;
    expect(notice.level).toBe("error");
    expect(notice.timeoutMs).toBeUndefined();
  });

  it("gives an informational notice a lifetime", () => {
    useNotices.getState().push("info", "magnet link copied");
    expect(useNotices.getState().notices[0].timeoutMs).toBe(DEFAULT_TIMEOUT_MS);
  });

  it("carries a detail line for the underlying error", () => {
    useNotices
      .getState()
      .push("error", "Remove failed", { detail: "rtorrent fault 3" });
    expect(useNotices.getState().notices[0].detail).toBe("rtorrent fault 3");
  });

  it("raises a keyed notice once per session", () => {
    // The connection banner must not stack one toast per failed poll.
    const first = useNotices
      .getState()
      .push("warn", "Lost connection", { key: "lost" });
    const second = useNotices
      .getState()
      .push("warn", "Lost connection", { key: "lost" });
    expect(first).not.toBe(0);
    expect(second).toBe(0);
    expect(useNotices.getState().notices).toHaveLength(1);
  });

  it("keeps the stack bounded, dropping the oldest", () => {
    for (let index = 0; index < MAX_NOTICES + 3; index += 1) {
      useNotices
        .getState()
        .push("info", `notice ${index}`, { timeoutMs: undefined });
    }
    const notices = useNotices.getState().notices;
    expect(notices).toHaveLength(MAX_NOTICES);
    expect(notices[0].message).toBe("notice 3");
  });
});

describe("dismissing", () => {
  it("drops one by id and all at once", () => {
    const first = useNotices
      .getState()
      .push("info", "one", { timeoutMs: undefined });
    useNotices.getState().push("info", "two", { timeoutMs: undefined });

    useNotices.getState().dismiss(first);
    expect(useNotices.getState().notices.map((n) => n.message)).toEqual([
      "two",
    ]);

    useNotices.getState().dismissAll();
    expect(useNotices.getState().notices).toEqual([]);
  });
});
