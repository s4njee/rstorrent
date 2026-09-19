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
/**
 * A compact duration as the design writes it: `45s`, `4m 12s`, `1h 04m`,
 * `2d 04h`. Minutes pair with seconds and hours with minutes; the second field
 * is zero-padded so the column stays aligned.
 */
export function formatDuration(totalSeconds: number): string {
  const s = Math.max(0, Math.floor(totalSeconds));
  if (s < 60) return `${s}s`;
  if (s < 3600) {
    return `${Math.floor(s / 60)}m ${String(s % 60).padStart(2, "0")}s`;
  }
  if (s < 86_400) {
    const m = Math.floor((s % 3600) / 60);
    return `${Math.floor(s / 3600)}h ${String(m).padStart(2, "0")}m`;
  }
  const h = Math.floor((s % 86_400) / 3600);
  return `${Math.floor(s / 86_400)}d ${String(h).padStart(2, "0")}h`;
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
 * The Added cell: `12:04 today` for today, `Aug 30` for anything older, and —
 * for rtorrent's "unknown" (0). Days difference is calendar-based, so a torrent
 * added at 23:50 yesterday reads as yesterday however long ago that was.
 */
export function formatAdded(unixSeconds: number, now = new Date()): string {
  if (!unixSeconds) return "—";
  const at = new Date(unixSeconds * 1000);
  if (Number.isNaN(at.getTime())) return "—";

  const midnight = (d: Date) =>
    new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  const days = Math.round((midnight(now) - midnight(at)) / 86_400_000);

  if (days === 0) {
    const hh = String(at.getHours()).padStart(2, "0");
    const mm = String(at.getMinutes()).padStart(2, "0");
    return `${hh}:${mm} today`;
  }
  if (days === 1) return "yesterday";
  // Same-year dates drop the year, as the design's `Aug 30` does.
  return at.toLocaleDateString(undefined, {
    year: at.getFullYear() === now.getFullYear() ? undefined : "2-digit",
    month: "short",
    day: "numeric",
  });
}

/**
 * Countdown to a future Unix time: "in 12m". Returns "—" for 0 (unset) or a
 * past time — rtorrent leaves the next-announce time in the past for a tracker
 * that's overdue or failing, and "115s ago" would misread as informative.
 */
export function formatCountdown(unixSeconds: number, now = Date.now()): string {
  if (!unixSeconds) return "—";
  const deltaSec = Math.round(unixSeconds - now / 1000);
  return deltaSec > 0 ? `in ${formatDuration(deltaSec)}` : "—";
}

/**
 * Elapsed time since a past Unix time: "4m ago". "—" for 0 (never happened).
 * Used for a tracker's last successful announce.
 */
export function formatAgo(unixSeconds: number, now = Date.now()): string {
  if (!unixSeconds) return "—";
  const deltaSec = Math.max(0, Math.round(now / 1000 - unixSeconds));
  return `${formatDuration(deltaSec)} ago`;
}

/**
 * A Unix-seconds timestamp as a compact date for the Added/Finished columns.
 * 0 (rtorrent's "unknown") renders as —. Same-year dates drop the year to save
 * width; older ones keep it. Absolute, not relative: it doesn't drift between
 * polls and needs no ticking clock.
 */
/**
 * Session uptime as the design writes it: `41d 06h`, `6h 12m`, `4m 09s`, `12s`.
 * Measured by the app (from its first snapshot), because rtorrent does not
 * report when it started.
 */
export function formatUptime(totalSeconds: number): string {
  const seconds = Math.max(0, Math.floor(totalSeconds));
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) {
    return `${Math.floor(seconds / 60)}m ${String(seconds % 60).padStart(2, "0")}s`;
  }
  if (seconds < 86_400) {
    const minutes = Math.floor((seconds % 3600) / 60);
    return `${Math.floor(seconds / 3600)}h ${String(minutes).padStart(2, "0")}m`;
  }
  const hours = Math.floor((seconds % 86_400) / 3600);
  return `${Math.floor(seconds / 86_400)}d ${String(hours).padStart(2, "0")}h`;
}

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
