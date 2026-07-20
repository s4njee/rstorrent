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
  formatDate,
  formatCountdown,
  formatAgo,
} from "./format";

const GIB = 1_073_741_824;
const MIB = 1_048_576;
const KIB = 1_024;

describe("formatDate", () => {
  it("renders — for the unknown (0) timestamp", () => {
    expect(formatDate(0)).toBe("—");
  });
  it("drops the year within the current year, keeps it otherwise", () => {
    const now = new Date("2026-07-20T12:00:00Z");
    // A date earlier the same year: no year shown.
    expect(formatDate(Date.UTC(2026, 6, 17) / 1000, now)).not.toMatch(
      /26|2026/,
    );
    // A prior-year date: year present.
    expect(formatDate(Date.UTC(2025, 11, 30) / 1000, now)).toMatch(/25|2025/);
  });
});

describe("formatCountdown / formatAgo (tracker times)", () => {
  const now = 1_784_584_112_000; // fixed "now" in ms
  const nowSec = now / 1000;

  it("counts down to a future announce", () => {
    expect(formatCountdown(nowSec + 720, now)).toBe("in 12m0s");
  });
  it("shows — for an unset or overdue next announce", () => {
    expect(formatCountdown(0, now)).toBe("—");
    // rtorrent leaves next-announce in the past for a failing tracker.
    expect(formatCountdown(nowSec - 100, now)).toBe("—");
  });
  it("shows elapsed time since the last announce", () => {
    expect(formatAgo(nowSec - 240, now)).toBe("4m0s ago");
    expect(formatAgo(0, now)).toBe("—");
  });
});

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
