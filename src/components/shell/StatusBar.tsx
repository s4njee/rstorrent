/**
 * The status bar (design frame 1a): where the app's connection and the daemon's
 * aggregate state are stated.
 *
 * Sits inside the main column, under the detail panel, as the design draws it —
 * the sidebar runs the full height beside it.
 *
 * Only values we actually have are rendered. Three of the design's items have no
 * source yet and are simply absent rather than faked:
 *   · the port's open/closed verdict needs a reachability probe (deferred);
 *   · session ratio needs the daemon's session totals, which the web host does
 *     not expose until WC8-S2;
 *   · uptime is measured here, from the first snapshot this app received — the
 *     daemon does not report when it started.
 */

import { useEffect, useState } from "react";
import { useTorrents } from "../../store/torrents";
import { useUi } from "../../store/ui";
import {
  activeMoves,
  baseName,
  movePercent,
  retryableMoves,
  useMoves,
} from "../../store/moves";
import { cancelMove, getMoves } from "../../ipc/commands";
import { formatFree, formatRate, formatUptime } from "../../utils/format";
import { statusBarCounts } from "../../store/selectors";
import styles from "./StatusBar.module.css";

export function StatusBar() {
  const connection = useTorrents((s) => s.connection);
  const globals = useTorrents((s) => s.globals);
  const torrents = useTorrents((s) => s.torrents);
  const openDialog = useUi((s) => s.openDialog);
  const [uptime, setUptime] = useState<number | null>(null);

  const connected = connection.phase === "connected";

  // Uptime is this app's session against the daemon: counted from the first
  // snapshot, so a reconnect keeps counting rather than resetting.
  useEffect(() => {
    if (!connected || uptime !== null) return;
    const started = Date.now();
    setUptime(0);
    const timer = setInterval(() => {
      setUptime(Math.floor((Date.now() - started) / 1000));
    }, 1000);
    return () => clearInterval(timer);
  }, [connected, uptime]);

  const counts = statusBarCounts(torrents);
  const free = formatFree(globals.freeSpace);
  const version = connection.daemonVersion;
  const moves = useMoves((s) => s.moves);
  const live = activeMoves(moves);
  const failed = retryableMoves(moves);
  const first = live[0] ?? null;
  const firstPct = first ? movePercent(first) : null;
  const hasLive = live.length > 0;

  // While a move is active, refetch statuses for smooth byte progress (the
  // push/event channel only guarantees transitions, not every chunk).
  useEffect(() => {
    if (!hasLive) return;
    const timer = setInterval(() => {
      void getMoves().then(
        (m) => useMoves.getState().set(m),
        () => {},
      );
    }, 1000);
    return () => clearInterval(timer);
  }, [hasLive]);

  return (
    <footer className={styles.bar}>
      <span
        className={styles.dot}
        data-connected={connected ? "" : undefined}
        aria-hidden="true"
      />
      <span className={styles.segment}>
        {version
          ? `rtorrent ${version}`
          : connected
            ? "rtorrent"
            : "disconnected"}
      </span>

      <span className={styles.segment}>
        {torrents.length} torrents · {counts.seeding} seeding ·{" "}
        {counts.downloading} downloading
      </span>

      {counts.errored > 0 && (
        <span className={styles.error}>{counts.errored} errored</span>
      )}

      {first && (
        <span
          className={`${styles.segment} ${styles.clickable}`}
          onClick={() => openDialog("moves")}
          title={`Moving ${first.src} → ${first.dst}. Open moves.`}
        >
          Moving {baseName(first.name)}
          {firstPct !== null ? ` · ${firstPct}%` : ""}
          {live.length > 1 ? ` (+${live.length - 1})` : ""}
          <button
            type="button"
            className={styles.inline}
            title="Cancel move (the torrent resumes in place)"
            aria-label={`Cancel move of ${first.name}`}
            onClick={(e) => {
              e.stopPropagation();
              void cancelMove(first.id).catch(() => {});
            }}
          >
            ×
          </button>
        </span>
      )}
      {!first && failed.length > 0 && (
        <span
          className={`${styles.segment} ${styles.clickable} ${styles.error}`}
          onClick={() => openDialog("moves")}
          title="A move needs attention. Open moves."
        >
          Move failed
        </span>
      )}

      {globals.dhtNodes > 0 && (
        <span
          className={`${styles.segment} ${styles.clickable}`}
          onClick={() => openDialog("stats")}
          title="Statistics"
        >
          DHT {globals.dhtNodes.toLocaleString()} nodes
        </span>
      )}

      <span className={styles.grow} />

      {free && <span className={styles.segment}>{free}</span>}
      <span className={styles.down}>↓ {formatRate(globals.downRate)}</span>
      <span className={styles.up}>↑ {formatRate(globals.upRate)}</span>
      {connected && uptime !== null && (
        <span className={styles.segment}>Uptime {formatUptime(uptime)}</span>
      )}
    </footer>
  );
}
