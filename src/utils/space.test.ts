import { describe, expect, it } from "vitest";
import { barFraction, spaceByLabel, UNLABELED } from "./space";
import type { TorrentDto } from "../ipc/types";

function torrent(label: string, size: number): TorrentDto {
  return { label, size } as TorrentDto;
}

describe("spaceByLabel", () => {
  it("groups sizes by label and buckets an empty label as unlabeled", () => {
    const result = spaceByLabel([
      torrent("iso", 100),
      torrent("iso", 50),
      torrent("", 30),
      torrent("video", 200),
    ]);
    expect(result).toEqual([
      { label: "video", bytes: 200, count: 1 },
      { label: "iso", bytes: 150, count: 2 },
      { label: UNLABELED, bytes: 30, count: 1 },
    ]);
  });

  it("breaks ties by label so the order is stable", () => {
    const result = spaceByLabel([torrent("b", 10), torrent("a", 10)]);
    expect(result.map((entry) => entry.label)).toEqual(["a", "b"]);
  });

  it("is empty for no torrents", () => {
    expect(spaceByLabel([])).toEqual([]);
  });
});

describe("barFraction", () => {
  it("scales to the largest bucket and clamps", () => {
    expect(barFraction(50, 100)).toBe(0.5);
    expect(barFraction(200, 100)).toBe(1);
    expect(barFraction(-5, 100)).toBe(0);
  });

  it("returns zero when there is nothing to scale to", () => {
    expect(barFraction(10, 0)).toBe(0);
  });
});
