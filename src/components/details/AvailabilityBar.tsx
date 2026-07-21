/**
 * The availability bar: how well the swarm covers each piece, drawn directly
 * below the pieces bar. Where the pieces bar shows what *we* have, this shows
 * how many connected peers have each piece — so a dim or bare column flags a
 * rare stretch that could stall the download if those peers leave.
 *
 * Like the pieces bar it renders on a canvas (a torrent can have far more chunks
 * than the bar has pixels): each pixel column averages the peer counts of its
 * slice (see `bucketAverages`) and is painted at an alpha scaled to that average
 * over the peak, so more-available stretches read brighter and 0-peer chunks
 * fall back to bare track.
 */

import { useEffect, useRef } from "react";
import type { PieceInfo } from "../../ipc/types";
import {
  availabilityToBytes,
  bucketAverages,
  distributedCopies,
} from "../../utils/bitfield";
import styles from "./AvailabilityBar.module.css";

const BAR_HEIGHT = 10;

/** Resolve a CSS custom property to a concrete color for canvas use. */
function cssVar(name: string, fallback: string): string {
  const v = getComputedStyle(document.documentElement)
    .getPropertyValue(name)
    .trim();
  return v || fallback;
}

export function AvailabilityBar({ pieces }: { pieces: PieceInfo }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  const counts = availabilityToBytes(pieces.availability ?? "");
  const { sizeChunks } = pieces;

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
      const seen = cssVar("--avail", "#9b8cf0");

      // Track underneath; availability columns painted over it.
      ctx.globalAlpha = 1;
      ctx.fillStyle = track;
      ctx.fillRect(0, 0, cssWidth, BAR_HEIGHT);

      const { avg, max } = bucketAverages(counts, sizeChunks, cssWidth);
      if (max > 0) {
        ctx.fillStyle = seen;
        for (let x = 0; x < avg.length; x++) {
          const a = avg[x];
          if (a <= 0) continue; // no peer has this stretch: leave bare track
          // Floor the alpha so a single-peer column is still legible, but keep
          // it well below the peak so relative scarcity stays readable.
          ctx.globalAlpha = 0.28 + 0.72 * (a / max);
          ctx.fillRect(x, 0, 1, BAR_HEIGHT);
        }
      }
      ctx.globalAlpha = 1;
    };

    draw();
    // Redraw when the panel resizes (the bar is full-width).
    const observer = new ResizeObserver(draw);
    observer.observe(canvas);
    return () => observer.disconnect();
  }, [counts, sizeChunks]);

  // Nothing to show without decoded counts (the caller usually gates on
  // `availability` too, but stay defensive against an empty/garbled buffer).
  const n = Math.min(sizeChunks, counts.length);
  if (n <= 0) return null;

  let min = Infinity;
  let max = 0;
  for (let i = 0; i < n; i++) {
    const c = counts[i] ?? 0;
    if (c < min) min = c;
    if (c > max) max = c;
  }
  if (!Number.isFinite(min)) min = 0;
  const copies = distributedCopies(counts, sizeChunks);

  return (
    <div className={styles.wrap}>
      <canvas
        ref={canvasRef}
        className={styles.canvas}
        style={{ height: BAR_HEIGHT }}
        // A screen reader can't read a canvas; describe it instead.
        role="img"
        aria-label={`Piece availability: ${min} to ${max} peers per piece, ${copies.toFixed(2)} distributed copies`}
      />
      <div className={styles.caption}>
        <span>
          <b>availability:</b> {min}–{max} peers/piece
        </span>
        <span className={styles.copies}>{copies.toFixed(2)} copies</span>
      </div>
    </div>
  );
}
