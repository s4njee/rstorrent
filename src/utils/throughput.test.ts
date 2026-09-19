import { describe, expect, it } from "vitest";
import { gridLines, throughputGeometry } from "./throughput";

describe("throughputGeometry", () => {
  it("scales both series against one shared peak", () => {
    const geometry = throughputGeometry(
      [
        { down: 100, up: 0 },
        { down: 0, up: 200 },
      ],
      100,
      50,
    );
    expect(geometry.peak).toBe(200);
    // The download peak sits halfway up (100/200); upload is at the baseline.
    expect(geometry.downLine).toBe("M0,25 L100,50");
    expect(geometry.upLine).toBe("M0,50 L100,0");
  });

  it("closes the download area to the baseline", () => {
    const geometry = throughputGeometry(
      [
        { down: 10, up: 0 },
        { down: 10, up: 0 },
      ],
      100,
      50,
    );
    expect(geometry.downArea.startsWith("M0,50 L")).toBe(true);
    expect(geometry.downArea.endsWith("L100,50 Z")).toBe(true);
  });

  it("draws a flat baseline for an empty window rather than an empty path", () => {
    const geometry = throughputGeometry([], 100, 50);
    expect(geometry.peak).toBe(0);
    expect(geometry.downLine).toBe("M0,50 L100,50");
    expect(geometry.downArea).toBe("");
  });

  it("handles a single sample without dividing by zero", () => {
    const geometry = throughputGeometry([{ down: 5, up: 5 }], 100, 50);
    expect(geometry.peak).toBe(5);
    expect(geometry.downLine).toBe("M0,0");
  });

  it("treats an all-zero window as flat, not NaN", () => {
    const geometry = throughputGeometry(
      [
        { down: 0, up: 0 },
        { down: 0, up: 0 },
      ],
      100,
      50,
    );
    expect(geometry.peak).toBe(0);
    expect(geometry.downLine).toBe("M0,50 L100,50");
  });
});

describe("gridLines", () => {
  it("spaces the requested number of lines evenly inside the height", () => {
    expect(gridLines(3, 120)).toEqual([30, 60, 90]);
    expect(gridLines(0, 120)).toEqual([]);
  });
});
