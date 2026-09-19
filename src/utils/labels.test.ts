// Label chips: the design's built-in families, the neutral fallback, and a
// deployment-configured colour composed from a hue.
import { describe, expect, it } from "vitest";
import { labelChip, isBuiltinLabel, labelToken } from "./labels";
import { contrastRatio } from "../theme/theme";

describe("built-in label families", () => {
  it("knows the design's five", () => {
    for (const label of ["iso", "archive", "kernel", "apps", "media"]) {
      expect(isBuiltinLabel(label)).toBe(true);
      expect(labelToken(label)).toMatch(/^var\(--label-/);
    }
  });

  it("treats anything else as uncoloured", () => {
    expect(isBuiltinLabel("video")).toBe(false);
    expect(labelToken("video")).toBeUndefined();
    // The chip still renders — with the neutral treatment.
    expect(labelChip("video")).toEqual({ text: "video" });
  });
});

describe("labelChip", () => {
  it("names an unlabelled row rather than showing an empty chip", () => {
    expect(labelChip("")).toEqual({ text: "unlabeled" });
  });

  it("renders a built-in family through its token, so it follows the theme", () => {
    const chip = labelChip("iso");
    expect(chip.text).toBe("iso");
    // No inline colours: the CSS class carries the tint.
    expect(chip.background).toBeUndefined();
    expect(chip.color).toBeUndefined();
  });

  it("composes a deployment colour into a readable chip", () => {
    const chip = labelChip("video", { video: "#a78bfa" });
    expect(chip.text).toBe("video");
    expect(chip.background).toBeDefined();
    expect(chip.color).toBeDefined();
    // The chip's own text must stay readable on its tint.
    expect(contrastRatio(chip.background!, chip.color!)).toBeGreaterThanOrEqual(
      4.5,
    );
  });

  it("falls back to the neutral chip for a malformed configured colour", () => {
    expect(labelChip("video", { video: "purple" })).toEqual({ text: "video" });
  });
});
