/**
 * Speed tab: a small SVG area chart of the selected torrent's down/up rates
 * over the last few minutes, drawn from the frontend rate-history ring buffer
 * (no rtorrent call). Download is cyan, upload is green; the y-axis auto-scales
 * to the peak in view. Renders an empty hint until a couple of samples exist.
 */

import { useRateHistory } from "../../store/rateHistory";
import { formatRate } from "../../utils/format";

const W = 520;
const H = 90;

export function SpeedChart({ hash }: { hash: string }) {
  // Subscribe to the series map so the chart re-renders as samples arrive.
  const series = useRateHistory((s) => s.series);
  const points = series.get(hash) ?? [];

  if (points.length < 2) {
    return (
      <div style={{ color: "var(--text-dim)", fontSize: "10.5px" }}>
        collecting speed data…
      </div>
    );
  }

  const peak = Math.max(1, ...points.map((p) => Math.max(p.down, p.up)));
  const n = points.length;
  const x = (i: number) => (i / (n - 1)) * W;
  const y = (v: number) => H - (v / peak) * H;

  // Build an area path (line down to the baseline and back) for a series.
  const area = (key: "down" | "up") => {
    const line = points
      .map(
        (p, i) =>
          `${i === 0 ? "M" : "L"}${x(i).toFixed(1)},${y(p[key]).toFixed(1)}`,
      )
      .join(" ");
    return `${line} L${W},${H} L0,${H} Z`;
  };

  const last = points[n - 1];

  return (
    <div>
      <svg
        width="100%"
        viewBox={`0 0 ${W} ${H}`}
        preserveAspectRatio="none"
        style={{ display: "block" }}
      >
        {/* Dim gridlines at 25/50/75%. */}
        {[0.25, 0.5, 0.75].map((f) => (
          <line
            key={f}
            x1={0}
            x2={W}
            y1={H * f}
            y2={H * f}
            stroke="var(--border-mid)"
            strokeWidth={1}
          />
        ))}
        <path
          d={area("down")}
          fill="var(--accent-cyan)"
          fillOpacity={0.18}
          stroke="var(--accent-cyan)"
          strokeWidth={1}
        />
        <path
          d={area("up")}
          fill="var(--accent-green)"
          fillOpacity={0.14}
          stroke="var(--accent-green-soft)"
          strokeWidth={1}
        />
      </svg>
      <div
        style={{ display: "flex", gap: 16, marginTop: 6, fontSize: "10.5px" }}
      >
        <span style={{ color: "var(--accent-cyan-bright)" }}>
          ↓ {formatRate(last.down)}
        </span>
        <span style={{ color: "var(--accent-green-soft)" }}>
          ↑ {formatRate(last.up)}
        </span>
        <span style={{ color: "var(--text-dim)" }}>
          peak {formatRate(peak)}
        </span>
      </div>
    </div>
  );
}
