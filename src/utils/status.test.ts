// The status vocabulary (design frame 1a): the table-driven mapping from
// rtorrent's raw flags to the word and tone a row shows.
import { describe, expect, it } from "vitest";
import type { Status, TorrentDto } from "../ipc/types";
import { ratioTone, seedsPeersCell, statusWord } from "./status";

/** Minimal input for the mapping; every field a viewer must supply. */
function input(
  status: Status,
  extra: Partial<{ percent: number; errorKind: string; isOpen: boolean }> = {},
) {
  return {
    status,
    percent: extra.percent ?? 0,
    errorKind: extra.errorKind ?? "",
    isOpen: extra.isOpen,
  };
}

/** Only the fields the cells read. */
function torrent(extra: Partial<TorrentDto> = {}): TorrentDto {
  return {
    hash: "H",
    name: "n",
    size: 0,
    bytesDone: 0,
    percent: 0,
    status: "downloading",
    statusMsg: "",
    errorKind: "",
    seedsConnected: 0,
    peersConnected: 0,
    seedsSwarm: 0,
    peersSwarm: 0,
    downRate: 0,
    upRate: 0,
    etaSeconds: null,
    ratio: 0,
    label: "",
    trackerHost: "",
    savePath: "",
    priority: 0,
    isPrivate: false,
    ...extra,
  } as TorrentDto;
}

describe("statusWord", () => {
  it("maps every rtorrent state to the design's word", () => {
    const cases: Array<[Status, string, string]> = [
      ["downloading", "Downloading", "active"],
      ["seeding", "Seeding", "seeding"],
      ["completed", "Seeding", "seeding"],
      ["stalled", "Stalled", "warn"],
      ["error", "Error", "error"],
    ];
    for (const [status, text, tone] of cases) {
      expect(statusWord(input(status))).toEqual({ text, tone });
    }
  });

  it("carries the verification sweep while checking", () => {
    // rtorrent reports hashed chunks; the DTO's percent holds that sweep.
    expect(statusWord(input("checking", { percent: 42.4 }))).toEqual({
      text: "Checking 42%",
      tone: "active",
    });
  });

  it("separates a tracker failure from a storage one", () => {
    for (const kind of ["unregistered", "tracker_timeout", "tracker_error"]) {
      expect(statusWord(input("error", { errorKind: kind })).text).toBe(
        "Tracker error",
      );
    }
    for (const kind of [
      "missing_files",
      "no_space",
      "permission",
      "disk_error",
    ]) {
      expect(statusWord(input("error", { errorKind: kind })).text).toBe(
        "Error",
      );
    }
  });

  it("tells a stopped torrent from a queued one", () => {
    // Both are `paused` to rtorrent; only `d.is_open` separates them.
    expect(statusWord(input("paused", { isOpen: false })).text).toBe("Stopped");
    expect(statusWord(input("paused", { isOpen: true })).text).toBe("Queued");
    // An older snapshot without the flag reads as stopped rather than guessing.
    expect(statusWord(input("paused")).text).toBe("Stopped");
    // Neither is active, so neither takes the accent.
    expect(statusWord(input("paused", { isOpen: true })).tone).toBe("idle");
  });
});

describe("seedsPeersCell", () => {
  it("reads connected seeds then connected peers while downloading", () => {
    expect(
      seedsPeersCell(
        torrent({
          status: "downloading",
          seedsConnected: 38,
          peersConnected: 112,
        }),
      ),
    ).toBe("38 / 112");
  });

  it("drops the seed count while seeding — there is nothing to download from", () => {
    expect(
      seedsPeersCell(
        torrent({ status: "seeding", seedsConnected: 12, peersConnected: 44 }),
      ),
    ).toBe("— / 44");
    expect(
      seedsPeersCell(
        torrent({ status: "completed", seedsConnected: 3, peersConnected: 9 }),
      ),
    ).toBe("— / 9");
  });

  it("shows zero peers rather than a dash when nothing is connected", () => {
    expect(
      seedsPeersCell(
        torrent({
          status: "downloading",
          seedsConnected: 0,
          peersConnected: 0,
        }),
      ),
    ).toBe("0 / 0");
  });
});

describe("ratioTone", () => {
  it("dims a ratio below 2.00", () => {
    expect(ratioTone(2.41)).toBe("good");
    expect(ratioTone(2)).toBe("good");
    expect(ratioTone(1.99)).toBe("poor");
    expect(ratioTone(0)).toBe("poor");
  });
});
