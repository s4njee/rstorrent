import { describe, expect, it } from "vitest";

import {
  effectiveWindows,
  evaluate,
  formatTransition,
  importLegacy,
  nextChange,
} from "./schedule";

describe("importLegacy", () => {
  it("needs an enabled, non-empty window", () => {
    expect(importLegacy(true, 1320, 360, [1, 2], 100, 50)).toHaveLength(1);
    expect(importLegacy(false, 1320, 360, [], 100, 50)).toHaveLength(0);
    expect(importLegacy(true, 600, 600, [], 100, 50)).toHaveLength(0);
  });
});

describe("evaluate", () => {
  const night = [
    { days: [], startMin: 1320, endMin: 360, pause: false, downKb: 100, upKb: 50 },
  ];
  it("opens outside windows and limits inside", () => {
    expect(evaluate(night, null, null, 3, 720, 0)).toEqual({ kind: "open" });
    expect(evaluate(night, null, null, 3, 1400, 0)).toEqual({
      kind: "limited",
      downKb: 100,
      upKb: 50,
    });
  });

  it("orders override, pause, limit, manual", () => {
    const grid = [
      ...night,
      { days: [], startMin: 600, endMin: 660, pause: true, downKb: 0, upKb: 0 },
    ];
    expect(evaluate(grid, null, null, 2, 630, 0)).toEqual({ kind: "paused" });
    expect(
      evaluate(
        grid,
        { pause: false, downKb: 10, upKb: 5, untilMs: 1000000 },
        null,
        2,
        630,
        0,
      ),
    ).toEqual({ kind: "limited", downKb: 10, upKb: 5 });
    expect(evaluate(grid, null, { downKb: 200, upKb: 100 }, 2, 700, 0)).toEqual({
      kind: "limited",
      downKb: 200,
      upKb: 100,
    });
    expect(evaluate([], null, { downKb: 200, upKb: 100 }, 2, 700, 0)).toEqual({
      kind: "limited",
      downKb: 200,
      upKb: 100,
    });
  });
});

describe("nextChange", () => {
  const night = [
    { days: [], startMin: 1320, endMin: 360, pause: false, downKb: 100, upKb: 50 },
  ];
  it("names the coming boundary", () => {
    expect(nextChange(night, null, null, 3, 720, 0)).toEqual({
      inMinutes: 600,
      weekday: 3,
      minute: 1320,
      to: "limited",
    });
    expect(nextChange(night, null, null, 3, 1380, 0)).toEqual({
      inMinutes: 420,
      weekday: 4,
      minute: 360,
      to: "open",
    });
  });

  it("returns null on an empty grid", () => {
    expect(nextChange([], null, null, 3, 720, 0)).toBeNull();
  });

  it("formats for the preview line", () => {
    expect(
      formatTransition({ inMinutes: 372, weekday: 1, minute: 1380, to: "paused" }),
    ).toBe("Mon 23:00 → paused (in 6h 12m)");
  });
});

describe("effectiveWindows", () => {
  it("prefers the grid and falls back to legacy", () => {
    const grid = [{ days: [], startMin: 0, endMin: 60, pause: false, downKb: 1, upKb: 1 }];
    const legacy = {
      enabled: true,
      startMin: 1320,
      endMin: 360,
      days: [] as number[],
      downKb: 100,
      upKb: 50,
    };
    expect(
      effectiveWindows({ windows: grid, tempOverride: null }, legacy),
    ).toEqual(grid);
    expect(
      effectiveWindows({ windows: [], tempOverride: null }, legacy),
    ).toHaveLength(1);
  });
});
