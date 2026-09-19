#!/usr/bin/env node
/*
 * Contrast check for the theme palettes.
 *
 * Five themes multiplied by an editable accent is a contrast accident waiting to
 * happen, so this reads the palette layers the same way the browser does — the
 * default block in `src/theme/palette.css`, overridden per theme by
 * `src/theme/themes/*.css` — resolves each theme's values, and checks the pairs
 * that carry text or meaning against a documented floor.
 *
 *   node scripts/check-contrast.mjs          # fails on a violation
 *   node scripts/check-contrast.mjs --verbose
 *
 * The floors are WCAG 2.1 AA, split by what the token actually does:
 *
 *   text   4.5  reading text (names, cells, labels, button labels, status words)
 *   meta   3.0  incidental text — hints, timestamps, placeholders, captions.
 *               The handoff puts these one band dimmer than body text on
 *               purpose; 4.5 there would mean overriding the design.
 *   ui     3.0  non-text indicators: progress fills, selection tints
 *
 * `src/theme/theme.test.ts` imports this module, so the same rules run in CI.
 */

import { readdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const THEME_DIR = join(HERE, "..", "src", "theme");

/** Themes that override the default palette in `palette.css`. */
const THEME_FILES = ["light", "midnight", "contrast", "classic"];

/** Every pair that must hold, with the floor for its role. `{theme}` in a
 * surface name means "the same token from that theme". */
export const PAIRS = [
  // Reading text on each surface it appears on.
  ["--pal-text-primary", "--pal-bg-app", 4.5, "text"],
  ["--pal-text-primary", "--pal-bg-panel", 4.5, "text"],
  ["--pal-text-primary", "--pal-bg-chrome", 4.5, "text"],
  ["--pal-text-primary", "--pal-bg-elevated", 4.5, "text"],
  ["--pal-text-body", "--pal-bg-app", 4.5, "text"],
  ["--pal-text-body", "--pal-bg-panel", 4.5, "text"],
  ["--pal-text-body", "--pal-bg-chrome", 4.5, "text"],
  ["--pal-text-secondary", "--pal-bg-panel", 4.5, "text"],
  ["--pal-text-muted", "--pal-bg-app", 4.5, "text"],
  ["--pal-text-muted", "--pal-bg-chrome", 4.5, "text"],
  ["--pal-text-dim", "--pal-bg-chrome", 4.5, "text"],

  // Incidental text: hints, timestamps, captions, placeholders.
  ["--pal-text-faint", "--pal-bg-app", 3.0, "meta"],
  ["--pal-text-fainter", "--pal-bg-app", 3.0, "meta"],
  ["--pal-text-ghost", "--pal-bg-app", 3.0, "meta"],
  ["--pal-text-ghost", "--pal-bg-panel", 3.0, "meta"],

  // Status words, rates and errors are read, not merely seen.
  ["--pal-status-error", "--pal-bg-app", 4.5, "text"],
  ["--pal-status-error", "--pal-bg-chrome", 4.5, "text"],
  ["--pal-status-warn", "--pal-bg-app", 4.5, "text"],
  ["--pal-rate-up", "--pal-bg-app", 4.5, "text"],
  ["--pal-rate-up-text", "--pal-bg-app", 4.5, "text"],

  // Button labels sit on the accent fill.
  ["--pal-accent-foreground", "--pal-accent", 4.5, "text"],

  // Label chips: the chip colour is the label hue at 22% over the app surface
  // (see tokens.css), so the chip's own hue must clear the non-text floor on it.
  ["--pal-label-iso", "--pal-bg-app", 3.0, "ui"],
  ["--pal-label-kernel", "--pal-bg-app", 3.0, "ui"],
  ["--pal-label-apps", "--pal-bg-app", 3.0, "ui"],
  ["--pal-label-media", "--pal-bg-app", 3.0, "ui"],

  // Progress fills are never the accent, and must be visible on the trough.
  ["--pal-progress-active", "--pal-bg-track", 3.0, "ui"],
  ["--pal-progress-complete", "--pal-bg-track", 3.0, "ui"],
];

/**
 * The stopped-and-incomplete bar is *deliberately* recessive — the design's
 * whole point is that it reads as "nothing happening" next to a live bar, so it
 * cannot meet 3:1 against its trough. The rule there is only that it is not
 * invisible: the fill must differ from the trough it sits in.
 */
export const RECESSIVE = {
  foreground: "--pal-state-disabled",
  background: "--pal-bg-track",
  floor: 1.25,
};

/**
 * A selected row is `--accent-tint` (the accent at 22% alpha) over the app
 * surface, so the row's text must stay readable over that composite — for every
 * accent preset the theme offers, not just its default.
 */
export const SELECTED_ROW = {
  text: "--pal-text-primary",
  alpha: 0.22,
  floor: 4.5,
};

/* ---------------------------------------------------------------------------
 * Parsing
 * ------------------------------------------------------------------------ */

const CUSTOM_PROPERTY = /(--[a-z0-9-]+)\s*:\s*([^;]+);/g;

/** All `--name: value` declarations inside a CSS text. */
export function parseDeclarations(css) {
  const found = {};
  for (const match of css.matchAll(CUSTOM_PROPERTY)) {
    found[match[1]] = match[2].trim();
  }
  return found;
}

/** The palette for one theme: the default block, overridden by its file. */
export function readPalettes(root = THEME_DIR) {
  const base = parseDeclarations(
    readFileSync(join(root, "palette.css"), "utf8"),
  );
  const palettes = { dark: { ...base } };
  for (const theme of THEME_FILES) {
    const path = join(root, "themes", `${theme}.css`);
    const overrides = parseDeclarations(readFileSync(path, "utf8"));
    // A theme must be a whole value set; a partial one silently inherits dark.
    palettes[theme] = { ...base, ...overrides };
  }
  return { base, palettes, overridesByTheme: readOverrides(root) };
}

function readOverrides(root) {
  const result = {};
  for (const file of readdirSync(join(root, "themes"))) {
    if (!file.endsWith(".css")) continue;
    result[file.replace(/\.css$/, "")] = parseDeclarations(
      readFileSync(join(root, "themes", file), "utf8"),
    );
  }
  return result;
}

/**
 * The semantic layer's declarations. Exported so `theme.test.ts` can assert the
 * structural invariants (every palette value is aliased; no alias is dangling)
 * without reading the filesystem itself.
 */
export function readTokens(root = THEME_DIR) {
  return parseDeclarations(readFileSync(join(root, "tokens.css"), "utf8"));
}

/**
 * Every CSS file under `src/` except the palette layer, with any hex or rgb()
 * literal they contain. The palette layer is the only place a colour literal may
 * live; a component with its own hex cannot follow a theme, and is exactly the
 * leak this catches. Stylelint would express the same rule, but the repository
 * has no stylelint and this runs in the existing test command.
 */
export function findColourLiterals(srcDir = join(THEME_DIR, "..")) {
  const literals = [];
  const walk = (dir) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const path = join(dir, entry.name);
      if (entry.isDirectory()) {
        walk(path);
        continue;
      }
      if (!entry.name.endsWith(".css")) continue;
      const relative = path.slice(srcDir.length + 1);
      if (
        relative.startsWith("theme/palette.css") ||
        relative.startsWith("theme/themes/")
      ) {
        continue;
      }
      const text = readFileSync(path, "utf8");
      text.split("\n").forEach((line, index) => {
        // Skip comments: a hex in prose is documentation, not a value.
        const code = line.replace(/\/\*.*?\*\//g, "");
        if (/#[0-9a-fA-F]{3,8}\b/.test(code) || /\brgba?\(/i.test(code)) {
          literals.push({ file: relative, line: index + 1, text: line.trim() });
        }
      });
    }
  };
  walk(srcDir);
  return literals;
}

/**
 * Rewriting the token layer is mostly a rename, and a rename's failure mode is a
 * `var(--old-name)` left behind that silently renders as nothing. This collects
 * every custom property declared in the theme layer (plus any a component sets
 * inline, e.g. `style={{ "--grid": … }}`) and every one consumed, then returns
 * the consumed-but-never-declared names.
 */
export function findDanglingTokens(srcDir = join(THEME_DIR, "..")) {
  const declared = new Set();
  const themeLayer = [
    join(THEME_DIR, "palette.css"),
    join(THEME_DIR, "tokens.css"),
    join(THEME_DIR, "tokens.web.css"),
  ];
  for (const file of themeLayer) {
    for (const name of Object.keys(
      parseDeclarations(readFileSync(file, "utf8")),
    )) {
      declared.add(name);
    }
  }
  for (const theme of readdirSync(join(THEME_DIR, "themes"))) {
    if (!theme.endsWith(".css")) continue;
    for (const name of Object.keys(
      parseDeclarations(readFileSync(join(THEME_DIR, "themes", theme), "utf8")),
    )) {
      declared.add(name);
    }
  }

  const used = [];
  const walk = (dir) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      if (entry.name === "node_modules") continue;
      const path = join(dir, entry.name);
      if (entry.isDirectory()) {
        walk(path);
        continue;
      }
      if (!/\.(css|ts|tsx)$/.test(entry.name)) continue;
      const relative = path.slice(srcDir.length + 1);
      const text = readFileSync(path, "utf8");
      for (const name of Object.keys(parseDeclarations(text)))
        declared.add(name);
      text.split("\n").forEach((line, index) => {
        for (const match of line.matchAll(/var\(\s*(--[a-z0-9-]+)/g)) {
          used.push({ file: relative, line: index + 1, name: match[1] });
        }
        // A custom property set from a component (inline styles, CSS-in-JS).
        for (const match of line.matchAll(/["'](--[a-z0-9-]+)["']\s*:/g)) {
          declared.add(match[1]);
        }
      });
    }
  };
  walk(srcDir);

  return used.filter((entry) => !declared.has(entry.name));
}

/* ---------------------------------------------------------------------------
 * Colour maths (mirrors src/theme/theme.ts)
 * ------------------------------------------------------------------------ */

export function parseColour(value) {
  const text = String(value).trim();
  const hex = /^#([0-9a-f]{6})$/i.exec(text);
  if (hex) {
    const int = parseInt(hex[1], 16);
    return { r: (int >> 16) & 255, g: (int >> 8) & 255, b: int & 255, a: 1 };
  }
  const rgb =
    /^rgba?\(\s*([\d.]+)[\s,]+([\d.]+)[\s,]+([\d.]+)(?:[\s,/]+([\d.]+))?\s*\)$/i.exec(
      text,
    );
  if (rgb) {
    return {
      r: Number(rgb[1]),
      g: Number(rgb[2]),
      b: Number(rgb[3]),
      a: rgb[4] === undefined ? 1 : Number(rgb[4]),
    };
  }
  return null;
}

/** `over` composited on `under` (both opaque results). */
export function composite(over, under) {
  return {
    r: over.r * over.a + under.r * (1 - over.a),
    g: over.g * over.a + under.g * (1 - over.a),
    b: over.b * over.a + under.b * (1 - over.a),
    a: 1,
  };
}

function luminance({ r, g, b }) {
  const channel = (value) => {
    const scaled = value / 255;
    return scaled <= 0.03928
      ? scaled / 12.92
      : Math.pow((scaled + 0.055) / 1.055, 2.4);
  };
  return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b);
}

export function contrastRatio(a, b) {
  const first = parseColour(a);
  const second = parseColour(b);
  if (!first || !second) return NaN;
  const [high, low] = [luminance(first), luminance(second)].sort(
    (x, y) => y - x,
  );
  return (high + 0.05) / (low + 0.05);
}

/* ---------------------------------------------------------------------------
 * The check
 * ------------------------------------------------------------------------ */

/** Runs every pair for every theme. Returns one result per pair. */
export function checkPalettes(palettes) {
  const results = [];
  for (const [theme, palette] of Object.entries(palettes)) {
    for (const [foreground, background, floor, role] of PAIRS) {
      const fg = palette[foreground];
      const bg = palette[background];
      if (!fg || !bg) {
        results.push({
          theme,
          foreground,
          background,
          role,
          floor,
          ratio: NaN,
          ok: false,
          reason: "missing value",
        });
        continue;
      }
      const ratio = contrastRatio(fg, bg);
      results.push({
        theme,
        foreground,
        background,
        role,
        floor,
        ratio,
        ok: ratio >= floor,
      });
    }

    // The recessive fill only has to differ from its trough.
    const recessiveFg = palette[RECESSIVE.foreground];
    const recessiveBg = palette[RECESSIVE.background];
    if (recessiveFg && recessiveBg) {
      const ratio = contrastRatio(recessiveFg, recessiveBg);
      results.push({
        theme,
        foreground: RECESSIVE.foreground,
        background: RECESSIVE.background,
        role: "recessive fill",
        floor: RECESSIVE.floor,
        ratio,
        ok: ratio >= RECESSIVE.floor,
      });
    }

    // Selected-row text over the accent tint, for every preset the theme offers.
    const surface = parseColour(palette["--pal-bg-app"]);
    const ink = parseColour(palette[SELECTED_ROW.text]);
    for (const accent of ACCENT_PRESETS[theme] ?? [palette["--pal-accent"]]) {
      const tint = parseColour(accent);
      if (!surface || !ink || !tint) continue;
      const row = composite({ ...tint, a: SELECTED_ROW.alpha }, surface);
      const ratio = contrastRatio(
        `rgb(${Math.round(ink.r)}, ${Math.round(ink.g)}, ${Math.round(ink.b)})`,
        `rgb(${Math.round(row.r)}, ${Math.round(row.g)}, ${Math.round(row.b)})`,
      );
      results.push({
        theme,
        foreground: `${SELECTED_ROW.text} on ${accent} tint`,
        background: "--pal-bg-app",
        role: "selected row",
        floor: SELECTED_ROW.floor,
        ratio,
        ok: ratio >= SELECTED_ROW.floor,
      });
    }
  }
  return results;
}

/** Accent presets per theme, mirroring `src/theme/theme.ts`'s registry. Kept
 * here rather than imported so this script has no build step. Pinned by
 * `theme.test.ts`, which compares them with the registry. */
export const ACCENT_PRESETS = {
  dark: ["#35418f", "#f59e0b", "#e5484d", "#2f9dff", "#3fb950"],
  light: ["#1d4ed8", "#b45309", "#c2410c", "#0e7490", "#15803d"],
  midnight: ["#4c5fd5", "#fbbf24", "#f87171", "#38bdf8", "#4ade80"],
  contrast: ["#7aa2ff", "#ffd60a", "#ff6b61", "#4cc9f0", "#52e28c"],
  classic: ["#0b5cad", "#b26a00", "#c0392b", "#0e7c86", "#1e8e3e"],
};

export function failuresFor(palettes) {
  return checkPalettes(palettes).filter((result) => !result.ok);
}

function main() {
  const verbose = process.argv.includes("--verbose");
  const { palettes } = readPalettes();
  const results = checkPalettes(palettes);
  const failures = results.filter((result) => !result.ok);
  const literals = findColourLiterals();

  if (verbose) {
    for (const result of results) {
      const ratio = Number.isNaN(result.ratio)
        ? "  n/a"
        : result.ratio.toFixed(2);
      console.log(
        `${result.ok ? "ok  " : "FAIL"} ${result.theme.padEnd(9)} ${ratio.padStart(5)} ` +
          `(min ${result.floor.toFixed(1)}) ${result.foreground} on ${result.background}`,
      );
    }
  }

  console.log(
    `\ncontrast: ${results.length - failures.length}/${results.length} pairs pass ` +
      `across ${Object.keys(palettes).length} themes`,
  );
  console.log(`colour literals outside the palette layer: ${literals.length}`);

  let exit = 0;
  if (failures.length > 0) {
    console.error(`\n${failures.length} pair(s) below the floor:`);
    for (const failure of failures) {
      const ratio = Number.isNaN(failure.ratio)
        ? "n/a"
        : failure.ratio.toFixed(2);
      console.error(
        `  ${failure.theme}: ${failure.foreground} on ${failure.background} ` +
          `= ${ratio} (needs ${failure.floor.toFixed(1)}; ${failure.reason ?? failure.role})`,
      );
    }
    exit = 1;
  }
  if (literals.length > 0) {
    console.error(
      `\n${literals.length} colour literal(s) outside the palette layer — a component` +
        ` with its own hex cannot follow a theme:`,
    );
    for (const literal of literals) {
      console.error(`  ${literal.file}:${literal.line}  ${literal.text}`);
    }
    exit = 1;
  }
  return exit;
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  process.exit(main());
}
