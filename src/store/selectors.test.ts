// Tests for the visible-list and sidebar-count derivations, using a small
// fixture that mirrors the design's status mix.
import { describe, it, expect } from "vitest";
import {
  selectionSummary,
  selectVisible,
  sidebarCounts,
  smartFilterCounts,
} from "./selectors";
import type { SmartFilter } from "./ui";
import { reconcile, smoothRates, type EmaState } from "./torrents";
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
    throttleName: "",
    downRateLimit: null,
    upRateLimit: null,
    startedAt: 0,
    finishedAt: 0,
    views: [],
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
  it("uses the new object when a per-torrent limit changes", () => {
    const prev = [...fixture];
    const limited = {
      ...fixture[0],
      throttleName: "rstorrent_1",
      downRateLimit: 512 * 1024,
      upRateLimit: 512 * 1024,
    };
    const merged = reconcile(prev, [limited, ...fixture.slice(1)]);
    expect(merged[0]).toBe(limited);
  });
});

describe("smoothRates (C6)", () => {
  const dl = (downRate: number, etaSeconds: number | null = 60) => ({
    ...mk("D", "cachyos.iso", "downloading", 50, "", "t.example", 1000),
    downRate,
    etaSeconds,
  });

  it("passes the first sample through unchanged", () => {
    const state: EmaState = new Map();
    const [t] = smoothRates(state, [dl(300)]);
    expect(t.downRate).toBe(300);
  });

  it("damps a one-tick spike instead of displaying it", () => {
    const state: EmaState = new Map();
    smoothRates(state, [dl(300)]);
    const [t] = smoothRates(state, [dl(900)]); // 3× spike for one poll
    // EMA with α=1/3: 300 + (900-300)/3 = 500, well short of the spike.
    expect(t.downRate).toBe(500);
  });

  it("snaps to zero immediately when a torrent stops", () => {
    const state: EmaState = new Map();
    smoothRates(state, [dl(900)]);
    const [t] = smoothRates(state, [dl(0)]);
    expect(t.downRate).toBe(0); // no multi-second fade-out
  });

  it("recomputes ETA from the smoothed rate but never invents one", () => {
    const state: EmaState = new Map();
    smoothRates(state, [dl(300)]);
    const [t] = smoothRates(state, [dl(900)]); // smoothed = 500, remaining = 500
    expect(t.etaSeconds).toBe(1);
    // null means the backend said ∞/— (paused, seeding): stays null.
    const [p] = smoothRates(state, [dl(900, null)]);
    expect(p.etaSeconds).toBeNull();
  });

  it("returns the same object when smoothing changes nothing", () => {
    const state: EmaState = new Map();
    const row = { ...dl(0), etaSeconds: null };
    const [t] = smoothRates(state, [row]);
    expect(t).toBe(row); // identity preserved → reconcile can skip the row
  });

  it("drops EMA state for torrents that disappear", () => {
    const state: EmaState = new Map();
    smoothRates(state, [dl(300)]);
    expect(state.has("D")).toBe(true);
    smoothRates(state, []);
    expect(state.has("D")).toBe(false);
  });
});

describe("smart filters (C4)", () => {
  const isoText: SmartFilter = {
    id: "sf1",
    name: "stalled linux-isos",
    status: "stalled",
    label: "linux-iso",
  };

  it("ANDs every present criterion", () => {
    // G is the only stalled linux-iso; E is linux-iso but paused.
    const rows = selectVisible(
      fixture,
      { type: "smart", value: "sf1" },
      "",
      "name",
      "asc",
      [isoText],
    );
    expect(rows.map((r) => r.hash)).toEqual(["G"]);
  });

  it("leaves absent criteria unconstrained", () => {
    const labelOnly: SmartFilter = {
      id: "sf2",
      name: "isos",
      label: "linux-iso",
    };
    const rows = selectVisible(
      fixture,
      { type: "smart", value: "sf2" },
      "",
      "name",
      "asc",
      [labelOnly],
    );
    expect(rows.map((r) => r.hash).sort()).toEqual(["A", "B", "C", "E", "G"]);
  });

  it("combines a text criterion with the other dimensions", () => {
    const withText: SmartFilter = {
      id: "sf3",
      name: "seeding debian",
      status: "seeding",
      text: "debian",
    };
    const rows = selectVisible(
      fixture,
      { type: "smart", value: "sf3" },
      "",
      "name",
      "asc",
      [withText],
    );
    expect(rows.map((r) => r.hash)).toEqual(["B"]);
  });

  it("still ANDs the live search box on top of a smart filter", () => {
    const labelOnly: SmartFilter = {
      id: "sf2",
      name: "isos",
      label: "linux-iso",
    };
    const rows = selectVisible(
      fixture,
      { type: "smart", value: "sf2" },
      "fedora",
      "name",
      "asc",
      [labelOnly],
    );
    expect(rows.map((r) => r.hash)).toEqual(["C"]);
  });

  it("honours the completed superset inside criteria", () => {
    const done: SmartFilter = { id: "sf4", name: "done", status: "completed" };
    const rows = selectVisible(
      fixture,
      { type: "smart", value: "sf4" },
      "",
      "name",
      "asc",
      [done],
    );
    // 100% rows regardless of status: two seeding + the paused raspios.
    expect(rows.map((r) => r.hash).sort()).toEqual(["A", "B", "F"]);
  });

  it("shows everything for a dangling id rather than an empty table", () => {
    const rows = selectVisible(
      fixture,
      { type: "smart", value: "gone" },
      "",
      "name",
      "asc",
      [],
    );
    expect(rows).toHaveLength(fixture.length);
  });

  it("counts rows per saved filter over the unfiltered list", () => {
    const counts = smartFilterCounts(fixture, [
      isoText,
      { id: "sf2", name: "isos", label: "linux-iso" },
    ]);
    expect(counts).toEqual({ sf1: 1, sf2: 5 });
  });
});

describe("selectionSummary (C3)", () => {
  it("aggregates count, size and rates for the selected rows", () => {
    const s = selectionSummary(fixture, new Set(["C", "D"]));
    expect(s.count).toBe(2);
    expect(s.size).toBe(2000);
    expect(s.downRate).toBe(700); // 500 + 200
    expect(s.upRate).toBe(0);
    expect(s.paused).toBe(0);
  });

  it("counts paused rows so Resume/Pause can be judged", () => {
    expect(selectionSummary(fixture, new Set(["E", "F", "C"])).paused).toBe(2);
  });

  it("ignores hashes that no longer exist", () => {
    // A removal can land between snapshot and render.
    const s = selectionSummary(fixture, new Set(["A", "does-not-exist"]));
    expect(s.count).toBe(1);
  });

  it("is empty for an empty selection", () => {
    expect(selectionSummary(fixture, new Set())).toEqual({
      count: 0,
      size: 0,
      downRate: 0,
      upRate: 0,
      paused: 0,
    });
  });
});
