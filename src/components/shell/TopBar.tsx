/**
 * The console's top bar (design frame 1a): the wordmark, the live global rates
 * with a 60-second sparkline, the filter field, the primary add action, and the
 * settings entry.
 *
 * Shared by both shells. The desktop sets `trafficLights`, which reserves the
 * macOS gutter and marks the bar as the window's drag region; a browser ignores
 * both, so neither shell needs its own bar.
 */

import { useId, type ReactNode } from "react";
import { useTorrents } from "../../store/torrents";
import { useUi } from "../../store/ui";
import { useTransferHistory } from "../../store/transferHistory";
import { accel } from "../../platform";
import { formatRate } from "../../utils/format";
import { sparkPeak, sparkPoints } from "../../utils/sparkline";
import { AddIcon, GearIcon, SearchIcon, SessionIcon, StatsIcon } from "../icons";
import styles from "./TopBar.module.css";

/** Samples the sparkline draws; 60s of the ~1s cadence. */
const SPARK_POINTS = 60;
/** Sparkline box in CSS pixels; the SVG viewBox is half-scale for crispness. */
const SPARK_W = 170;
const SPARK_H = 26;

interface TopBarProps {
  /** Desktop only: reserve the traffic-light gutter and drag the window. */
  trafficLights?: boolean;
  /** Web only: the account chip (sign out). */
  account?: ReactNode;
  /**
   * When set, the settings entry opens this instead of the desktop Preferences
   * dialog — the web shell routes to `/settings` (WC7-S1).
   */
  onSettings?: () => void;
  /** Web only: a route to the Stats page (WC8). */
  onStats?: () => void;
}

export function TopBar({
  trafficLights = false,
  account,
  onSettings,
  onStats,
}: TopBarProps) {
  const globals = useTorrents((s) => s.globals);
  const points = useTransferHistory((s) => s.points);
  const search = useUi((s) => s.search);
  const setSearch = useUi((s) => s.setSearch);
  const openDialog = useUi((s) => s.openDialog);
  const titleId = useId();

  const recent = points.slice(-SPARK_POINTS);
  const peak = sparkPeak(recent);
  const downPoints = sparkPoints(
    recent.map((point) => point.down),
    peak,
    SPARK_H,
    SPARK_W,
  );
  const upPoints = sparkPoints(
    recent.map((point) => point.up),
    peak,
    SPARK_H,
    SPARK_W,
  );

  return (
    <header
      className={styles.bar}
      data-desktop={trafficLights ? "" : undefined}
      {...(trafficLights ? { "data-tauri-drag-region": true } : {})}
    >
      <img
        className={styles.logo}
        src="/blackbird.jpg"
        alt=""
        aria-hidden="true"
      />
      <span className={styles.wordmark} id={titleId}>
        Blackbird
      </span>

      <span className={styles.divider} />

      <span className={styles.rates} title="Global transfer rates">
        <span className={styles.rateDown}>
          <span className={styles.rateGlyph} aria-hidden="true">
            ▼
          </span>
          {formatRate(globals.downRate)}
        </span>
        <span className={styles.rateUp}>
          <span className={styles.rateGlyph} aria-hidden="true">
            ▲
          </span>
          {formatRate(globals.upRate)}
        </span>
      </span>

      <span className={styles.spark} title="Last 60 seconds">
        <svg
          width="100%"
          height="100%"
          viewBox={`0 0 ${SPARK_W} ${SPARK_H}`}
          preserveAspectRatio="none"
          aria-hidden="true"
        >
          <polyline className={styles.sparkDown} points={downPoints} />
          <polyline className={styles.sparkUp} points={upPoints} />
        </svg>
      </span>

      <span className={styles.grow} />

      <label className={styles.filter} htmlFor="filter-input">
        <SearchIcon size={11} />
        <input
          id="filter-input"
          value={search}
          onChange={(event) => setSearch(event.currentTarget.value)}
          placeholder="Filter torrents…"
          spellCheck={false}
          aria-label="Filter torrents"
        />
      </label>

      <button
        type="button"
        className={styles.primary}
        onClick={() => openDialog("add-file")}
        title={`Add torrent (${accel("O")})`}
      >
        <AddIcon size={11} />
        Add torrent
      </button>

      {onStats && (
        <button
          type="button"
          className={styles.iconButton}
          onClick={onStats}
          title="Statistics"
          aria-label="Statistics"
        >
          <StatsIcon size={12} />
        </button>
      )}

      <button
        type="button"
        className={styles.iconButton}
        onClick={() => openDialog("session")}
        title="Session export / import"
        aria-label="Session export and import"
      >
        <SessionIcon size={12} />
      </button>

      <button
        type="button"
        className={styles.iconButton}
        onClick={() => (onSettings ? onSettings() : openDialog("prefs"))}
        title={`Settings (${accel(",")})`}
        aria-label="Settings"
      >
        <GearIcon size={12} />
      </button>

      {account}
    </header>
  );
}
