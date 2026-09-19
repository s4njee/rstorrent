/**
 * Parsing for the add modal's magnet/URL pane (WC6-S2).
 *
 * Pure so the per-line validation the pane shows is the same rule the add path
 * used before, just applied line by line: a valid entry is a `magnet:` URI with
 * a btih hash, or an `http(s)` URL. Blank lines are ignored rather than
 * reported as errors.
 */

export interface SourceLine {
  /** The trimmed text, which is what gets added. */
  value: string;
  /** False when the line is neither a magnet nor a URL; the pane marks it. */
  valid: boolean;
}

/** True when `text` is a magnet with a btih hash, or an http(s) `.torrent` URL. */
export function isLoadableSource(text: string): boolean {
  const trimmed = text.trim();
  if (/^magnet:\?.*xt=urn:btih:[0-9a-z]+/i.test(trimmed)) return true;
  return /^https?:\/\/.+/i.test(trimmed);
}

/** The btih info-hash from a magnet, uppercased, or null. Mirrors `magnet_hash`. */
export function magnetHash(uri: string): string | null {
  const match = /[?&]xt=urn:btih:([0-9a-f]{40})/i.exec(uri);
  return match ? match[1].toUpperCase() : null;
}

/** The display name from a magnet's `dn=`, with `+` as a space, or "". Mirrors
 *  `magnet_name` (no percent-decoding, so both shells agree). */
export function magnetName(uri: string): string {
  const match = /[?&]dn=([^&]+)/i.exec(uri);
  if (!match) return "";
  return match[1].split("#")[0].replace(/\+/g, " ").trim();
}

/** The `tr=` announce URLs in a magnet, in order. */
export function magnetTrackers(uri: string): string[] {
  const out: string[] = [];
  for (const part of uri.split(/[&?]/)) {
    if (!part.startsWith("tr=")) continue;
    const value = part.slice(3).split("#")[0];
    if (value) out.push(value);
  }
  return out;
}

/**
 * Split a textarea into one entry per non-blank line, each validated. The order
 * is preserved so per-line failures can be reported against what was typed.
 */
export function parseSourceLines(text: string): SourceLine[] {
  return text
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line.length > 0)
    .map((value) => ({ value, valid: isLoadableSource(value) }));
}
