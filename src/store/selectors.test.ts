// Tests for the visible-list and sidebar-count derivations, using a small
// fixture that mirrors the design's status mix.
import { describe, it, expect } from "vitest";
import { selectVisible, sidebarCounts } from "./selectors";
import { reconcile } from "./torrents";
import type { TorrentDto, Status } from "../ipc/types";

function mk(
  hash: string,
  name: string,
  status: Status,
  percent: number,
  label: string,
  tracker: string,
  size = 1000,
  downRate = 0,
): TorrentDto {
  return {
    hash,
    name,
    size,
    bytesDone: (size * percent) / 100,
    percent,
    status,
    statusMsg: "",
    seedsConnected: 0,
    peersConnected: 0,
    seedsSwarm: 0,
    peersSwarm: 0,
    downRate,
    upRate: 0,
    etaSeconds: null,
    ratio: 0,
    label,
    trackerHost: tracker,
    savePath: "",
    priority: 2,
    isPrivate: false,
  };
}

const fixture: TorrentDto[] = [
  mk("A", "ubuntu.iso", "seeding", 100, "linux-iso", "torrent.ubuntu.com"),
  mk("B", "debian.iso", "seeding", 100, "linux-iso", "bttracker.debian.org"),
  mk(
    "C",
    "fedora.iso",
    "downloading",
    67,
    "linux-iso",
    "linuxtracker.org",
    1000,
    500,
  ),
  mk(
    "D",
    "sintel.mkv",
    "downloading",
    91,
    "video",
    "tracker.blender.org",
    1000,
    200,
  ),
  mk("E", "mint.iso", "paused", 45, "linux-iso", "linuxtracker.org"),
  mk("F", "raspios.img", "paused", 100, "sbc", "downloads.raspberrypi.org"),
  mk("G", "suse.iso", "stalled", 12, "linux-iso", "linuxtracker.org"),
  mk("H", "cosmos.mkv", "error", 66, "video", "tracker.blender.org"),
];

describe("sidebarCounts", () => {
  it("counts overlapping status predicates globally", () => {
    const c = sidebarCounts(fixture);
    expect(c.status.all).toBe(8);
    expect(c.status.downloading).toBe(2);
    expect(c.status.seeding).toBe(2);
    expect(c.status.paused).toBe(2);
    expect(c.status.stalled).toBe(1);
    expect(c.status.error).toBe(1);
    // completed = anything at 100% (superset: 2 seeding + 1 paused raspios).
    expect(c.status.completed).toBe(3);
  });
  it("groups labels and trackers", () => {
    const c = sidebarCounts(fixture);
    expect(c.labels.find((l) => l.value === "linux-iso")?.count).toBe(5);
    expect(c.labels.find((l) => l.value === "video")?.count).toBe(2);
    expect(c.trackers.find((t) => t.value === "linuxtracker.org")?.count).toBe(
      3,
    );
  });
});

describe("selectVisible", () => {
  it("filters by status", () => {
    const rows = selectVisible(
      fixture,
      { type: "status", value: "downloading" },
      "",
      "name",
      "asc",
    );
    expect(rows.map((r) => r.hash)).toEqual(["C", "D"]);
  });
  it("completed filter is a superset of 100% rows", () => {
    const rows = selectVisible(
      fixture,
      { type: "status", value: "completed" },
      "",
      "name",
      "asc",
    );
    expect(rows.map((r) => r.hash).sort()).toEqual(["A", "B", "F"]);
  });
  it("search matches name/label/tracker and ANDs with filter", () => {
    const rows = selectVisible(fixture, null, "fedora", "name", "asc");
    expect(rows.map((r) => r.hash)).toEqual(["C"]);
    const both = selectVisible(
      fixture,
      { type: "label", value: "linux-iso" },
      "iso",
      "name",
      "asc",
    );
    // Every linux-iso row also matches the search "iso" via its label, so all
    // five come through (sorted by name).
    expect(both.map((r) => r.hash)).toEqual(["B", "C", "E", "G", "A"]);
  });
  it("sorts numerically by download rate descending", () => {
    const rows = selectVisible(fixture, null, "", "downRate", "desc");
    expect(rows[0].hash).toBe("C"); // 500 > 200 > 0...
  });
});

describe("reconcile", () => {
  it("preserves object identity for unchanged rows", () => {
    const prev = [...fixture];
    const next = fixture.map((t) => ({ ...t })); // fresh objects, same values
    const merged = reconcile(prev, next);
    // Unchanged rows keep the PREVIOUS object reference.
    expect(merged[0]).toBe(prev[0]);
  });
  it("uses the new object when a field changed", () => {
    const prev = [...fixture];
    const next = fixture.map((t) => ({ ...t }));
    next[2] = { ...next[2], downRate: 999 };
    const merged = reconcile(prev, next);
    expect(merged[2]).toBe(next[2]);
    expect(merged[0]).toBe(prev[0]);
  });
});
