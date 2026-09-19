/**
 * Sidebar disk footer (design frame 1a): the save path, its used/total figure,
 * and a 4px pressure bar.
 *
 * The bar's colour is the point: it goes from accent to warning at 85% and to
 * error at 95%, so a volume running out of room says so before a download fails.
 * Hidden entirely when the free or total figure is unknown — a remote daemon we
 * can't stat does not get a made-up number.
 */

import { formatBytes } from "../../utils/format";
import styles from "./DiskCard.module.css";

/** Used fraction (0..1) of a volume, or null when it can't be computed. */
export function usedFraction(
  freeSpace: number | null,
  diskSize: number | null,
): number | null {
  if (freeSpace == null || diskSize == null || diskSize <= 0) return null;
  return Math.min(1, Math.max(0, 1 - freeSpace / diskSize));
}

/** Where the design switches the bar's colour. */
export const DISK_WARN_AT = 0.85;
export const DISK_ERROR_AT = 0.95;

export type DiskPressure = "normal" | "warn" | "full";

/**
 * The bar's severity for a used fraction. Reported to the daemon at all three
 * levels: 85% is "start clearing space", 95% is "this is about to fail".
 */
export function diskPressure(used: number): DiskPressure {
  if (used >= DISK_ERROR_AT) return "full";
  if (used >= DISK_WARN_AT) return "warn";
  return "normal";
}

export function DiskCard({
  freeSpace,
  diskSize,
  path,
}: {
  freeSpace: number | null;
  diskSize: number | null;
  /** The volume's path, shown above the figures when known. */
  path?: string | null;
}) {
  const used = usedFraction(freeSpace, diskSize);
  if (used == null || diskSize == null) return null;

  const pressure = diskPressure(used);
  const usedBytes = diskSize - (freeSpace ?? 0);

  return (
    <div className={styles.card}>
      <div className={styles.path} title={path ?? undefined}>
        {path ?? "Disk"}
      </div>
      <div className={styles.caption}>
        <span>
          {formatBytes(usedBytes)} / {formatBytes(diskSize)}
        </span>
        <span className={styles[pressure]}>{Math.round(used * 100)}% used</span>
      </div>
      <div
        className={styles.track}
        role="meter"
        aria-label="Disk usage"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={Math.round(used * 100)}
      >
        <div
          className={`${styles.fill} ${styles[pressure]}`}
          style={{ width: `${Math.round(used * 100)}%` }}
        />
      </div>
      <div className={styles.free}>{formatBytes(freeSpace ?? 0)} free</div>
    </div>
  );
}
