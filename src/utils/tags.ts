/**
 * Tag chips and normalisation (V3-10).
 *
 * A tag is a many-to-many label stored in rtorrent's `d.custom=tags` (a
 * comma-separated string), so it follows the torrent across daemon restarts and
 * across every client. The label stays the single category.
 *
 * rtorrent stores no colour for a tag, so the colour is the app's: a fixed
 * palette picked by a hash of the name, so a tag looks the same everywhere
 * without a settings round-trip. `normaliseTags` mirrors the core's normaliser
 * (`crates/rtorrent/src/tags.rs`) so the editor never sends a name the daemon
 * would store differently.
 */

import { labelChipStyle } from "../theme/theme";
import type { LabelChip } from "./labels";

/** Longest tag accepted, matching the core. */
const MAX_TAG_LEN = 64;

/** The tag palette; each is a contrast-safe hue on the dark surface. */
const TAG_COLOURS = [
  "#4f9ecf",
  "#7daea3",
  "#c9a86a",
  "#b47cc7",
  "#d08989",
  "#8fbf76",
  "#c79a5b",
  "#6fa8b8",
];

/**
 * Trim, drop empties, drop the comma separator, cap the length and de-duplicate
 * case-insensitively (first spelling wins). Mirrors `tags::normalise`.
 */
export function normaliseTags(tags: readonly string[]): string[] {
  const out: string[] = [];
  const seen = new Set<string>();
  for (const raw of tags) {
    const cleaned = raw
      .replace(/,/g, "")
      .trim()
      .slice(0, MAX_TAG_LEN)
      .trimEnd();
    if (!cleaned) continue;
    const key = cleaned.toLowerCase();
    if (seen.has(key)) continue;
    seen.add(key);
    out.push(cleaned);
  }
  return out;
}

/** A stable colour for a tag name, from the app's palette. */
export function tagColour(tag: string): string {
  let hash = 0;
  for (const ch of tag.toLowerCase()) {
    hash = (hash * 31 + ch.charCodeAt(0)) >>> 0;
  }
  return TAG_COLOURS[hash % TAG_COLOURS.length];
}

/** The tinted chip for a tag, mirroring `labelChipForLabel`. */
export function tagChip(tag: string, surface = "#101214"): LabelChip {
  const style = labelChipStyle(tagColour(tag), surface);
  if (style)
    return { text: tag, background: style.background, color: style.color };
  return { text: tag };
}

/** The union of the tags across `lists`, normalised and in first-seen order. */
export function unionTags(lists: ReadonlyArray<readonly string[]>): string[] {
  return normaliseTags(lists.flat());
}
