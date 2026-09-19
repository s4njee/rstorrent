// Sparkline geometry: shared scaling between the two series, and a baseline for
// a zero rate.
import { describe, expect, it } from "vitest";
import { sparkPeak, sparkPoints } from "./sparkline";

describe("sparkPoints", () => {
  it("has no points to draw for an empty series", () => {
    expect(sparkPoints([], 10, 26, 170)).toBe("");
  });

  it("puts a zero rate on the baseline and the peak at the top", () => {
    // height 26: the peak reaches y=0, a zero sits at y=26.
    expect(sparkPoints([0, 10], 10, 26, 170)).toBe("0.0,26.0 170.0,0.0");
  });

  it("scales both series against the shared peak", () => {
    // Half the peak sits halfway up — the point of a shared max.
    expect(sparkPoints([5], 10, 26, 170)).toBe("0.0,13.0");
  });

  it("draws a single sample at the left edge rather than dividing by zero", () => {
    expect(sparkPoints([10], 10, 26, 170)).toBe("0.0,0.0");
  });

  it("spreads samples evenly across the box", () => {
    const points = sparkPoints([0, 0, 0], 10, 26, 100).split(" ");
    expect(points).toEqual(["0.0,26.0", "50.0,26.0", "100.0,26.0"]);
  });

  it("treats a non-positive peak as flat rather than dividing by it", () => {
    expect(sparkPoints([0, 0], 0, 26, 170)).toBe("0.0,26.0 170.0,26.0");
  });
});

describe("sparkPeak", () => {
  it("floors at one so an idle app still scales", () => {
    expect(sparkPeak([])).toBe(1);
    expect(sparkPeak([{ down: 0, up: 0 }])).toBe(1);
  });

  it("takes the larger of the two directions", () => {
    expect(
      sparkPeak([
        { down: 100, up: 40 },
        { down: 20, up: 900 },
      ]),
    ).toBe(900);
  });
});
