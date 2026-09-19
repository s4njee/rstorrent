import { describe, expect, it } from "vitest";
import { detectDuplicate } from "./duplicates";
import type { TorrentDto } from "../ipc/types";

function torrent(
  hash: string,
  name: string,
  size: number,
  savePath: string,
): TorrentDto {
  return { hash, name, size, savePath } as TorrentDto;
}

const library: TorrentDto[] = [
  torrent("AAAA", "Show.S01", 1000, "/data/Show.S01"),
  torrent("BBBB", "Movie.mkv", 2000, "/data/Movie.mkv"),
];

describe("detectDuplicate (V3-13)", () => {
  it("prefers an exact hash, case-insensitively", () => {
    expect(
      detectDuplicate(
        { hash: "aaaa", name: "Other", size: 9, destination: "/data" },
        library,
      ),
    ).toEqual({ kind: "exact", hash: "AAAA", name: "Show.S01" });
  });

  it("warns on the same name and size", () => {
    expect(
      detectDuplicate({ hash: "CCCC", name: "show.s01", size: 1000 }, library),
    ).toEqual({
      kind: "sameNameAndSize",
      hash: "AAAA",
      name: "Show.S01",
      size: 1000,
    });
  });

  it("does not warn on a name match without a size match", () => {
    expect(
      detectDuplicate({ hash: "CCCC", name: "Show.S01", size: 7 }, library),
    ).toBeNull();
  });

  it("warns when the destination is another torrent's folder", () => {
    expect(
      detectDuplicate(
        { hash: "CCCC", name: "Show.S01", size: 7, destination: "/data/" },
        library,
      ),
    ).toEqual({
      kind: "destinationInUse",
      hash: "AAAA",
      name: "Show.S01",
      path: "/data/Show.S01",
    });
    // A different folder under the same parent is clean.
    expect(
      detectDuplicate(
        { hash: "CCCC", name: "Show.S01", size: 7, destination: "/data/other" },
        library,
      ),
    ).toBeNull();
  });

  it("never matches on unknown fields", () => {
    expect(detectDuplicate({}, library)).toBeNull();
    expect(detectDuplicate({ destination: "/data" }, library)).toBeNull();
    expect(
      detectDuplicate({ hash: "CCCC", name: "Show.S01", size: 0 }, library),
    ).toBeNull();
  });
});
