// The detail panel's pure helpers: which torrent the panel describes, how a
// peer's address splits, and the tracker status vocabulary.
import { describe, expect, it } from "vitest";
import { focusedHashOf, splitAddress, trackerStatusWord } from "./DetailTabs";
import type { TrackerRow } from "../../ipc/types";

function tracker(extra: Partial<TrackerRow> = {}): TrackerRow {
  return {
    index: 0,
    url: "http://bttracker.debian.org:6969/announce",
    enabled: true,
    status: "working",
    seeds: 38,
    leeches: 112,
    kind: "http",
    nextAnnounce: 0,
    lastAnnounce: 0,
    ...extra,
  };
}

describe("focusedHashOf", () => {
  it("has no subject without a selection", () => {
    expect(focusedHashOf(new Set(), null)).toBeNull();
    expect(focusedHashOf(new Set(), "A")).toBeNull();
  });

  it("keeps the anchor a multi-selection was built around", () => {
    // The design keeps showing the most recently clicked row.
    expect(focusedHashOf(new Set(["A", "B", "C"]), "C")).toBe("C");
  });

  it("stands in with the first selected row when the anchor is deselected", () => {
    // A toggle removed the anchor from the set: the panel follows what is left.
    expect(focusedHashOf(new Set(["B", "C"]), "A")).toBe("B");
  });

  it("follows the only selected row whether or not it is the anchor", () => {
    expect(focusedHashOf(new Set(["B"]), "B")).toBe("B");
    expect(focusedHashOf(new Set(["B"]), null)).toBe("B");
  });
});

describe("splitAddress", () => {
  it("splits the host and port rtorrent reports as one address", () => {
    expect(splitAddress("185.21.104.7:51413")).toEqual({
      ip: "185.21.104.7",
      port: "51413",
    });
  });

  it("dashes the port of a bare address rather than mangling it", () => {
    expect(splitAddress("94.140.8.221")).toEqual({
      ip: "94.140.8.221",
      port: null,
    });
  });

  it("reads a bracketed IPv6 address's port off the last colon", () => {
    expect(splitAddress("[2001:db8::1]:6881")).toEqual({
      ip: "2001:db8::1",
      port: "6881",
    });
  });

  it("treats an unbracketed IPv6 address as all host", () => {
    expect(splitAddress("2001:db8::1")).toEqual({
      ip: "2001:db8::1",
      port: null,
    });
    expect(splitAddress("::1")).toEqual({ ip: "::1", port: null });
  });

  it("does not read a trailing non-numeric field as a port", () => {
    expect(splitAddress("host.example")).toEqual({
      ip: "host.example",
      port: null,
    });
  });

  it("shows a dash for an address the daemon left empty", () => {
    expect(splitAddress("   ")).toEqual({ ip: "—", port: null });
  });
});

describe("trackerStatusWord", () => {
  it("says disabled outranks a status the daemon has stopped updating", () => {
    expect(
      trackerStatusWord(tracker({ enabled: false, status: "error" })),
    ).toBe("disabled");
  });

  it("keeps the daemon's word for a working or updating tracker", () => {
    expect(trackerStatusWord(tracker({ status: "working" }))).toBe("working");
    expect(trackerStatusWord(tracker({ status: "updating" }))).toBe("updating");
  });

  it("folds the three spellings of a failure into one word", () => {
    expect(trackerStatusWord(tracker({ status: "error" }))).toBe("timed out");
    expect(trackerStatusWord(tracker({ status: "timeout" }))).toBe("timed out");
    expect(trackerStatusWord(tracker({ status: "timed out" }))).toBe(
      "timed out",
    );
  });

  it("passes through a status this build does not know", () => {
    expect(trackerStatusWord(tracker({ status: "queued" }))).toBe("queued");
    expect(trackerStatusWord(tracker({ status: "" }))).toBe("—");
  });
});
