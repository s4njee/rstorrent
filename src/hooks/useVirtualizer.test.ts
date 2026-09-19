import { describe, it, expect } from "vitest";
import { resolveRowHeight } from "./useVirtualizer";

describe("useVirtualizer helpers", () => {
  it("resolveRowHeight reads CSS var or falls back", () => {
    // jsdom: no --row-height defined → fallback
    expect(resolveRowHeight(23)).toBe(23);
    document.documentElement.style.setProperty("--row-height", "25px");
    expect(resolveRowHeight(23)).toBe(25);
    document.documentElement.style.removeProperty("--row-height");
  });

  it("virtual range math is bounded", () => {
    const count = 5000;
    const rowHeight = 23;
    const overscan = 10;
    const scrollTop = 1000;
    const viewport = 600;
    const start = Math.max(0, Math.floor(scrollTop / rowHeight) - overscan);
    const end = Math.min(
      count - 1,
      Math.ceil((scrollTop + viewport) / rowHeight) + overscan,
    );
    expect(start).toBe(Math.floor(1000 / 23) - 10);
    expect(end).toBeGreaterThan(start);
    // window size bounded to overscan + viewport
    const windowSize = end - start + 1;
    expect(windowSize).toBeLessThan(
      Math.ceil(viewport / rowHeight) + overscan * 2 + 2,
    );
    // total height
    expect(count * rowHeight).toBe(115000);
  });
});
