/**
 * Sidebar disk card (WE2-S4).
 *
 * Pinned at the bottom of the filter sidebar: a "DISK / N GiB free" caption and
 * a 5px usage bar filled to the used fraction. Hidden entirely when the free or
 * total figure is unknown (a remote daemon we can't stat, per the desktop's
 * localhost gating).
 */

import { formatBytes } from "../utils/format";

/** Used fraction (0..1) of a volume, or null when it can't be computed. */
export function usedFraction(
  freeSpace: number | null,
  diskSize: number | null,
): number | null {
  if (freeSpace == null || diskSize == null || diskSize <= 0) return null;
  return Math.min(1, Math.max(0, 1 - freeSpace / diskSize));
}

export function DiskCard({
  freeSpace,
  diskSize,
}: {
  freeSpace: number | null;
  diskSize: number | null;
}) {
  const used = usedFraction(freeSpace, diskSize);
  if (used == null) return null;

  return (
    <div style={S.card}>
      <div style={S.caption}>
        <span>Disk</span>
        <span>{freeSpace != null ? `${formatBytes(freeSpace)} free` : ""}</span>
      </div>
      <div style={S.track}>
        <div style={{ ...S.fill, width: `${Math.round(used * 100)}%` }} />
      </div>
    </div>
  );
}

const S = {
  card: {
    margin: "16px 8px 8px",
    padding: 10,
    border: "1px solid var(--border-mid)",
    borderRadius: "var(--radius-card, 6px)",
    background: "var(--bg-row-alt)",
  } as const,
  caption: {
    display: "flex",
    justifyContent: "space-between",
    fontSize: 9.5,
    color: "var(--text-dim)",
    textTransform: "uppercase",
    letterSpacing: ".06em",
    marginBottom: 7,
  } as const,
  track: {
    height: 5,
    borderRadius: 3,
    background: "var(--bg-track)",
    overflow: "hidden",
  } as const,
  fill: { height: "100%", background: "var(--accent-cyan)" } as const,
};
