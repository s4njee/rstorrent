// Bitfield tests. The bit ordering is the easy thing to get wrong, so these
// pin it against real rtorrent output shapes (all-F for a complete torrent,
// and a partial "FFFF0000…" prefix for a half-done one).
import { describe, it, expect } from "vitest";
import {
  bitfieldToBytes,
  hasPiece,
  countPieces,
  bucketFractions,
  availabilityToBytes,
  bucketAverages,
  distributedCopies,
} from "./bitfield";

/** Base64 of the given per-chunk peer counts, as the backend would send it. */
function b64(counts: number[]): string {
  let bin = "";
  for (const c of counts) bin += String.fromCharCode(c);
  return btoa(bin);
}

describe("bitfieldToBytes / hasPiece", () => {
  it("reads MSB-first within each byte", () => {
    // 0x80 = 1000_0000 → only piece 0 present.
    const b = bitfieldToBytes("80");
    expect(hasPiece(b, 0)).toBe(true);
    expect(hasPiece(b, 1)).toBe(false);
    expect(hasPiece(b, 7)).toBe(false);
    // 0x01 = 0000_0001 → only piece 7 present.
    const c = bitfieldToBytes("01");
    expect(hasPiece(c, 7)).toBe(true);
    expect(hasPiece(c, 0)).toBe(false);
  });

  it("all-F means every piece present (rtorrent's complete torrent)", () => {
    const b = bitfieldToBytes("FFFF");
    expect(countPieces(b, 16)).toBe(16);
  });

  it("half-done torrent: FFFF0000 → first 16 of 32 pieces", () => {
    const b = bitfieldToBytes("FFFF0000");
    expect(countPieces(b, 32)).toBe(16);
    expect(hasPiece(b, 15)).toBe(true);
    expect(hasPiece(b, 16)).toBe(false);
  });

  it("handles empty and out-of-range safely", () => {
    const b = bitfieldToBytes("");
    expect(b.length).toBe(0);
    expect(hasPiece(b, 0)).toBe(false);
    expect(hasPiece(bitfieldToBytes("FF"), 99)).toBe(false);
    expect(hasPiece(bitfieldToBytes("FF"), -1)).toBe(false);
  });

  it("is case-insensitive and tolerates whitespace", () => {
    expect(countPieces(bitfieldToBytes(" ffff "), 16)).toBe(16);
  });
});

describe("bucketFractions", () => {
  it("summarizes each bucket's completed fraction", () => {
    // 32 pieces, first 16 done → 2 buckets: [1.0, 0.0]
    const b = bitfieldToBytes("FFFF0000");
    expect(Array.from(bucketFractions(b, 32, 2))).toEqual([1, 0]);
    // 4 buckets over the same field → [1, 1, 0, 0]
    expect(Array.from(bucketFractions(b, 32, 4))).toEqual([1, 1, 0, 0]);
  });

  it("returns partial fractions when a bucket straddles the edge", () => {
    // 16 pieces, first 8 done, 1 bucket → 0.5
    const b = bitfieldToBytes("FF00");
    expect(bucketFractions(b, 16, 1)[0]).toBeCloseTo(0.5);
  });

  it("downsamples a huge piece count to the bar width", () => {
    // 100k pieces all present, 500 buckets → every bucket fully done.
    const bytes = new Uint8Array(12500).fill(0xff);
    const f = bucketFractions(bytes, 100000, 500);
    expect(f.length).toBe(500);
    expect(Array.from(f).every((v) => v === 1)).toBe(true);
  });

  it("handles more buckets than pieces without dropping any", () => {
    // 4 pieces (all done) across 8 buckets: each bucket maps to one piece.
    const b = bitfieldToBytes("F0");
    const f = bucketFractions(b, 4, 8);
    expect(f.length).toBe(8);
    expect(Array.from(f).every((v) => v === 1)).toBe(true);
  });

  it("degenerate inputs return empty/zero", () => {
    expect(bucketFractions(new Uint8Array(), 0, 4).length).toBe(4);
    expect(Array.from(bucketFractions(new Uint8Array(), 0, 4))).toEqual([
      0, 0, 0, 0,
    ]);
  });
});

describe("availabilityToBytes", () => {
  it("decodes base64 into full-range per-chunk counts", () => {
    expect(Array.from(availabilityToBytes(b64([0, 1, 2, 255])))).toEqual([
      0, 1, 2, 255,
    ]);
  });

  it("returns empty for blank or garbled input", () => {
    expect(availabilityToBytes("").length).toBe(0);
    expect(availabilityToBytes("   ").length).toBe(0);
    expect(availabilityToBytes("!!not base64!!").length).toBe(0);
  });
});

describe("bucketAverages", () => {
  it("averages each bucket's peer count and reports the peak", () => {
    const c = availabilityToBytes(b64([4, 4, 0, 0]));
    const { avg, max } = bucketAverages(c, 4, 2);
    expect(Array.from(avg)).toEqual([4, 0]);
    expect(max).toBe(4);
  });

  it("averages a straddling bucket", () => {
    const c = availabilityToBytes(b64([2, 4]));
    const { avg, max } = bucketAverages(c, 2, 1);
    expect(avg[0]).toBeCloseTo(3);
    expect(max).toBeCloseTo(3);
  });

  it("handles more buckets than chunks without dropping any", () => {
    const c = availabilityToBytes(b64([5, 5]));
    const { avg, max } = bucketAverages(c, 2, 4);
    expect(avg.length).toBe(4);
    expect(Array.from(avg).every((v) => v === 5)).toBe(true);
    expect(max).toBe(5);
  });

  it("degenerate inputs return zeroed avg and zero max", () => {
    const { avg, max } = bucketAverages(new Uint8Array(), 0, 3);
    expect(Array.from(avg)).toEqual([0, 0, 0]);
    expect(max).toBe(0);
  });
});

describe("distributedCopies", () => {
  it("is the rarest chunk's count plus the fraction above it", () => {
    // rarest = 1, three of four chunks exceed it → 1 + 3/4.
    const c = availabilityToBytes(b64([1, 2, 2, 2]));
    expect(distributedCopies(c, 4)).toBeCloseTo(1.75);
  });

  it("uniform availability reports whole copies", () => {
    const c = availabilityToBytes(b64([3, 3, 3]));
    expect(distributedCopies(c, 3)).toBe(3);
  });

  it("a single scarce chunk floors the whole torrent", () => {
    const c = availabilityToBytes(b64([0, 9, 9, 9]));
    expect(distributedCopies(c, 4)).toBeCloseTo(0.75);
  });

  it("no data means zero copies", () => {
    expect(distributedCopies(new Uint8Array(), 0)).toBe(0);
  });
});
