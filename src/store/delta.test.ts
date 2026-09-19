import { describe, it, expect, beforeEach } from "vitest";
import { useTorrents } from "./torrents";
import type { Snapshot, SnapshotDelta } from "../ipc/types";

const G = {
  downRate: 0,
  upRate: 0,
  downRateLimit: 0,
  upRateLimit: 0,
  dhtNodes: 0,
  freeSpace: null,
  diskSize: null,
  turtleActive: false,
};
const C = {
  phase: "connected" as const,
  endpoint: "test",
  daemonVersion: "0.9.8",
  error: null,
  retryInSeconds: null,
};

function dto(hash: string, name: string): import("../ipc/types").TorrentDto {
  return {
    hash,
    name,
    size: 100,
    bytesDone: 100,
    percent: 100,
    status: "seeding",
    statusMsg: "",
    errorKind: "",
    seedsConnected: 0,
    peersConnected: 0,
    seedsSwarm: 0,
    peersSwarm: 0,
    downRate: 0,
    upRate: 0,
    etaSeconds: null,
    ratio: 1,
    label: "",
    trackerHost: "",
    savePath: "",
    priority: 0,
    isPrivate: false,
    throttleName: "",
    downRateLimit: null,
    upRateLimit: null,
    startedAt: 0,
    finishedAt: 0,
    views: [],
  };
}

describe("delta apply (FND-02)", () => {
  beforeEach(() => {
    useTorrents.setState({
      torrents: [],
      globals: G,
      connection: C,
      revision: 0,
    });
  });

  it("applies added/updated/removed and bumps revision", () => {
    const snap: Snapshot = {
      revision: 1,
      torrents: [dto("A", "a"), dto("B", "b")],
      globals: G,
      connection: C,
    };
    useTorrents.getState().applySnapshot(snap);
    expect(useTorrents.getState().revision).toBe(1);

    const delta: SnapshotDelta = {
      revision: 2,
      baseRevision: 1,
      added: [dto("C", "c")],
      updated: [{ ...dto("B", "b2") }],
      removed: ["A"],
      globals: G,
      connection: C,
    };
    const ok = useTorrents.getState().applyDelta(delta);
    expect(ok).toBe(true);
    expect(useTorrents.getState().revision).toBe(2);
    const hashes = useTorrents
      .getState()
      .torrents.map((t) => t.hash)
      .sort();
    expect(hashes).toEqual(["B", "C"]);
    expect(
      useTorrents.getState().torrents.find((t) => t.hash === "B")!.name,
    ).toBe("b2");
  });

  it("rejects delta with wrong baseRevision (missed revision)", () => {
    const snap: Snapshot = {
      revision: 5,
      torrents: [dto("A", "a")],
      globals: G,
      connection: C,
    };
    useTorrents.getState().applySnapshot(snap);
    const delta: SnapshotDelta = {
      revision: 6,
      baseRevision: 4, // missed 5
      added: [],
      updated: [],
      removed: [],
      globals: G,
      connection: C,
    };
    const ok = useTorrents.getState().applyDelta(delta);
    expect(ok).toBe(false);
    expect(useTorrents.getState().revision).toBe(5); // unchanged
  });

  it("self-heals via full snapshot after missed delta", async () => {
    const snap1: Snapshot = {
      revision: 1,
      torrents: [dto("A", "a")],
      globals: G,
      connection: C,
    };
    useTorrents.getState().applySnapshot(snap1);
    // Missed delta for 2, next delta base is 2 but we are at 1 => reject
    const badDelta: SnapshotDelta = {
      revision: 3,
      baseRevision: 2,
      added: [dto("B", "b")],
      updated: [],
      removed: [],
      globals: G,
      connection: C,
    };
    expect(useTorrents.getState().applyDelta(badDelta)).toBe(false);
    // Heal with full snapshot at 3
    const snap2: Snapshot = {
      revision: 3,
      torrents: [dto("A", "a"), dto("B", "b")],
      globals: G,
      connection: C,
    };
    useTorrents.getState().applySnapshot(snap2);
    expect(useTorrents.getState().revision).toBe(3);
    expect(useTorrents.getState().torrents.length).toBe(2);
  });
});
