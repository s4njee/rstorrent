/*
 * Types for `check-contrast.mjs`.
 *
 * The checker is a plain Node script on purpose — it runs from `npm run
 * check:contrast` with no build step — so its public surface is declared here
 * for the TypeScript test that imports it. Keep the two in step: the test is
 * what proves the rules hold, and it cannot compile without these signatures.
 */

export interface Rgba {
  r: number;
  g: number;
  b: number;
  a: number;
}

export interface ContrastResult {
  theme: string;
  foreground: string;
  background: string;
  role: string;
  floor: number;
  ratio: number;
  ok: boolean;
  reason?: string;
}

export type Palette = Record<string, string>;
export type Palettes = Record<string, Palette>;

/** `[foreground token, background token, floor, role]`. */
export declare const PAIRS: ReadonlyArray<
  readonly [string, string, number, string]
>;

/** The stopped-and-incomplete fill, which is deliberately recessive. */
export declare const RECESSIVE: {
  foreground: string;
  background: string;
  floor: number;
};

/** Selected-row text over the accent tint, checked for every preset. */
export declare const SELECTED_ROW: {
  text: string;
  alpha: number;
  floor: number;
};

/** Accent presets per theme, mirroring src/theme/theme.ts's registry. */
export declare const ACCENT_PRESETS: Record<string, readonly string[]>;

export declare function parseDeclarations(css: string): Record<string, string>;

export declare function readPalettes(root?: string): {
  base: Palette;
  palettes: Palettes;
  overridesByTheme: Record<string, Palette>;
};

export declare function readTokens(root?: string): Record<string, string>;

/** `var(--name)` references with no declaration anywhere in the tree. */
export declare function findDanglingTokens(
  srcDir?: string,
): Array<{ file: string; line: number; name: string }>;

/** CSS files (outside the palette layer) holding a colour literal. */
export declare function findColourLiterals(
  srcDir?: string,
): Array<{ file: string; line: number; text: string }>;

export declare function parseColour(value: string): Rgba | null;

export declare function composite(over: Rgba, under: Rgba): Rgba;

export declare function contrastRatio(a: string, b: string): number;

export declare function checkPalettes(palettes: Palettes): ContrastResult[];

export declare function failuresFor(palettes: Palettes): ContrastResult[];
