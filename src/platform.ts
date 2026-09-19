/**
 * Which OS the browser runs on, and the conventions that follow from it.
 *
 * The user agent is enough here: nothing in the UI branches on anything finer
 * grained than "macOS or not", and that distinction is unambiguous in the UA
 * string. Computed once at module load — it cannot change mid-session.
 */

const ua =
  typeof navigator === "undefined"
    ? ""
    : navigator.userAgent + " " + (navigator.platform ?? "");

const isMac = /Mac|iPhone|iPad/i.test(ua);

/**
 * Render a keyboard shortcut the way the platform writes it: macOS uses bare
 * glyphs (`⌘⇧O`), elsewhere the modifiers are spelled out and joined with `+`
 * (`Ctrl+Shift+O`).
 */
export function accel(key: string, opts: { shift?: boolean } = {}): string {
  if (isMac) return `⌘${opts.shift ? "⇧" : ""}${key}`;
  return `Ctrl+${opts.shift ? "Shift+" : ""}${key}`;
}
