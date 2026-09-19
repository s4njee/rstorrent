/**
 * Theme and density: the registry, application, persistence and the accent
 * derivation maths.
 *
 * The layers this drives, in import order:
 *   `palette.css`      — raw values for the default (dark) theme
 *   `themes/*.css`     — the same names, overridden per `data-theme`
 *   `tokens.css`       — semantic aliases and the accent derivations
 *
 * A theme is therefore a palette value set, and this module's only jobs are to
 * put the right id on `<html>`, to set the one tweakable `--accent` (plus its
 * derived tokens where the browser cannot do it in CSS), and to remember what
 * the user chose.
 *
 * `color-mix() in srgb` interpolates raw channels, so the JS mirror below is
 * plain linear interpolation — no gamma handling.
 */

export type ThemeId = "dark" | "light" | "midnight" | "contrast" | "classic";
export type ThemeChoice = ThemeId | "system";

/** Row density: dense is the handoff; comfortable adds 4px to rows and controls. */
export type Density = "dense" | "comfortable";

export interface ThemeDef {
  id: ThemeId;
  label: string;
  description: string;
  /** True when the theme is dark-surfaced (drives `color-scheme` and `system`). */
  dark: boolean;
  /** Accent presets offered in Settings; the first is the theme's default. */
  accents: readonly string[];
  /** Miniature preview swatches. Pinned to the palette files by theme.test.ts. */
  preview: {
    bg: string;
    panel: string;
    text: string;
    accent: string;
    progress: string;
  };
}

/**
 * The five built-in themes. Order is the order they are listed in Settings.
 * `dark` carries the handoff palette and the handoff's accent with its three
 * documented alternates, plus blackbird's indigo for a fourth choice.
 */
export const THEMES: readonly ThemeDef[] = [
  {
    id: "dark",
    label: "Console Dark",
    description: "Blackbird's palette. Default.",
    dark: true,
    accents: ["#35418f", "#f59e0b", "#e5484d", "#2f9dff", "#3fb950"],
    preview: {
      bg: "#101214",
      panel: "#0e1012",
      text: "#d6d9dd",
      accent: "#35418f",
      progress: "#2f9dff",
    },
  },
  {
    id: "light",
    label: "Console Light",
    description: "Light surfaces, dark text.",
    dark: false,
    accents: ["#1d4ed8", "#b45309", "#c2410c", "#0e7490", "#15803d"],
    preview: {
      bg: "#f4f5f6",
      panel: "#ffffff",
      text: "#1a1d21",
      accent: "#1d4ed8",
      progress: "#1b80e4",
    },
  },
  {
    id: "midnight",
    label: "Midnight",
    description: "True-black OLED variant.",
    dark: true,
    accents: ["#4c5fd5", "#fbbf24", "#f87171", "#38bdf8", "#4ade80"],
    preview: {
      bg: "#000000",
      panel: "#0a0a0b",
      text: "#e6e9ec",
      accent: "#4c5fd5",
      progress: "#2f9dff",
    },
  },
  {
    id: "contrast",
    label: "High Contrast",
    description: "Dark, stronger text and borders.",
    dark: true,
    accents: ["#7aa2ff", "#ffd60a", "#ff6b61", "#4cc9f0", "#52e28c"],
    preview: {
      bg: "#000000",
      panel: "#000000",
      text: "#ffffff",
      accent: "#7aa2ff",
      progress: "#4da3ff",
    },
  },
  {
    id: "classic",
    label: "Classic",
    description: "ruTorrent-inspired light theme, blue selection.",
    dark: false,
    accents: ["#0b5cad", "#b26a00", "#c0392b", "#0e7c86", "#1e8e3e"],
    preview: {
      bg: "#e8edf2",
      panel: "#ffffff",
      text: "#1a1d21",
      accent: "#0b5cad",
      progress: "#2579c9",
    },
  },
];

export const THEME_IDS: readonly ThemeId[] = THEMES.map((theme) => theme.id);
export const THEME_CHOICES: readonly ThemeChoice[] = [...THEME_IDS, "system"];
export const DEFAULT_THEME_ID: ThemeId = "dark";
export const DENSITIES: readonly Density[] = ["dense", "comfortable"];

export function themeDef(id: ThemeId): ThemeDef {
  return THEMES.find((theme) => theme.id === id) ?? THEMES[0];
}

export function paletteOf(id: ThemeId): string {
  return themeDef(id).accents[0];
}

export function parseThemeChoice(value: unknown): ThemeChoice | null {
  return typeof value === "string" &&
    (THEME_CHOICES as readonly string[]).includes(value)
    ? (value as ThemeChoice)
    : null;
}

export function parseThemeId(value: unknown): ThemeId | null {
  return typeof value === "string" &&
    (THEME_IDS as readonly string[]).includes(value)
    ? (value as ThemeId)
    : null;
}

export function parseDensity(value: unknown): Density | null {
  return typeof value === "string" &&
    (DENSITIES as readonly string[]).includes(value)
    ? (value as Density)
    : null;
}

/** Resolve a stored choice to a concrete theme id. `system` follows the OS. */
export function resolveThemeId(
  choice: ThemeChoice,
  prefersDark: boolean,
): ThemeId {
  if (choice !== "system") return choice;
  return prefersDark ? "dark" : "light";
}

export function prefersDarkScheme(win?: Window | null): boolean {
  try {
    return win?.matchMedia?.("(prefers-color-scheme: dark)").matches ?? true;
  } catch {
    return true;
  }
}

/* ---------------------------------------------------------------------------
 * Colour maths — the JS mirror of the color-mix() derivations in tokens.css.
 * ------------------------------------------------------------------------ */

export type RGB = { r: number; g: number; b: number };

const HEX6 = /^#[0-9a-f]{6}$/i;

export function parseHex(hex: string): RGB | null {
  if (!HEX6.test(hex)) return null;
  return {
    r: parseInt(hex.slice(1, 3), 16),
    g: parseInt(hex.slice(3, 5), 16),
    b: parseInt(hex.slice(5, 7), 16),
  };
}

/** Parses `#rrggbb` or `rgb()`/`rgba()` (computed values); null otherwise.
 * Alpha is dropped — callers composite explicitly when they need it. */
export function parseCssColor(value: string): RGB | null {
  const hex = parseHex(value.trim());
  if (hex) return hex;
  const match = /^rgba?\(\s*(\d{1,3})\s*,\s*(\d{1,3})\s*,\s*(\d{1,3})/i.exec(
    value.trim(),
  );
  if (!match) return null;
  const [r, g, b] = [Number(match[1]), Number(match[2]), Number(match[3])];
  if (
    [r, g, b].some(
      (channel) => !Number.isInteger(channel) || channel < 0 || channel > 255,
    )
  ) {
    return null;
  }
  return { r, g, b };
}

export function toHex(colour: RGB): string {
  const channel = (value: number) =>
    Math.max(0, Math.min(255, Math.round(value)))
      .toString(16)
      .padStart(2, "0");
  return `#${channel(colour.r)}${channel(colour.g)}${channel(colour.b)}`;
}

/** Opaque mix of two colours, `fraction` of the way toward `other`. */
export function mixWith(hex: string, other: string, fraction: number): string {
  const base = parseHex(hex);
  const ink = parseHex(other);
  if (!base || !ink) return hex;
  const weight = Math.max(0, Math.min(1, fraction));
  return toHex({
    r: base.r * (1 - weight) + ink.r * weight,
    g: base.g * (1 - weight) + ink.g * weight,
    b: base.b * (1 - weight) + ink.b * weight,
  });
}

/** Hue-preserving alpha scaling: `color-mix(in srgb, hex N%, transparent)`. */
export function withAlpha(hex: string, alpha: number): string {
  const base = parseHex(hex);
  if (!base) return hex;
  const clamped = Math.max(0, Math.min(1, alpha));
  return `rgba(${base.r}, ${base.g}, ${base.b}, ${clamped.toFixed(3)})`;
}

/**
 * The derivation spec, mirroring `tokens.css` — keep the two in step. Tints mix
 * toward transparent; text and ring mix toward the palette's ink endpoint.
 */
export const ACCENT_DERIVATIONS = {
  tintAlpha: 0.22, // 22% accent + transparent — selected row
  tintStrongAlpha: 0.3, // 30% — active sidebar item, chips, graph fill
  textInk: 0.45, // 55% accent + 45% ink — text on a tinted chip
  ringInk: 0.55, // 45% accent + 55% ink — focus ring
} as const;

export type DerivedTokens = {
  tint: string;
  tintStrong: string;
  text: string;
  ring: string;
};

/** The JS mirror of the `--accent-*` and `--focus-ring` derivations. */
export function deriveAccentTokens(
  accent: string,
  ink = "#ffffff",
): DerivedTokens {
  return {
    tint: withAlpha(accent, ACCENT_DERIVATIONS.tintAlpha),
    tintStrong: withAlpha(accent, ACCENT_DERIVATIONS.tintStrongAlpha),
    text: mixWith(accent, ink, ACCENT_DERIVATIONS.textInk),
    ring: mixWith(accent, ink, ACCENT_DERIVATIONS.ringInk),
  };
}

/** WCAG 2.1 relative luminance (0..1). */
export function luminance(colour: RGB): number {
  const channel = (value: number) => {
    const scaled = value / 255;
    return scaled <= 0.03928
      ? scaled / 12.92
      : Math.pow((scaled + 0.055) / 1.055, 2.4);
  };
  return (
    0.2126 * channel(colour.r) +
    0.7152 * channel(colour.g) +
    0.0722 * channel(colour.b)
  );
}

/** WCAG contrast ratio (1..21); NaN when either colour cannot be parsed. */
export function contrastRatio(a: string, b: string): number {
  const first = parseCssColor(a);
  const second = parseCssColor(b);
  if (!first || !second) return NaN;
  const [high, low] = [luminance(first), luminance(second)].sort(
    (x, y) => y - x,
  );
  return (high + 0.05) / (low + 0.05);
}

/** Readable text colour for a filled chip or dot: white or near-black,
 * whichever contrasts better. Used for user-chosen label colours. */
export function readableTextOn(background: string): string {
  const onWhite = contrastRatio(background, "#ffffff");
  const onBlack = contrastRatio(background, "#0b0c0d");
  if (Number.isNaN(onWhite)) return "#ffffff";
  return onWhite >= onBlack ? "#ffffff" : "#0b0c0d";
}

/** Tinted chip colours for a user-chosen label colour: the colour at 22% over
 * the app surface, with contrast-safe text. Returns undefined when the caller
 * has no colour for that label (the built-in palette classes apply instead). */
export function labelChipStyle(
  colour: string | undefined,
  surface: string,
): { background: string; color: string } | undefined {
  if (!colour || !parseHex(colour)) return undefined;
  const background = mixWith(colour, surface, 0.78);
  return { background, color: readableTextOn(background) };
}

/* ---------------------------------------------------------------------------
 * Applying and remembering a theme
 * ------------------------------------------------------------------------ */

export const THEME_STORAGE_KEY = "rstorrent.theme.v1";
export const DENSITY_STORAGE_KEY = "rstorrent.density.v1";
/** Operator default, injected into the HTML as a meta tag (WC7-S5 sets it). */
export const THEME_DEFAULT_META = "rstorrent-theme-default";
export const ACCENT_DEFAULT_META = "rstorrent-accent-default";

/** The accent tokens CSS derives; only written when the browser needs them. */
const DERIVED_PROPS = [
  "--accent-tint",
  "--accent-tint-strong",
  "--accent-text",
  "--focus-ring",
] as const;

function rootOf(doc?: Document): HTMLElement | null {
  const target =
    doc ?? (typeof document === "undefined" ? undefined : document);
  return target?.documentElement ?? null;
}

function storageOrNull(storage?: Storage | null): Storage | null {
  if (storage !== undefined) return storage;
  try {
    return typeof localStorage === "undefined" ? null : localStorage;
  } catch {
    // Private mode or a blocked origin: the choice lasts the session.
    return null;
  }
}

/** The browser's stored choice, or null when it follows the operator default. */
export function loadThemeChoice(storage?: Storage | null): ThemeChoice | null {
  try {
    return parseThemeChoice(storageOrNull(storage)?.getItem(THEME_STORAGE_KEY));
  } catch {
    return null;
  }
}

export function saveThemeChoice(
  choice: ThemeChoice | null,
  storage?: Storage | null,
): void {
  try {
    const store = storageOrNull(storage);
    if (!store) return;
    if (choice === null) store.removeItem(THEME_STORAGE_KEY);
    else store.setItem(THEME_STORAGE_KEY, choice);
  } catch {
    /* private mode: the choice lasts the session */
  }
}

export function loadDensity(storage?: Storage | null): Density {
  try {
    return (
      parseDensity(storageOrNull(storage)?.getItem(DENSITY_STORAGE_KEY)) ??
      "dense"
    );
  } catch {
    return "dense";
  }
}

export function saveDensity(density: Density, storage?: Storage | null): void {
  try {
    storageOrNull(storage)?.setItem(DENSITY_STORAGE_KEY, density);
  } catch {
    /* private mode */
  }
}

/** Applies a theme id to `<html>`, plus the `color-scheme` native controls use. */
export function applyThemeId(id: ThemeId, doc?: Document): ThemeId {
  const root = rootOf(doc);
  if (!root) return id;
  root.dataset.theme = id;
  root.style.colorScheme = themeDef(id).dark ? "dark" : "light";
  return id;
}

export function applyDensity(density: Density, doc?: Document): void {
  const root = rootOf(doc);
  if (root) root.dataset.density = density;
}

/** True when the browser resolves `color-mix()` in authored styles. */
export function supportsColorMix(doc?: Document): boolean {
  const target =
    doc ?? (typeof document === "undefined" ? undefined : document);
  const css = target?.defaultView?.CSS;
  if (!css || typeof css.supports !== "function") return false;
  try {
    return css.supports("color", "color-mix(in srgb, red 50%, white)");
  } catch {
    return false;
  }
}

/** Reads a computed custom property, trimmed; "" when unavailable. */
export function readToken(name: string, doc?: Document): string {
  const target =
    doc ?? (typeof document === "undefined" ? undefined : document);
  const root = rootOf(target);
  if (!root || typeof target?.defaultView?.getComputedStyle !== "function")
    return "";
  try {
    return target.defaultView
      .getComputedStyle(root)
      .getPropertyValue(name)
      .trim();
  } catch {
    return "";
  }
}

/**
 * Sets the one tweakable accent. Where `color-mix()` is available CSS owns the
 * derived tokens, so any values a previous application set inline are removed;
 * otherwise the JS mirror fills them in, reading the theme's ink endpoint so
 * light themes keep readable accent text.
 *
 * Returns true when the accent was valid and applied.
 */
export function applyAccent(accent: string, doc?: Document): boolean {
  if (!parseHex(accent)) return false;
  const root = rootOf(doc);
  if (!root) return false;
  root.style.setProperty("--accent", accent);
  if (supportsColorMix(doc)) {
    for (const property of DERIVED_PROPS) root.style.removeProperty(property);
  } else {
    const ink = readToken("--accent-ink", doc) || "#ffffff";
    const derived = deriveAccentTokens(accent, ink);
    root.style.setProperty("--accent-tint", derived.tint);
    root.style.setProperty("--accent-tint-strong", derived.tintStrong);
    root.style.setProperty("--accent-text", derived.text);
    root.style.setProperty("--focus-ring", derived.ring);
  }
  return true;
}

/** Drops an explicit accent so the theme's own default shows again. */
export function clearAccent(doc?: Document): void {
  const root = rootOf(doc);
  if (!root) return;
  root.style.removeProperty("--accent");
  for (const property of DERIVED_PROPS) root.style.removeProperty(property);
}

/** The operator's default choice from the injected meta tag, if any. */
export function serverThemeChoice(doc?: Document): ThemeChoice | null {
  const target =
    doc ?? (typeof document === "undefined" ? undefined : document);
  try {
    return parseThemeChoice(
      target
        ?.querySelector(`meta[name="${THEME_DEFAULT_META}"]`)
        ?.getAttribute("content"),
    );
  } catch {
    return null;
  }
}

/** The operator's default accent from the injected meta tag, if any. */
export function serverAccent(doc?: Document): string | null {
  const target =
    doc ?? (typeof document === "undefined" ? undefined : document);
  try {
    const value = target
      ?.querySelector(`meta[name="${ACCENT_DEFAULT_META}"]`)
      ?.getAttribute("content");
    return value && parseHex(value) ? value : null;
  } catch {
    return null;
  }
}

/** Keeps the `theme-color` meta in step with the app background. */
export function syncThemeColor(doc?: Document): void {
  const target =
    doc ?? (typeof document === "undefined" ? undefined : document);
  if (!target) return;
  const colour = readToken("--bg-app", doc);
  if (!colour) return;
  let meta = target.querySelector('meta[name="theme-color"]');
  if (!meta) {
    meta = target.createElement("meta");
    meta.setAttribute("name", "theme-color");
    target.head?.appendChild(meta);
  }
  meta.setAttribute("content", colour);
}

export interface ThemeState {
  choice: ThemeChoice;
  resolved: ThemeId;
  accent: string | null;
}

/**
 * Resolves and applies the theme: the browser's stored choice wins, then the
 * operator default, then dark. Returns what was applied.
 */
export function applyTheme(
  choice: ThemeChoice,
  accent: string | null,
  doc?: Document,
): ThemeId {
  const win = (doc?.defaultView ?? null) as Window | null;
  // Without a window (tests, SSR) assume dark, which is what the palettes default to.
  const resolved = resolveThemeId(choice, win ? prefersDarkScheme(win) : true);
  applyThemeId(resolved, doc);
  if (accent) applyAccent(accent, doc);
  else clearAccent(doc);
  syncThemeColor(doc);
  return resolved;
}

/** The stored choice, else the operator default, else dark. */
export function initialChoice(
  storage?: Storage | null,
  doc?: Document,
): ThemeChoice {
  return loadThemeChoice(storage) ?? serverThemeChoice(doc) ?? DEFAULT_THEME_ID;
}

/**
 * Boot wiring: apply the theme and density now, then follow the OS while the
 * choice is `system`. Returns a cleanup for the media-query listener.
 *
 * The HTML shells run the same resolution in a tiny inline script before first
 * paint, so this normally re-applies what is already on screen.
 */
export function initTheme(options?: {
  doc?: Document;
  storage?: Storage | null;
  onSystemChange?: (state: ThemeState) => void;
}): () => void {
  const doc =
    options?.doc ?? (typeof document === "undefined" ? undefined : document);
  const storage = options?.storage;
  const choice = initialChoice(storage, doc);
  const accent = serverAccent(doc);
  applyTheme(choice, accent, doc);
  applyDensity(loadDensity(storage), doc);

  const win = (doc?.defaultView ?? null) as (Window & typeof globalThis) | null;
  if (choice !== "system" || !win?.matchMedia) return () => {};

  const query = win.matchMedia("(prefers-color-scheme: dark)");
  const onChange = () => {
    const next = applyTheme("system", accent, doc);
    options?.onSystemChange?.({ choice, resolved: next, accent });
  };
  query.addEventListener("change", onChange);
  return () => query.removeEventListener("change", onChange);
}
