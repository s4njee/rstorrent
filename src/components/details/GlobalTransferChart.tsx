/** Global download/upload history chart (C17). */

import { useTransferHistory } from "../../store/transferHistory";
import { formatRate } from "../../utils/format";

const W = 520;
const H = 150;

export function GlobalTransferChart() {
  const points = useTransferHistory((s) => s.points);

  if (points.length < 2) {
    return (
      <div style={{ color: "var(--text-dim)", fontSize: "var(--fs-cell)" }}>
        collecting global transfer data…
      </div>
    );
  }

  const peak = Math.max(1, ...points.map((p) => Math.max(p.down, p.up)));
  const last = points[points.length - 1];
  const x = (i: number) => (i / (points.length - 1)) * W;
  const y = (rate: number) => H - (rate / peak) * H;

  const area = (key: "down" | "up") => {
    const line = points
      .map(
        (point, i) =>
          `${i === 0 ? "M" : "L"}${x(i).toFixed(1)},${y(point[key]).toFixed(1)}`,
      )
      .join(" ");
    return `${line} L${W},${H} L0,${H} Z`;
  };

  const elapsed = Math.max(0, last.time - points[0].time);
  const minutes = Math.max(1, Math.round(elapsed / 60_000));

  return (
    <div>
      <svg
        width="100%"
        viewBox={`0 0 ${W} ${H}`}
        preserveAspectRatio="none"
        role="img"
        aria-label={`Global transfer rates for the last ${minutes} minutes`}
        style={{ display: "block" }}
      >
        {[0.25, 0.5, 0.75].map((fraction) => (
          <line
            key={fraction}
            x1={0}
            x2={W}
            y1={H * fraction}
            y2={H * fraction}
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
        style={{
          display: "flex",
          gap: 16,
          marginTop: 6,
          fontSize: "var(--fs-cell)",
          flexWrap: "wrap",
        }}
      >
        <span style={{ color: "var(--accent-cyan-bright)" }}>
          ↓ {formatRate(last.down)}
        </span>
        <span style={{ color: "var(--accent-green-soft)" }}>
          ↑ {formatRate(last.up)}
        </span>
        <span style={{ color: "var(--text-dim)" }}>
          peak {formatRate(peak)} · {minutes}m · {points.length} samples
        </span>
      </div>
    </div>
  );
}
