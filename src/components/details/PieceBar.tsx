/**
 * The pieces bar: a map of which pieces of the torrent are on disk.
 *
 * Rendered on a canvas rather than as DOM/SVG rects because a torrent can have
 * hundreds of thousands of pieces — far more than the bar has pixels. Each pixel
 * column summarizes its slice of the bitfield (see `bucketFractions`) and is
 * drawn with alpha = completed fraction over the track color, so a partially
 * finished slice reads as a proportionally dimmer column instead of being
 * rounded away.
 *
 * Completed pieces take the torrent's status color (cyan downloading, green
 * seeding, …), matching the table's progress bar.
 */

import { useEffect, useRef } from "react";
import type { PieceInfo, Status } from "../../ipc/types";
import { bitfieldToBytes, bucketFractions } from "../../utils/bitfield";
import { formatBytes } from "../../utils/format";
import styles from "./PieceBar.module.css";

const BAR_HEIGHT = 14;

/** Status → the CSS custom property used for completed pieces. */
const STATUS_VAR: Record<Status, string> = {
  downloading: "--status-downloading",
  seeding: "--status-seeding",
  completed: "--status-seeding",
  paused: "--status-paused",
  stalled: "--status-stalled",
  checking: "--status-checking",
  error: "--status-error",
};

/** Resolve a CSS custom property to a concrete color for canvas use. */
function cssVar(name: string, fallback: string): string {
  const v = getComputedStyle(document.documentElement)
    .getPropertyValue(name)
    .trim();
  return v || fallback;
}

export function PieceBar({
  pieces,
  status,
}: {
  pieces: PieceInfo;
  status: Status;
}) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    const draw = () => {
      const cssWidth = canvas.clientWidth;
      if (cssWidth <= 0) return;
      // Render at device resolution so columns stay crisp on Retina.
      const dpr = window.devicePixelRatio || 1;
      canvas.width = Math.round(cssWidth * dpr);
      canvas.height = Math.round(BAR_HEIGHT * dpr);
      const ctx = canvas.getContext("2d");
      if (!ctx) return;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

      const track = cssVar("--bg-track", "#22262d");
      const done = cssVar(STATUS_VAR[status], "#58c4dd");

      // Track underneath; completed columns painted over it.
      ctx.globalAlpha = 1;
      ctx.fillStyle = track;
      ctx.fillRect(0, 0, cssWidth, BAR_HEIGHT);

      const bytes = bitfieldToBytes(pieces.bitfield);
      const fractions = bucketFractions(bytes, pieces.sizeChunks, cssWidth);
      ctx.fillStyle = done;
      for (let x = 0; x < fractions.length; x++) {
        const f = fractions[x];
        if (f <= 0) continue;
        ctx.globalAlpha = f;
        ctx.fillRect(x, 0, 1, BAR_HEIGHT);
      }
      ctx.globalAlpha = 1;
    };

    draw();
    // Redraw when the panel resizes (the bar is full-width).
    const observer = new ResizeObserver(draw);
    observer.observe(canvas);
    return () => observer.disconnect();
  }, [pieces, status]);

  const { completedChunks, sizeChunks, chunkSize } = pieces;
  const pct = sizeChunks > 0 ? (completedChunks / sizeChunks) * 100 : 0;

  return (
    <div className={styles.wrap}>
      <canvas
        ref={canvasRef}
        className={styles.canvas}
        style={{ height: BAR_HEIGHT }}
        // A screen reader can't read a canvas; describe it instead.
        role="img"
        aria-label={`${completedChunks} of ${sizeChunks} pieces downloaded, ${pct.toFixed(0)} percent`}
      />
      <div className={styles.caption}>
        <span>
          <b>pieces:</b> {completedChunks.toLocaleString()} /{" "}
          {sizeChunks.toLocaleString()}
        </span>
        {chunkSize > 0 && (
          <span>
            <b>piece size:</b> {formatBytes(chunkSize)}
          </span>
        )}
        <span className={styles.pct}>{pct.toFixed(1)}%</span>
      </div>
    </div>
  );
}
