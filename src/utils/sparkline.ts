/**
 * Sparkline geometry for the top bar's rate graph.
 *
 * Pure point maths, separate from the SVG that consumes it, so the scaling and
 * baseline behaviour can be tested without a DOM.
 */

/**
 * A polyline's `points` string for one series, scaled into a box.
 *
 * Both series share one `max` so the download and upload lines are comparable —
 * scaling each to its own peak would draw a trickle the same height as a flood.
 * A zero rate sits on the baseline (SVG y grows downward).
 */
export function sparkPoints(
  values: number[],
  max: number,
  height: number,
  width: number,
): string {
  if (values.length === 0) return "";
  const step = values.length > 1 ? width / (values.length - 1) : width;
  return values
    .map((value, index) => {
      const x = index * step;
      const y = height - (max > 0 ? (value / max) * height : 0);
      return `${x.toFixed(1)},${y.toFixed(1)}`;
    })
    .join(" ");
}

/**
 * The peak of both series, floored at 1 so an idle app draws a flat line on the
 * baseline rather than dividing by zero.
 */
export function sparkPeak(points: Array<{ down: number; up: number }>): number {
  let peak = 1;
  for (const point of points) {
    peak = Math.max(peak, point.down, point.up);
  }
  return peak;
}
