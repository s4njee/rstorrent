// The disk footer's arithmetic and its pressure levels: the bar must warn before
// a download fails, not after.
import { describe, expect, it } from "vitest";
import {
  DISK_ERROR_AT,
  DISK_WARN_AT,
  diskPressure,
  usedFraction,
} from "./DiskCard";

describe("usedFraction", () => {
  it("is the complement of the free fraction", () => {
    expect(usedFraction(2, 8)).toBeCloseTo(0.75);
  });

  it("is unknown without both figures", () => {
    // A remote daemon we cannot stat gets no invented number.
    expect(usedFraction(null, 8)).toBeNull();
    expect(usedFraction(2, null)).toBeNull();
    expect(usedFraction(2, 0)).toBeNull();
  });

  it("clamps nonsense into range", () => {
    // Free space larger than the volume, or negative: still a sane bar.
    expect(usedFraction(10, 8)).toBe(0);
    expect(usedFraction(-1, 8)).toBe(1);
  });
});

describe("diskPressure", () => {
  it("is normal while there is room", () => {
    expect(diskPressure(0)).toBe("normal");
    expect(diskPressure(0.5)).toBe("normal");
    expect(diskPressure(DISK_WARN_AT - 0.001)).toBe("normal");
  });

  it("warns at the warn threshold and errors at the error threshold", () => {
    expect(diskPressure(DISK_WARN_AT)).toBe("warn");
    expect(diskPressure(DISK_ERROR_AT - 0.001)).toBe("warn");
    expect(diskPressure(DISK_ERROR_AT)).toBe("full");
    expect(diskPressure(1)).toBe("full");
  });

  it("keeps the thresholds in the design's order", () => {
    expect(DISK_WARN_AT).toBeLessThan(DISK_ERROR_AT);
  });
});
