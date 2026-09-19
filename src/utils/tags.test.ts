import { describe, expect, it } from "vitest";
import { normaliseTags, tagChip, tagColour, unionTags } from "./tags";

describe("normaliseTags", () => {
  it("trims, drops empties and the separator, and de-dupes case-insensitively", () => {
    expect(normaliseTags([" Linux ", "", "a,b", "linux"])).toEqual([
      "Linux",
      "ab",
    ]);
  });

  it("caps a single tag at 64 characters", () => {
    expect(normaliseTags(["x".repeat(200)])[0]).toHaveLength(64);
  });

  it("accepts a readonly list", () => {
    const tags = ["iso", "archive"] as const;
    expect(normaliseTags(tags)).toEqual(["iso", "archive"]);
  });
});

describe("tagColour", () => {
  it("is stable for a name and case-insensitive", () => {
    expect(tagColour("Linux")).toBe(tagColour("linux"));
    expect(tagColour("linux")).toMatch(/^#[0-9a-f]{6}$/i);
  });

  it("gives different names different colours in practice", () => {
    // Not a guarantee for every pair, but the palette spread means these differ.
    expect(tagColour("linux")).not.toBe(tagColour("iso"));
  });
});

describe("tagChip", () => {
  it("composes a tinted chip with readable text", () => {
    const chip = tagChip("linux");
    expect(chip.text).toBe("linux");
    expect(chip.background).toBeTruthy();
    expect(chip.color).toBeTruthy();
  });
});

describe("unionTags", () => {
  it("merges several lists, preserving first-seen order", () => {
    expect(
      unionTags([
        ["b", "a"],
        ["c", "a"],
      ]),
    ).toEqual(["b", "a", "c"]);
  });
});
