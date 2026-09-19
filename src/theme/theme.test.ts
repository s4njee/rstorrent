/**
 * The theme system's runtime behaviour: the derivation maths, choice parsing,
 * and what applying a theme actually does to the document.
 *
 * The palette files themselves — their structure, the preview swatches and the
 * contrast floors — are covered by `palette.test.ts`, which runs in Node so it
 * can read them from disk.
 */
import { beforeEach, describe, expect, it } from "vitest";
import {
  ACCENT_DERIVATIONS,
  DENSITY_STORAGE_KEY,
  THEMES,
  THEME_CHOICES,
  THEME_IDS,
  THEME_STORAGE_KEY,
  applyAccent,
  applyDensity,
  applyTheme,
  applyThemeId,
  clearAccent,
  contrastRatio,
  deriveAccentTokens,
  initialChoice,
  labelChipStyle,
  loadDensity,
  loadThemeChoice,
  mixWith,
  parseCssColor,
  parseDensity,
  parseHex,
  parseThemeChoice,
  parseThemeId,
  readableTextOn,
  resolveThemeId,
  saveDensity,
  saveThemeChoice,
  supportsColorMix,
  toHex,
  withAlpha,
} from "./theme";

const root = () => document.documentElement;

beforeEach(() => {
  root().removeAttribute("data-theme");
  root().removeAttribute("data-density");
  root().removeAttribute("style");
  localStorage.clear();
  document
    .querySelectorAll('meta[name^="rstorrent-"]')
    .forEach((meta) => meta.remove());
});

describe("colour maths", () => {
  it("parses the hex and rgb() forms the palettes and tokens use", () => {
    expect(parseHex("#f59e0b")).toEqual({ r: 245, g: 158, b: 11 });
    expect(parseHex("#fff")).toBeNull();
    expect(parseHex("red")).toBeNull();
    expect(parseCssColor("rgba(1, 2, 3, 0.5)")).toEqual({ r: 1, g: 2, b: 3 });
    expect(parseCssColor("rgb(255, 255, 255)")).toEqual({
      r: 255,
      g: 255,
      b: 255,
    });
    expect(parseCssColor("hsl(1 2% 3%)")).toBeNull();
  });

  it("round-trips a colour through hex", () => {
    expect(toHex({ r: 245, g: 158, b: 11 })).toBe("#f59e0b");
    // Out-of-range channels clamp rather than wrapping.
    expect(toHex({ r: 300, g: -4, b: 0 })).toBe("#ff0000");
  });

  it("mixes toward white and scales alpha like color-mix", () => {
    expect(mixWith("#000000", "#ffffff", 0)).toBe("#000000");
    expect(mixWith("#000000", "#ffffff", 1)).toBe("#ffffff");
    expect(mixWith("#000000", "#ffffff", 0.5)).toBe("#808080");
    expect(mixWith("#000000", "#ffffff", 2)).toBe("#ffffff");
    expect(withAlpha("#f59e0b", 0.22)).toBe("rgba(245, 158, 11, 0.220)");
  });

  it("mirrors the derivations tokens.css performs", () => {
    const derived = deriveAccentTokens("#f59e0b", "#ffffff");
    expect(derived.tint).toBe(
      withAlpha("#f59e0b", ACCENT_DERIVATIONS.tintAlpha),
    );
    expect(derived.tintStrong).toBe(
      withAlpha("#f59e0b", ACCENT_DERIVATIONS.tintStrongAlpha),
    );
    expect(derived.text).toBe(
      mixWith("#f59e0b", "#ffffff", ACCENT_DERIVATIONS.textInk),
    );
    expect(derived.ring).toBe(
      mixWith("#f59e0b", "#ffffff", ACCENT_DERIVATIONS.ringInk),
    );
  });

  it("ranks contrast the way WCAG does", () => {
    expect(contrastRatio("#ffffff", "#000000")).toBeCloseTo(21, 0);
    expect(contrastRatio("#ffffff", "#ffffff")).toBeCloseTo(1, 5);
    expect(Number.isNaN(contrastRatio("nonsense", "#000000"))).toBe(true);
    // Symmetric.
    expect(contrastRatio("#101214", "#e8eaed")).toBeCloseTo(
      contrastRatio("#e8eaed", "#101214"),
      5,
    );
  });

  it("picks readable text for a user-chosen label colour", () => {
    expect(readableTextOn("#101214")).toBe("#ffffff");
    expect(readableTextOn("#f4f5f6")).toBe("#0b0c0d");
    // Unparseable input still returns a usable colour rather than throwing.
    expect(readableTextOn("not-a-colour")).toBe("#ffffff");
  });

  it("tints a custom label chip and keeps its text readable", () => {
    const chip = labelChipStyle("#a78bfa", "#101214");
    expect(chip).toBeDefined();
    expect(contrastRatio(chip!.background, chip!.color)).toBeGreaterThanOrEqual(
      4.5,
    );
    // No colour configured: the built-in palette chip applies instead.
    expect(labelChipStyle(undefined, "#101214")).toBeUndefined();
    expect(labelChipStyle("nonsense", "#101214")).toBeUndefined();
  });
});

describe("choosing a theme", () => {
  it("parses only known values", () => {
    expect(parseThemeChoice("midnight")).toBe("midnight");
    expect(parseThemeChoice("system")).toBe("system");
    expect(parseThemeChoice("solarized")).toBeNull();
    expect(parseThemeChoice(null)).toBeNull();
    expect(parseThemeId("nope")).toBeNull();
    expect(parseDensity("comfortable")).toBe("comfortable");
    expect(parseDensity("roomy")).toBeNull();
  });

  it("resolves `system` against the OS preference", () => {
    expect(resolveThemeId("system", true)).toBe("dark");
    expect(resolveThemeId("system", false)).toBe("light");
    expect(resolveThemeId("contrast", false)).toBe("contrast");
  });

  it("exposes a registry the choices and previews agree with", () => {
    expect(THEME_IDS).toEqual([
      "dark",
      "light",
      "midnight",
      "contrast",
      "classic",
    ]);
    expect(THEME_CHOICES).toContain("system");
    expect(THEMES.every((theme) => theme.accents.length === 5)).toBe(true);
    expect(
      THEMES.filter((theme) => theme.dark).map((theme) => theme.id),
    ).toEqual(["dark", "midnight", "contrast"]);
  });
});

describe("applying a theme", () => {
  it("puts the id and the native scheme on the document", () => {
    applyThemeId("contrast");
    expect(root().dataset.theme).toBe("contrast");
    expect(root().style.colorScheme).toBe("dark");

    applyThemeId("light");
    expect(root().dataset.theme).toBe("light");
    expect(root().style.colorScheme).toBe("light");
  });

  it("sets the one tweakable accent, and the derivations only where CSS cannot", () => {
    expect(applyAccent("#e5484d")).toBe(true);
    expect(root().style.getPropertyValue("--accent")).toBe("#e5484d");

    if (supportsColorMix(document)) {
      // CSS owns the derivations, so any stale inline values must be gone.
      expect(root().style.getPropertyValue("--accent-tint")).toBe("");
      expect(root().style.getPropertyValue("--accent-text")).toBe("");
    } else {
      const derived = deriveAccentTokens("#e5484d", "#ffffff");
      expect(root().style.getPropertyValue("--accent-tint")).toBe(derived.tint);
      expect(root().style.getPropertyValue("--accent-tint-strong")).toBe(
        derived.tintStrong,
      );
      expect(root().style.getPropertyValue("--focus-ring")).toBe(derived.ring);
    }

    clearAccent();
    expect(root().style.getPropertyValue("--accent")).toBe("");
    expect(root().style.getPropertyValue("--accent-tint")).toBe("");
  });

  it("refuses an accent that is not a plain hex", () => {
    expect(applyAccent("red")).toBe(false);
    expect(applyAccent("rgb(1,2,3)")).toBe(false);
    expect(applyAccent("#12345")).toBe(false);
    expect(root().style.getPropertyValue("--accent")).toBe("");
  });

  it("applies the density attribute", () => {
    applyDensity("comfortable");
    expect(root().dataset.density).toBe("comfortable");
    applyDensity("dense");
    expect(root().dataset.density).toBe("dense");
  });

  it("resolves a choice end to end, preferring the stored one", () => {
    // No matchMedia in this environment: `system` falls back to dark.
    expect(applyTheme("system", null)).toBe("dark");
    expect(root().dataset.theme).toBe("dark");

    saveThemeChoice("classic");
    expect(loadThemeChoice()).toBe("classic");
    expect(initialChoice()).toBe("classic");
    applyTheme("classic", "#0b5cad");
    expect(root().dataset.theme).toBe("classic");
    expect(root().style.getPropertyValue("--accent")).toBe("#0b5cad");
  });

  it("falls back to the operator default, then to dark", () => {
    expect(initialChoice()).toBe("dark");

    const meta = document.createElement("meta");
    meta.setAttribute("name", "rstorrent-theme-default");
    meta.setAttribute("content", "midnight");
    document.head.appendChild(meta);
    expect(initialChoice()).toBe("midnight");

    // A stored choice beats the operator default.
    saveThemeChoice("light");
    expect(initialChoice()).toBe("light");

    // Junk in the meta tag is ignored rather than applied.
    meta.setAttribute("content", "__PLACEHOLDER__");
    saveThemeChoice(null);
    expect(initialChoice()).toBe("dark");
  });

  it("round-trips density through storage", () => {
    expect(loadDensity()).toBe("dense");
    saveDensity("comfortable");
    expect(localStorage.getItem(DENSITY_STORAGE_KEY)).toBe("comfortable");
    expect(loadDensity()).toBe("comfortable");
  });

  it("clears a stored choice so the operator default can show again", () => {
    saveThemeChoice("midnight");
    expect(localStorage.getItem(THEME_STORAGE_KEY)).toBe("midnight");
    saveThemeChoice(null);
    expect(localStorage.getItem(THEME_STORAGE_KEY)).toBeNull();
    expect(loadThemeChoice()).toBeNull();
  });
});
