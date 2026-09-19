/**
 * The palette files, their structure, and the contrast guardrail.
 *
 * Runs in the Node environment (`@vitest-environment node`) because it reads the
 * palette layers from disk through `scripts/check-contrast.mjs` — the same module
 * `npm run check:contrast` runs, so the rules are implemented once and the CI
 * test cannot drift from the command.
 *
 * It asserts the invariants a hand-edit can break:
 *   · a theme that defines only part of the palette, silently inheriting dark;
 *   · a palette value with no alias in the semantic layer, or an alias holding a
 *     literal instead of deriving one;
 *   · a preview swatch that has drifted from the palette it claims to preview;
 *   · a pair below its contrast floor, including selected-row text under every
 *     accent preset the picker offers.
 *
 * `src/theme/theme.test.ts` covers the runtime side (application, storage, the
 * colour maths) under jsdom.
 *
 * @vitest-environment node
 */
import { describe, expect, it } from "vitest";
import {
  ACCENT_PRESETS,
  checkPalettes,
  failuresFor,
  findColourLiterals,
  findDanglingTokens,
  PAIRS,
  readPalettes,
  readTokens,
} from "../../scripts/check-contrast.mjs";
import { THEMES, paletteOf } from "./theme";

const { palettes, overridesByTheme } = readPalettes();
const tokens = readTokens();

describe("palette structure", () => {
  it("has a value for every alias the semantic layer uses", () => {
    const names = Object.keys(palettes.dark);
    expect(names.length).toBeGreaterThan(20);
    const aliasValues = Object.values(tokens).join("\n");
    // A palette value with no alias is dead weight; an alias with no value
    // renders empty, so both directions matter.
    const unaliased = names.filter(
      (name) => !aliasValues.includes(`var(${name})`),
    );
    expect(unaliased).toEqual([]);
  });

  it("defines every theme as a complete value set", () => {
    // A partial theme silently falls through to the dark palette, which is how
    // a "new theme" ends up half dark.
    for (const [theme, overrides] of Object.entries(overridesByTheme)) {
      expect(Object.keys(overrides).sort(), `${theme} is partial`).toEqual(
        Object.keys(palettes.dark).sort(),
      );
    }
    expect(Object.keys(overridesByTheme).sort()).toEqual([
      "classic",
      "contrast",
      "light",
      "midnight",
    ]);
  });

  it("carries no colour literal outside the palette layer", () => {
    // tokens.css may alias and derive, never hard-code.
    for (const [name, value] of Object.entries(tokens)) {
      expect(value, `${name} holds a literal`).not.toMatch(/#[0-9a-f]{3,8}\b/i);
      expect(value, `${name} holds a literal`).not.toMatch(/\brgba?\(/i);
    }
  });

  it("declares every custom property the tree consumes", () => {
    // A renamed token that some component still references renders as nothing.
    const dangling = findDanglingTokens();
    const report = dangling
      .map((t) => `${t.file}:${t.line} ${t.name}`)
      .join("\n");
    expect(report).toBe("");
  });

  it("keeps every component's colours in the tokens", () => {
    // A component with its own hex cannot follow a theme. This is the rule
    // stylelint would express; the repository has no stylelint, so it runs here.
    const literals = findColourLiterals();
    const report = literals
      .map((l) => `${l.file}:${l.line} ${l.text}`)
      .join("\n");
    expect(report).toBe("");
  });

  it("lists every theme's accents in the checker's presets", () => {
    // The checker cannot import the registry (it runs without a build step), so
    // the two copies are pinned here.
    for (const theme of THEMES) {
      expect(ACCENT_PRESETS[theme.id], `${theme.id} presets`).toEqual([
        ...theme.accents,
      ]);
      expect(theme.accents[0], `${theme.id} default accent`).toBe(
        palettes[theme.id]["--pal-accent"],
      );
    }
    // And the registry's own accessor agrees with the file.
    expect(paletteOf("classic")).toBe(palettes.classic["--pal-accent"]);
  });
});

describe("preview swatches", () => {
  it("match the palette each theme ships", () => {
    const expected: Record<string, string> = {
      bg: "--pal-bg-app",
      panel: "--pal-bg-panel",
      text: "--pal-text-body",
      accent: "--pal-accent",
      progress: "--pal-progress-active",
    };
    for (const theme of THEMES) {
      for (const [swatch, token] of Object.entries(expected)) {
        expect(
          theme.preview[swatch as keyof typeof theme.preview].toLowerCase(),
          `${theme.id} preview.${swatch}`,
        ).toBe(palettes[theme.id][token].toLowerCase());
      }
    }
  });
});

describe("contrast guardrail", () => {
  it("holds for every pair in every theme", () => {
    const failures = failuresFor(palettes);
    const report = failures
      .map(
        (f) =>
          `${f.theme} ${f.foreground} on ${f.background} = ${f.ratio.toFixed(2)}`,
      )
      .join("\n");
    expect(report).toBe("");
  });

  it("checks a meaningful number of pairs", () => {
    // Guards against a refactor that quietly stops running the pairs.
    const results = checkPalettes(palettes);
    expect(results.length).toBeGreaterThanOrEqual(PAIRS.length * THEMES.length);
    expect(results.every((result) => Number.isFinite(result.ratio))).toBe(true);
  });

  it("keeps selected-row text readable under every accent preset", () => {
    // The row is the accent at 22% over the app surface, so an accent that
    // darkens or lightens the row too far takes the row's text with it.
    const results = checkPalettes(palettes).filter(
      (r) => r.role === "selected row",
    );
    expect(results.length).toBe(THEMES.length * 5);
    expect(results.filter((result) => !result.ok)).toEqual([]);
  });
});
