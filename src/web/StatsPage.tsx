/**
 * The Stats route (WC8).
 *
 * Five stat cards, the 60-minute throughput graph the server keeps, the
 * configured volumes, and space by label aggregated from the live snapshot. All
 * formatting is shared with the table and status bar so a figure reads the same
 * everywhere; anything the server cannot answer (a gone volume, the port check
 * WC11 owns) reads as unavailable rather than as zero.
 */

import { useEffect, useMemo, useState } from "react";
import {
  getStats,
  type StatsPayload,
  type VolumeStat,
} from "../ipc/webSettings";
import { useTorrents } from "../store/torrents";
import { spaceByLabel, barFraction } from "../utils/space";
import { throughputGeometry, gridLines } from "../utils/throughput";
import {
  formatBytes,
  formatFree,
  formatRate,
  formatRatio,
  formatUptime,
} from "../utils/format";
import styles from "./StatsPage.module.css";

/** The graph's viewBox; the element scales to its container. */
const GRAPH_W = 800;
const GRAPH_H = 160;

export function StatsPage({ onBack }: { onBack: () => void }) {
  const [stats, setStats] = useState<StatsPayload | null>(null);
  const [error, setError] = useState<string | null>(null);
  const torrents = useTorrents((s) => s.torrents);
  const globals = useTorrents((s) => s.globals);

  useEffect(() => {
    let cancelled = false;
    const load = () =>
      getStats()
        .then((next) => {
          if (!cancelled) {
            setStats(next);
            setError(null);
          }
        })
        .catch((e: unknown) => !cancelled && setError(String(e)));
    void load();
    // The server samples on its slow cadence; 5s is enough to follow it without
    // hammering the daemon.
    const timer = setInterval(load, 5000);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, []);

  const space = useMemo(() => spaceByLabel(torrents), [torrents]);
  const geometry = useMemo(
    () =>
      throughputGeometry(
        (stats?.history ?? []).map((s) => ({ down: s.down, up: s.up })),
        GRAPH_W,
        GRAPH_H,
      ),
    [stats],
  );
  const largestLabel = space.length > 0 ? space[0].bytes : 0;

  if (error && !stats) {
    return (
      <div className={styles.page}>
        <div className={styles.panel}>
          <p className={styles.meta}>
            Could not load statistics: {error}.{" "}
            <button type="button" className={styles.name} onClick={onBack}>
              Back to console
            </button>
          </p>
        </div>
      </div>
    );
  }

  const counts = stats?.counts;
  const knownFree = (stats?.volumes ?? [])
    .map((volume) => volume.free)
    .filter((free): free is number => free !== null);
  const diskFree = knownFree.length
    ? knownFree.reduce((sum, free) => sum + free, 0)
    : null;

  return (
    <div className={styles.page} data-testid="stats-page">
      <div className={styles.panel}>
        <div className={styles.cards}>
          <Card
            caption="Download"
            value={formatRate(globals.downRate)}
            sub={`${counts?.downloading ?? 0} downloading`}
            valueClass={styles.valueDown}
          />
          <Card
            caption="Upload"
            value={formatRate(globals.upRate)}
            sub={`${counts?.seeding ?? 0} seeding`}
            valueClass={styles.valueUp}
          />
          <Card
            caption="Session ratio"
            value={
              stats?.sessionRatio != null
                ? formatRatio(stats.sessionRatio)
                : "—"
            }
            sub={`${formatBytes(stats?.sessionDown ?? 0)} in · ${formatBytes(
              stats?.sessionUp ?? 0,
            )} out`}
          />
          <Card
            caption="Torrents"
            value={String(counts?.total ?? 0)}
            sub={`${counts?.stopped ?? 0} stopped · ${counts?.errored ?? 0} errored`}
          />
          <Card
            caption="Disk free"
            value={formatFree(diskFree)}
            sub={`${(stats?.volumes ?? []).length} volume${
              (stats?.volumes ?? []).length === 1 ? "" : "s"
            }`}
          />
        </div>

        <section className={styles.section}>
          <h2 className={styles.sectionTitle}>Throughput</h2>
          <div className={styles.legend}>
            <span className={styles.legendItem}>
              <span className={styles.dotDown} /> download
            </span>
            <span className={styles.legendItem}>
              <span className={styles.dotUp} /> upload
            </span>
            <span className={styles.peak}>
              peak {formatRate(geometry.peak)} · last 60 min
            </span>
          </div>
          <svg
            className={styles.graph}
            viewBox={`0 0 ${GRAPH_W} ${GRAPH_H}`}
            preserveAspectRatio="none"
            role="img"
            aria-label="Global transfer rates over the last 60 minutes"
          >
            {gridLines(3, GRAPH_H).map((y) => (
              <line
                key={y}
                className={styles.gridline}
                x1={0}
                x2={GRAPH_W}
                y1={y}
                y2={y}
              />
            ))}
            {geometry.downArea && (
              <path className={styles.area} d={geometry.downArea} />
            )}
            <path className={styles.lineDown} d={geometry.downLine} />
            <path className={styles.lineUp} d={geometry.upLine} />
          </svg>
        </section>

        <div className={styles.columns}>
          <section className={styles.section}>
            <h2 className={styles.sectionTitle}>Volumes</h2>
            {(stats?.volumes ?? []).length === 0 ? (
              <p className={styles.meta}>
                No volumes configured. Add a `[paths] volumes = […]` list to
                report them here.
              </p>
            ) : (
              (stats?.volumes ?? []).map((volume) => (
                <VolumeRow key={volume.path} volume={volume} />
              ))
            )}
          </section>

          <section className={styles.section}>
            <h2 className={styles.sectionTitle}>Space by label</h2>
            {space.length === 0 ? (
              <p className={styles.meta}>No torrents.</p>
            ) : (
              space.map((entry) => (
                <div className={styles.row} key={entry.label}>
                  <span className={styles.name}>{entry.label}</span>
                  <span className={styles.figure}>
                    {formatBytes(entry.bytes)} · {entry.count}
                  </span>
                  <span className={styles.bar}>
                    <span
                      className={styles.barFill}
                      style={{
                        width: `${barFraction(entry.bytes, largestLabel) * 100}%`,
                      }}
                    />
                  </span>
                </div>
              ))
            )}
          </section>
        </div>

        <p className={styles.meta}>
          Uptime {formatUptime(stats?.uptimeSeconds ?? 0)} · DHT{" "}
          {(stats?.dhtNodes ?? 0).toLocaleString()} nodes · port{" "}
          {stats?.portRange ?? "—"}
          {stats?.portStatus == null ? " (not checked)" : ""}
        </p>
      </div>
    </div>
  );
}

function Card({
  caption,
  value,
  sub,
  valueClass,
}: {
  caption: string;
  value: string;
  sub: string;
  valueClass?: string;
}) {
  return (
    <div className={styles.card}>
      <span className={styles.caption}>{caption}</span>
      <span className={`${styles.value} ${valueClass ?? ""}`}>{value}</span>
      <span className={styles.sub}>{sub}</span>
    </div>
  );
}

/** One volume: path, used-of-total and a pressure bar; unavailable when unread. */
function VolumeRow({ volume }: { volume: VolumeStat }) {
  if (volume.free === null || volume.total === null || volume.total <= 0) {
    return (
      <div className={styles.row}>
        <span className={styles.name}>{volume.path}</span>
        <span className={styles.unavailable}>unavailable</span>
      </div>
    );
  }
  const used = volume.total - volume.free;
  const pressure = used / volume.total;
  const fillClass =
    pressure >= 0.95
      ? `${styles.barFill} ${styles.barCrit}`
      : pressure >= 0.85
        ? `${styles.barFill} ${styles.barWarn}`
        : styles.barFill;
  return (
    <div className={styles.row}>
      <span className={styles.name}>{volume.path}</span>
      <span className={styles.figure}>
        {formatBytes(used)} / {formatBytes(volume.total)}
      </span>
      <span className={styles.bar}>
        <span
          className={fillClass}
          style={{ width: `${Math.min(1, pressure) * 100}%` }}
        />
      </span>
    </div>
  );
}
