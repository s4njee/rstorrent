/**
 * Throughput-graph geometry (WC8-S4).
 *
 * Pure maths for the Stats route's SVG: the two series share one peak (a
 * trickle must not draw as tall as a flood), scaled to the 60-minute window the
 * server keeps. No axes; the legend and peak readout come from the same peak.
 */

export interface RatePoint {
  down: number;
  up: number;
}

export interface ThroughputGeometry {
  downLine: string;
  upLine: string;
  /** The download series closed to the baseline, for the filled area. */
  downArea: string;
  /** The largest value in the window; 0 when there are no samples. */
  peak: number;
}

/**
 * Build the two polylines and the download area for `points` in a
 * `width × height` box. One or zero samples draw a flat baseline rather than an
 * empty or crashing path.
 */
export function throughputGeometry(
  points: readonly RatePoint[],
  width: number,
  height: number,
): ThroughputGeometry {
  if (points.length === 0) {
    const flat = `M0,${height} L${width},${height}`;
    return { downLine: flat, upLine: flat, downArea: "", peak: 0 };
  }

  let peak = 0;
  for (const point of points) {
    peak = Math.max(peak, point.down, point.up);
  }
  // A flat-at-zero window still needs a non-zero divisor.
  const scale = peak > 0 ? peak : 1;
  const lastIndex = Math.max(1, points.length - 1);
  const x = (index: number) => (index / lastIndex) * width;
  const y = (value: number) => height - (Math.max(0, value) / scale) * height;

  const down = points.map((point, i) => `${x(i)},${y(point.down)}`);
  const up = points.map((point, i) => `${x(i)},${y(point.up)}`);
  const downArea = `M0,${height} L${down.join(" L")} L${width},${height} Z`;

  return {
    downLine: `M${down.join(" L")}`,
    upLine: `M${up.join(" L")}`,
    downArea,
    peak,
  };
}

/** Positions of `count` evenly spaced horizontal gridlines within `height`. */
export function gridLines(count: number, height: number): number[] {
  if (count <= 0) return [];
  return Array.from(
    { length: count },
    (_, i) => ((i + 1) / (count + 1)) * height,
  );
}
