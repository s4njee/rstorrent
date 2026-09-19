import { beforeEach, describe, expect, it } from "vitest";
import { MAX_TRANSFER_POINTS, useTransferHistory } from "./transferHistory";
import type { GlobalStats } from "../ipc/types";

const globals = (downRate: number, upRate: number): GlobalStats => ({
  downRate,
  upRate,
  downRateLimit: 0,
  upRateLimit: 0,
  dhtNodes: 0,
  freeSpace: null,
  diskSize: null,
  turtleActive: false,
});

describe("global transfer history", () => {
  beforeEach(() => useTransferHistory.getState().clear());

  it("records timestamped download and upload samples", () => {
    useTransferHistory.getState().record(globals(100, 25), 1_000);
    useTransferHistory.getState().record(globals(200, 50), 2_000);

    expect(useTransferHistory.getState().points).toEqual([
      { time: 1_000, down: 100, up: 25 },
      { time: 2_000, down: 200, up: 50 },
    ]);
  });

  it("normalizes invalid rates and keeps a bounded ring buffer", () => {
    const store = useTransferHistory.getState();
    store.record(globals(Number.NaN, -1), 1);
    for (let i = 1; i < MAX_TRANSFER_POINTS + 3; i += 1) {
      store.record(globals(i, i), i);
    }

    const points = useTransferHistory.getState().points;
    expect(points).toHaveLength(MAX_TRANSFER_POINTS);
    expect(points[0].time).toBe(3);
    expect(points[points.length - 1]).toEqual({
      time: MAX_TRANSFER_POINTS + 2,
      down: MAX_TRANSFER_POINTS + 2,
      up: MAX_TRANSFER_POINTS + 2,
    });
  });
});
