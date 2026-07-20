/**
 * Human-readable formatting for sizes, rates, durations, and ratios.
 *
 * These are pure functions with no dependencies so they can be unit-tested
 * against the exact strings in the design reference (see format.test.ts). Units
 * are binary (KiB/MiB/GiB…) throughout, matching rtorrent and the mockup.
 */

const BINARY_UNITS = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];

/** Choose a decimal count: one place under 10, none at/above (per the mockup). */
function decimals(value: number): number {
  return value < 10 ? 1 : 0;
}

/**
 * Format a byte count, e.g. 6227702349 → "5.8 GiB", 661651456 → "631 MiB".
 * `B` is always shown without decimals.
 */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < BINARY_UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const d = unit === 0 ? 0 : decimals(value);
  return `${value.toFixed(d)} ${BINARY_UNITS[unit]}`;
}

/** Format a transfer rate, e.g. 8808038 → "8.4 MiB/s", 634880 → "620 KiB/s". */
export function formatRate(bytesPerSec: number): string {
  if (!Number.isFinite(bytesPerSec) || bytesPerSec <= 0) return "0 B/s";
  return `${formatBytes(bytesPerSec)}/s`;
}

/**
 * The Down cell: rate when moving, "0 B/s" while actively (down)loading at zero
 * (stalled), and "—" otherwise (seeding/paused/error have no download).
 */
export function formatDownCell(rate: number, status: string): string {
  if (rate > 0) return formatRate(rate);
  return status === "downloading" || status === "stalled" ? "0 B/s" : "—";
}

/** The Up cell: rate when uploading, else "—". */
export function formatUpCell(rate: number): string {
  return rate > 0 ? formatRate(rate) : "—";
}

/** Compact duration, e.g. 252 → "4m12s", 820 → "13m40s", 45 → "45s". */
export function formatDuration(totalSeconds: number): string {
  const s = Math.max(0, Math.floor(totalSeconds));
  if (s < 60) return `${s}s`;
  if (s < 3600) {
    const m = Math.floor(s / 60);
    return `${m}m${s % 60}s`;
  }
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  return `${h}h${m}m`;
}

/**
 * The ETA cell. A finite value formats as a duration; otherwise the status
 * decides: seeding/stalled show ∞ (indefinite), everything else shows —.
 */
export function formatEta(etaSeconds: number | null, status: string): string {
  if (etaSeconds != null) return formatDuration(etaSeconds);
  return status === "seeding" || status === "stalled" ? "∞" : "—";
}

/** Share ratio to two decimals, e.g. 0.19, 2.41. */
export function formatRatio(ratio: number): string {
  return ratio.toFixed(2);
}

/**
 * A Unix-seconds timestamp as a compact date for the Added/Finished columns.
 * 0 (rtorrent's "unknown") renders as —. Same-year dates drop the year to save
 * width; older ones keep it. Absolute, not relative: it doesn't drift between
 * polls and needs no ticking clock.
 */
export function formatDate(unixSeconds: number, now = new Date()): string {
  if (!unixSeconds) return "—";
  const d = new Date(unixSeconds * 1000);
  const sameYear = d.getFullYear() === now.getFullYear();
  return d.toLocaleDateString(undefined, {
    year: sameYear ? undefined : "2-digit",
    month: "short",
    day: "numeric",
  });
}

/** Free-space status-bar string, or empty when unknown. */
export function formatFree(freeBytes: number | null): string {
  return freeBytes == null ? "" : `free: ${formatBytes(freeBytes)}`;
}
