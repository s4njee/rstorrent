// Formatter tests — assert the exact strings shown in the design reference so
// the table renders byte-for-byte like the mockup.
import { describe, it, expect } from "vitest";
import {
  formatBytes,
  formatRate,
  formatDuration,
  formatEta,
  formatRatio,
  formatDownCell,
  formatUpCell,
} from "./format";

const GIB = 1_073_741_824;
const MIB = 1_048_576;
const KIB = 1_024;

describe("formatBytes", () => {
  it("matches design size strings", () => {
    expect(formatBytes(5.8 * GIB)).toBe("5.8 GiB");
    expect(formatBytes(631 * MIB)).toBe("631 MiB");
    expect(formatBytes(2.3 * GIB)).toBe("2.3 GiB");
    expect(formatBytes(1.1 * GIB)).toBe("1.1 GiB");
    expect(formatBytes(412 * GIB)).toBe("412 GiB");
  });
  it("handles zero and bytes", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(1400)).toBe("1.4 KiB");
  });
});

describe("formatRate", () => {
  it("matches design rate strings", () => {
    expect(formatRate(8.4 * MIB)).toBe("8.4 MiB/s");
    expect(formatRate(620 * KIB)).toBe("620 KiB/s");
    expect(formatRate(1.2 * MIB)).toBe("1.2 MiB/s");
    expect(formatRate(214 * KIB)).toBe("214 KiB/s");
    expect(formatRate(9.5 * MIB)).toBe("9.5 MiB/s");
  });
});

describe("formatDuration / formatEta", () => {
  it("matches design ETA strings", () => {
    expect(formatDuration(252)).toBe("4m12s");
    expect(formatDuration(820)).toBe("13m40s");
    expect(formatDuration(148)).toBe("2m28s");
    expect(formatDuration(45)).toBe("45s");
    expect(formatDuration(3723)).toBe("1h2m");
  });
  it("chooses infinity/dash by status when eta is null", () => {
    expect(formatEta(null, "seeding")).toBe("∞");
    expect(formatEta(null, "stalled")).toBe("∞");
    expect(formatEta(null, "paused")).toBe("—");
    expect(formatEta(null, "error")).toBe("—");
    expect(formatEta(252, "downloading")).toBe("4m12s");
  });
});

describe("rate cells", () => {
  it("down cell shows 0 B/s only while (down)loading, else dash", () => {
    expect(formatDownCell(8.4 * MIB, "downloading")).toBe("8.4 MiB/s");
    expect(formatDownCell(0, "stalled")).toBe("0 B/s");
    expect(formatDownCell(0, "seeding")).toBe("—");
    expect(formatDownCell(0, "paused")).toBe("—");
  });
  it("up cell shows rate or dash", () => {
    expect(formatUpCell(1.2 * MIB)).toBe("1.2 MiB/s");
    expect(formatUpCell(0)).toBe("—");
  });
});

describe("formatRatio", () => {
  it("two decimals", () => {
    expect(formatRatio(2.41)).toBe("2.41");
    expect(formatRatio(0.19)).toBe("0.19");
  });
});
