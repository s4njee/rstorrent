import { describe, expect, it } from "vitest";

import type { MoveStatus } from "../ipc/types";
import { activeMoves, baseName, movePercent, retryableMoves } from "./moves";

function move(over: Partial<MoveStatus>): MoveStatus {
  return {
    id: "A-1",
    hash: "A",
    name: "Show",
    src: "/incomplete/Show",
    dst: "/dl/Show",
    state: "pending",
    error: "",
    doneBytes: 0,
    totalBytes: 100,
    ...over,
  };
}

describe("move selectors", () => {
  it("splits live from retryable", () => {
    const moves = [
      move({ id: "1", state: "pending" }),
      move({ id: "2", state: "in_progress" }),
      move({ id: "3", state: "failed", error: "boom" }),
      move({ id: "4", state: "cancelled" }),
    ];
    expect(activeMoves(moves).map((m) => m.id)).toEqual(["1", "2"]);
    expect(retryableMoves(moves).map((m) => m.id)).toEqual(["3", "4"]);
  });

  it("percents clamp and go null without a total", () => {
    expect(movePercent(move({ doneBytes: 50, totalBytes: 100 }))).toBe(50);
    expect(movePercent(move({ doneBytes: 200, totalBytes: 100 }))).toBe(100);
    expect(movePercent(move({ doneBytes: 0, totalBytes: 0 }))).toBeNull();
  });

  it("basenames the pill label", () => {
    expect(baseName("/dl/Show.S01")).toBe("Show.S01");
    expect(baseName("Show.S01")).toBe("Show.S01");
    expect(baseName("/dl/Show.S01/")).toBe("Show.S01");
  });
});
