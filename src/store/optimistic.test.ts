import { beforeEach, describe, expect, it } from "vitest";
import { useTorrents } from "./torrents";
import type { Snapshot, TorrentDto } from "../ipc/types";

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

function dto(hash: string, status: TorrentDto["status"]): TorrentDto {
  return {
    hash,
    name: hash,
    size: 100,
    bytesDone: 0,
    percent: 0,
    status,
    statusMsg: "",
    errorKind: "",
    seedsConnected: 0,
    peersConnected: 0,
    seedsSwarm: 0,
    peersSwarm: 0,
    downRate: 0,
    upRate: 0,
    etaSeconds: null,
    ratio: 0,
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

function snapshot(torrents: TorrentDto[]): Snapshot {
  return { revision: 1, torrents, globals: G, connection: C };
}

describe("optimistic transport actions (WC9-S2)", () => {
  beforeEach(() => {
    useTorrents.setState({
      torrents: [],
      globals: G,
      connection: C,
      optimistic: new Map(),
      revision: 0,
    });
  });

  it("shows the expected status before the daemon confirms it", () => {
    useTorrents.setState({ torrents: [dto("A", "paused")] });
    useTorrents.getState().markOptimistic({ A: "downloading" });
    expect(useTorrents.getState().torrents[0].status).toBe("downloading");
  });

  it("keeps the override while the daemon still reports the old state", () => {
    useTorrents.setState({ torrents: [dto("A", "paused")] });
    useTorrents.getState().markOptimistic({ A: "downloading" });
    // The daemon has not caught up yet.
    useTorrents.getState().applySnapshot(snapshot([dto("A", "paused")]));
    expect(useTorrents.getState().torrents[0].status).toBe("downloading");
  });

  it("drops the override the moment the daemon agrees", () => {
    useTorrents.setState({ torrents: [dto("A", "paused")] });
    useTorrents.getState().markOptimistic({ A: "downloading" });
    useTorrents.getState().applySnapshot(snapshot([dto("A", "downloading")]));
    expect(useTorrents.getState().optimistic.size).toBe(0);
    expect(useTorrents.getState().torrents[0].status).toBe("downloading");
  });

  it("reverts to the daemon's truth when the RPC failed", () => {
    useTorrents.setState({ torrents: [dto("A", "paused")] });
    useTorrents.getState().markOptimistic({ A: "downloading" });
    useTorrents.getState().clearOptimistic(["A"]);
    useTorrents.getState().applySnapshot(snapshot([dto("A", "paused")]));
    expect(useTorrents.getState().torrents[0].status).toBe("paused");
  });

  it("drops an override for a torrent that is gone", () => {
    useTorrents.setState({ torrents: [dto("A", "paused")] });
    useTorrents.getState().markOptimistic({ A: "downloading" });
    useTorrents.getState().applySnapshot(snapshot([]));
    expect(useTorrents.getState().optimistic.size).toBe(0);
  });
});
