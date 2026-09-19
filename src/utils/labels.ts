/**
 * Label chips.
 *
 * rtorrent has one label string per torrent and no colours, so a chip's colour
 * is the app's. The design names five label families with a fixed hue each and a
 * neutral chip for anything else (including unlabelled rows).
 *
 * A deployment can override a label's colour in settings (WC4-S3); that path
 * composes a tint from the chosen hue with `labelChipStyle` in `theme.ts`, which
 * is the same maths used for the accent.
 */

import { labelChipStyle } from "../theme/theme";

/** The design's label palette, as CSS custom properties. */
const LABEL_TOKENS: Record<string, string> = {
  iso: "var(--label-iso)",
  archive: "var(--label-archive)",
  kernel: "var(--label-kernel)",
  apps: "var(--label-apps)",
  media: "var(--label-media)",
};

export interface LabelChip {
  /** The label, or "unlabeled" for an empty one. */
  text: string;
  /** Tinted chip background; undefined means "use the neutral chip class". */
  background?: string;
  /** Text colour for a custom (hex) chip. */
  color?: string;
}

/** True when the label has a built-in hue (rendered from a token, not a hex). */
export function isBuiltinLabel(label: string): boolean {
  return label in LABEL_TOKENS;
}

/** The CSS variable holding a built-in label's hue, if it has one. */
export function labelToken(label: string): string | undefined {
  return LABEL_TOKENS[label];
}

/**
 * The chip for a label: a built-in hue, a deployment-configured colour, or the
 * neutral chip. `colours` maps label name to a hex chosen in settings.
 */
export function labelChip(
  label: string,
  colours: Record<string, string> = {},
  surface = "#101214",
): LabelChip {
  if (!label) return { text: "unlabeled" };
  const custom = colours[label];
  if (custom) {
    const style = labelChipStyle(custom, surface);
    if (style)
      return { text: label, background: style.background, color: style.color };
  }
  // A built-in hue renders through its token so it follows the theme; the chip
  // background is the accent-tint treatment at the same 22% the tints use.
  if (isBuiltinLabel(label)) return { text: label };
  return { text: label };
}
