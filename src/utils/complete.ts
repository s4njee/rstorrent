/**
 * Move-on-complete + incomplete-data folder (V3-14) — TS mirror of
 * `crates/rtorrent/src/complete.rs`.
 *
 * Pure: destination resolution (first matching tag rule, then label rule,
 * then default), incomplete-dir routing, collision-safe naming, and the
 * free-space preflight. Keep in lockstep with the Rust module; the tests on
 * both sides pin the shared behaviour.
 */

export interface MoveRule {
  tag?: string;
  label?: string;
  destination: string;
}

export function tagRule(tag: string, destination: string): MoveRule {
  return { tag: tag.trim().toLowerCase(), destination: destination.trim() };
}

export function labelRule(label: string, destination: string): MoveRule {
  return { label: label.trim().toLowerCase(), destination: destination.trim() };
}

/** Where finished data belongs: tag rule → label rule → default. */
export function resolveDestination(
  label: string,
  tags: readonly string[],
  rules: readonly MoveRule[],
  defaultDir: string,
): string {
  const lowered = tags.map((t) => t.toLowerCase());
  for (const rule of rules) {
    const tag = rule.tag?.trim().toLowerCase();
    if (tag && lowered.includes(tag) && rule.destination.trim()) {
      return rule.destination.trim();
    }
  }
  const labelKey = label.trim().toLowerCase();
  if (labelKey) {
    for (const rule of rules) {
      if (
        rule.tag == null &&
        rule.label?.trim().toLowerCase() === labelKey &&
        rule.destination.trim()
      ) {
        return rule.destination.trim();
      }
    }
  }
  return defaultDir;
}

/** Where a *completing* torrent belongs: rules, then C11 label paths, then default. */
export function completionDestination(
  label: string,
  tags: readonly string[],
  moveRules: readonly MoveRule[],
  labelPaths: ReadonlyArray<{ label: string; savePath: string }>,
  defaultDir: string,
): string {
  const combined: MoveRule[] = [...moveRules];
  for (const { label: l, savePath } of labelPaths) {
    if (l.trim() && savePath.trim()) combined.push(labelRule(l, savePath));
  }
  return resolveDestination(label, tags, combined, defaultDir);
}

/**
 * Route a new download: the directory to load it with, plus the final
 * directory to record (if any). No incomplete dir, nothing chosen, or
 * already home → identity with no recording.
 */
export function routeNewDownload(
  incompleteDir: string,
  chosenDir: string,
): { directory: string; finalDir: string | null } {
  const incomplete = incompleteDir.trim();
  const chosen = chosenDir.trim();
  if (!incomplete || !chosen) return { directory: chosen, finalDir: null };
  if (
    incomplete.replace(/\/+$/, "").toLowerCase() ===
    chosen.replace(/\/+$/, "").toLowerCase()
  ) {
    return { directory: chosen, finalDir: null };
  }
  return { directory: incomplete, finalDir: chosen };
}

export interface MovePlan {
  src: string;
  dst: string;
}

/** `null` when already home or inputs are missing. */
export function planMove(
  currentDir: string,
  destination: string,
  name: string,
): MovePlan | null {
  const current = currentDir.trim().replace(/\/+$/, "");
  const dest = destination.trim().replace(/\/+$/, "");
  const cleanName = name.trim();
  if (!dest || !cleanName || !current) return null;
  if (current.toLowerCase() === dest.toLowerCase()) return null;
  return { src: `${current}/${cleanName}`, dst: `${dest}/${cleanName}` };
}

export type CollisionPolicy = "error" | "auto-rename";

function splitExt(name: string): [string, string] {
  // Mirrors the Rust side: only a trailing dot + 2–5 ASCII letters is an
  // extension ("Movie.mkv"); dotted folders ("Show.S01") suffix at the end.
  const idx = name.lastIndexOf(".");
  if (idx > 0) {
    const ext = name.slice(idx + 1);
    if (/^[A-Za-z]{2,5}$/.test(ext)) return [name.slice(0, idx), ext];
  }
  return [name, ""];
}

/** Pick a free target under `dstDir`; `siblingsLower` are known taken names. */
export function resolveCollision(
  dstDir: string,
  name: string,
  siblingsLower: readonly string[],
  policy: CollisionPolicy,
): string {
  const dir = dstDir.trim().replace(/\/+$/, "");
  const clean = name.trim();
  if (!siblingsLower.includes(clean.toLowerCase())) return `${dir}/${clean}`;
  if (policy === "error")
    throw new Error(`destination already exists: ${dir}/${clean}`);
  const [stem, ext] = splitExt(clean);
  for (let n = 1; n < 1000; n++) {
    const candidate = ext ? `${stem} (${n}).${ext}` : `${stem} (${n})`;
    if (!siblingsLower.includes(candidate.toLowerCase()))
      return `${dir}/${candidate}`;
  }
  throw new Error(`destination already exists: ${dir}/${clean}`);
}

export type Preflight =
  | { ok: true; unknown?: boolean }
  | { ok: false; required: number; free: number };

/** `remaining` is size − done clamped at zero; `free` prefers d.free_diskspace. */
export function preflight(remaining: number, free: number | null): Preflight {
  if (remaining <= 0) return { ok: true };
  if (free == null) return { ok: true, unknown: true };
  if (free >= remaining) return { ok: true };
  return { ok: false, required: remaining, free };
}
