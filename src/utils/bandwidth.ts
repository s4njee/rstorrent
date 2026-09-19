/**
 * Bandwidth rules (V3-18 / QUE-04) — TS mirror of the matching half of
 * `crates/rtorrent/src/bandwidth.rs`.
 *
 * Pure: first matching tag rule wins, then the label rule; the precedence
 * display reads torrent override → rule → turtle → global. Keep in lockstep
 * with the Rust module; the tests on both sides pin the shared behaviour.
 */

import type { BandwidthRule } from "../ipc/types";

/** First matching tag rule (in order), then the label rule. */
export function ruleFor(
  label: string,
  tags: readonly string[],
  rules: readonly BandwidthRule[],
): BandwidthRule | null {
  const lowered = tags.map((t) => t.toLowerCase());
  for (const rule of rules) {
    if (!rule.id.trim()) continue;
    const tag = rule.tag?.trim();
    if (tag && lowered.includes(tag.toLowerCase())) return rule;
  }
  const key = label.trim().toLowerCase();
  if (key) {
    for (const rule of rules) {
      if (!rule.id.trim() || rule.tag != null) continue;
      if (rule.label?.trim().toLowerCase() === key) return rule;
    }
  }
  return null;
}

/** Human match descriptor: `tag "x"`. */
export function describeRule(rule: BandwidthRule): string {
  const tag = rule.tag?.trim();
  if (tag) return `tag "${tag}"`;
  const label = rule.label?.trim();
  if (label) return `label "${label}"`;
  return "untargeted";
}

export type LimitSource =
  | { kind: "manual" }
  | { kind: "rule"; describe: string }
  | { kind: "turtle" }
  | { kind: "global" };

/** Where limits come from: an explicit throttle with no rule marker is manual. */
export function limitSource(
  throttle: string,
  marker: string | undefined,
  hasOwnLimit: boolean,
  rule: BandwidthRule | null,
  turtleActive: boolean,
): LimitSource {
  if (!(marker ?? "").trim() && (hasOwnLimit || !!throttle.trim())) {
    return { kind: "manual" };
  }
  if (rule) return { kind: "rule", describe: describeRule(rule) };
  if (turtleActive) return { kind: "turtle" };
  return { kind: "global" };
}

/** The precedence word the limit rows show. */
export function sourceWord(source: LimitSource): string {
  switch (source.kind) {
    case "manual":
      return "torrent override";
    case "rule":
      return `rule ${source.describe}`;
    case "turtle":
      return "turtle";
    case "global":
      return "global";
  }
}
