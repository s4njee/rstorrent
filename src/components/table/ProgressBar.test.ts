import { describe, expect, it } from "vitest";
import { clampPercent, fillColor, percentLabel } from "./ProgressBar";

describe("clampPercent", () => {
  it("keeps progress in the range used by CSS and ARIA", () => {
    expect(clampPercent(-1)).toBe(0);
    expect(clampPercent(42.5)).toBe(42.5);
    expect(clampPercent(101)).toBe(100);
  });

  it("turns non-finite daemon values into an empty bar", () => {
    expect(clampPercent(Number.NaN)).toBe(0);
    expect(clampPercent(Number.POSITIVE_INFINITY)).toBe(0);
  });
});

describe("the design's progress rule", () => {
  it("is blue below 100%, green at 100%, and never the accent", () => {
    expect(fillColor(61.4, "downloading")).toBe("var(--progress-active)");
    expect(fillColor(100, "seeding")).toBe("var(--progress-complete)");
    expect(fillColor(0, "downloading")).toBe("var(--progress-active)");
  });

  it("dims a stopped, incomplete bar so it reads as inert", () => {
    expect(fillColor(47, "paused")).toBe("var(--state-disabled)");
    // A stopped but *complete* torrent is still finished, so it stays green.
    expect(fillColor(100, "paused")).toBe("var(--progress-complete)");
  });

  it("keeps the error colour on an error row", () => {
    expect(fillColor(12.6, "error")).toBe("var(--status-error)");
  });

  it("labels 100% exactly and everything else to one decimal", () => {
    expect(percentLabel(100)).toBe("100%");
    expect(percentLabel(61.4)).toBe("61.4%");
    expect(percentLabel(0)).toBe("0.0%");
    expect(percentLabel(Number.NaN)).toBe("0.0%");
  });
});
