// The facts rail as data: the handoff's eleven keys in its order, the arithmetic
// behind Uploaded, and our own additions at the end.
import { describe, expect, it } from "vitest";
import {
  addedText,
  extraFacts,
  primaryFacts,
  uploadedBytes,
  piecesText,
  type Fact,
} from "./facts";
import type { PieceInfo, TorrentDto } from "../ipc/types";

function torrent(extra: Partial<TorrentDto> = {}): TorrentDto {
  return {
    hash: "8F4A2C1D9E7B3F60",
    name: "debian-13.1.0-amd64-DVD-1",
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
    views: [],
    ...extra,
  } as TorrentDto;
}

function pieces(extra: Partial<PieceInfo> = {}): PieceInfo {
  return {
    sizeChunks: 0,
    chunkSize: 0,
    completedChunks: 0,
    bitfield: "",
    availability: "",
    ...extra,
  } as PieceInfo;
}

/** The value of a named fact, or undefined if the rail does not carry it. */
function valueOf(facts: Fact[], name: string): string | undefined {
  return facts.find(([key]) => key === name)?.[1];
}

describe("uploadedBytes", () => {
  it("inverts the ratio the daemon reports", () => {
    expect(
      uploadedBytes(torrent({ bytesDone: 2_469_000_000, ratio: 0.18 })),
    ).toBe(444_420_000);
  });

  it("reads 0 when nothing has been uploaded", () => {
    expect(uploadedBytes(torrent({ bytesDone: 1000, ratio: 0 }))).toBe(0);
  });

  it("survives a ratio the daemon could not compute", () => {
    expect(uploadedBytes(torrent({ bytesDone: 1000, ratio: NaN }))).toBe(0);
    expect(uploadedBytes(torrent({ bytesDone: 1000, ratio: Infinity }))).toBe(
      0,
    );
    expect(uploadedBytes(torrent({ bytesDone: 1000, ratio: -1 }))).toBe(0);
  });
});

describe("piecesText", () => {
  it("is a dash until the detail poll delivers the piece map", () => {
    expect(piecesText(undefined)).toBe("—");
    expect(piecesText(pieces())).toBe("—");
  });

  it("reads as a count of chunks at the chunk size", () => {
    expect(piecesText(pieces({ sizeChunks: 256, chunkSize: 262_144 }))).toBe(
      "256 × 256 KiB",
    );
  });

  it("asks rather than inventing a size the daemon did not report", () => {
    expect(piecesText(pieces({ sizeChunks: 256 }))).toBe("256 × ?");
  });
});

describe("addedText", () => {
  it("is a dash when neither who nor when is known", () => {
    expect(addedText(torrent())).toBe("—");
  });

  it("says who, when that is all rtorrent knows", () => {
    expect(addedText(torrent({ addedBy: "alice" }))).toBe("alice");
  });

  it("pairs the added-by with the date", () => {
    const text = addedText(
      torrent({ addedBy: "alice", addedAt: 1_700_000_000 }),
    );
    expect(text.startsWith("alice · ")).toBe(true);
    expect(text.length).toBeGreaterThan("alice · ".length);
  });

  it("names an unknown author rather than dropping the date", () => {
    expect(
      addedText(torrent({ addedAt: 1_700_000_000 })).startsWith("unknown · "),
    ).toBe(true);
  });
});

describe("primaryFacts", () => {
  it("is the handoff's eleven facts, in its order", () => {
    expect(primaryFacts(torrent()).map(([key]) => key)).toEqual([
      "Hash",
      "Downloaded",
      "Uploaded",
      "Ratio",
      "Pieces",
      "Peers",
      "Down rate",
      "Up rate",
      "Path",
      "Added",
      "Private",
    ]);
  });

  it("gives each rate its direction's tone, and a dash when idle", () => {
    const facts = primaryFacts(
      torrent({ downRate: 8.4 * 1024 * 1024, upRate: 0 }),
    );
    expect(facts.find(([key]) => key === "Down rate")).toEqual([
      "Down rate",
      "8.4 MiB/s",
      "rate",
    ]);
    expect(facts.find(([key]) => key === "Up rate")).toEqual([
      "Up rate",
      "—",
      "up",
    ]);
  });

  it("formats the figures the rail shares with the table", () => {
    const facts = primaryFacts(
      torrent({
        bytesDone: 2_469_000_000,
        ratio: 0.18,
        peersConnected: 112,
        savePath: "/mnt/data/downloads/iso",
      }),
    );
    expect(valueOf(facts, "Downloaded")).toBe("2.3 GiB");
    expect(valueOf(facts, "Ratio")).toBe("0.18");
    expect(valueOf(facts, "Peers")).toBe("112");
    expect(valueOf(facts, "Path")).toBe("/mnt/data/downloads/iso");
  });

  it("dashes a path the daemon has not reported", () => {
    expect(valueOf(primaryFacts(torrent()), "Path")).toBe("—");
  });

  it("answers Private in both directions", () => {
    expect(valueOf(primaryFacts(torrent()), "Private")).toBe("no");
    expect(valueOf(primaryFacts(torrent({ isPrivate: true })), "Private")).toBe(
      "yes",
    );
  });
});

describe("extraFacts", () => {
  it("falls back to the session limits and says that is what it did", () => {
    const facts = extraFacts(torrent(), 1024 * 1024, 0);
    expect(valueOf(facts, "Down limit")).toBe("1.0 MiB/s · global");
    // 0 is rtorrent's "unlimited", so there is no rate to show.
    expect(valueOf(facts, "Up limit")).toBe("∞ · global");
  });

  it("prefers a torrent's own throttle, and labels it as one", () => {
    const facts = extraFacts(
      torrent({ downRateLimit: 2048, throttleName: "lan" }),
      1024 * 1024,
      0,
    );
    expect(valueOf(facts, "Down limit")).toBe("2.0 KiB/s · torrent override");
  });

  it("words the full precedence: rule, turtle, global", () => {
    const rules = [{ id: "vid", label: "video", downKb: 1024, upKb: 256 }];
    const ruled = extraFacts(
      torrent({
        downRateLimit: 1024,
        label: "video",
        throttleName: "rule_vid",
        throttleRule: "vid",
      }),
      0,
      0,
      rules,
      true,
    );
    expect(valueOf(ruled, "Down limit")).toBe('1.0 KiB/s · rule label "video"');
    const turtled = extraFacts(torrent(), 0, 0, [], true);
    expect(valueOf(turtled, "Down limit")).toBe("∞ · turtle");
  });

  it("reads an unset cap as the daemon's default", () => {
    const facts = extraFacts(torrent(), 0, 0);
    expect(valueOf(facts, "Peer cap")).toBe("default");
    expect(valueOf(facts, "Peer floor")).toBe("default");
    expect(valueOf(facts, "Upload slots")).toBe("default");
  });

  it("reports a cap the torrent carries", () => {
    const facts = extraFacts(
      torrent({ peersMax: 200, peersMin: 40, uploadsMax: 12 }),
      0,
      0,
    );
    expect(valueOf(facts, "Peer cap")).toBe("200");
    expect(valueOf(facts, "Peer floor")).toBe("40");
    expect(valueOf(facts, "Upload slots")).toBe("12");
  });

  it("counts the connections the design's rail could not", () => {
    const facts = extraFacts(
      torrent({ seedsConnected: 38, peersConnected: 112 }),
    );
    expect(valueOf(facts, "Connections")).toBe("150");
  });

  it("adds provenance only when there is provenance to add", () => {
    const plain = extraFacts(torrent(), 0, 0).map(([key]) => key);
    expect(plain).not.toContain("Source");
    expect(plain).not.toContain("Mode");
    expect(plain).not.toContain("DHT / PEX");

    const rich = extraFacts(
      torrent({
        sourcePath: "/inbox/debian.torrent",
        connectionType: "initial_seed",
        isPrivate: true,
      }),
      0,
      0,
    );
    expect(valueOf(rich, "Source")).toBe("/inbox/debian.torrent");
    expect(valueOf(rich, "Mode")).toBe("super-seeding");
    expect(valueOf(rich, "DHT / PEX")).toBe("off — private torrent");
  });

  it("keeps the state facts the design's rail had no field for", () => {
    const keys = extraFacts(torrent(), 0, 0).map(([key]) => key);
    expect(keys).toEqual([
      "Size",
      "ETA",
      "Connections",
      "Down limit",
      "Up limit",
      "Peer cap",
      "Peer floor",
      "Upload slots",
    ]);
  });
});
