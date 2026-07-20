/**
 * Which desktop we're running on, and the conventions that follow from it.
 *
 * The webview's user agent is enough here and costs no extra plugin or IPC
 * round trip: nothing in the UI branches on anything finer-grained than
 * "macOS or not", and that distinction is unambiguous in the UA string.
 *
 * Everything is computed once at module load — the platform cannot change
 * mid-session, and keeping it constant lets these be used in module scope.
 */

const ua =
  typeof navigator === "undefined"
    ? ""
    : navigator.userAgent + " " + (navigator.platform ?? "");

export const isMac = /Mac|iPhone|iPad/i.test(ua);
export const isWindows = /Win/i.test(ua) && !isMac;

/**
 * Render a keyboard shortcut the way the host platform writes it: macOS uses
 * bare glyphs (`⌘⇧O`), Windows spells the modifiers out and joins with `+`
 * (`Ctrl+Shift+O`).
 */
export function accel(key: string, opts: { shift?: boolean } = {}): string {
  if (isMac) return `⌘${opts.shift ? "⇧" : ""}${key}`;
  return `Ctrl+${opts.shift ? "Shift+" : ""}${key}`;
}

/**
 * What the platform calls its file manager, for menu items and error copy.
 */
export const fileManagerName = isMac
  ? "Finder"
  : isWindows
    ? "Explorer"
    : "file manager";

/**
 * What the platform calls its credential store, for the Preferences hint that
 * explains where a remote daemon's password is kept.
 */
export const credentialStoreName = isMac
  ? "macOS Keychain"
  : isWindows
    ? "Windows Credential Manager"
    : "system keyring";

/**
 * Expose the platform to CSS as `html[data-platform]`, so stylesheets can drop
 * mac-only affordances (the traffic-light gutter) without threading a prop
 * through every component.
 */
if (typeof document !== "undefined") {
  document.documentElement.dataset.platform = isMac
    ? "mac"
    : isWindows
      ? "windows"
      : "other";
}
