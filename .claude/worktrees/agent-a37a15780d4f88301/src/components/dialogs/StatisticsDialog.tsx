/**
 * Statistics dialog (design screen 05).
 *
 * Two groups of `key … value` rows — User and Cache statistics — loaded once via
 * the `get_statistics` command. Values that the backend can't provide come back
 * null and render as "—" (see plan.md §10 / E12). The all-time ratio is shown in
 * green when healthy (≥ 1).
 */

import { useEffect, useState } from "react";
import { useUi } from "../../store/ui";
import { getStatistics } from "../../ipc/commands";
import type { Statistics } from "../../ipc/types";
import { formatBytes } from "../../utils/format";
import { ModalBase, Button } from "./ModalBase";
import forms from "./forms.module.css";

/** Format a nullable number, or an em-dash when unavailable. */
const dash = (v: number | null, fmt: (n: number) => string) =>
  v == null ? "—" : fmt(v);

export function StatisticsDialog() {
  const closeDialog = useUi((s) => s.closeDialog);
  const [stats, setStats] = useState<Statistics | null>(null);

  useEffect(() => {
    void getStatistics().then(setStats);
  }, []);

  const userRows: Array<[string, string, string?]> = stats
    ? [
        ["Session download", formatBytes(stats.sessionDown)],
        ["Session upload", formatBytes(stats.sessionUp)],
        ["All-time download", formatBytes(stats.allTimeDown)],
        ["All-time upload", formatBytes(stats.allTimeUp)],
        [
          "All-time share ratio",
          dash(stats.allTimeRatio, (r) => r.toFixed(2)),
          stats.allTimeRatio != null && stats.allTimeRatio >= 1
            ? "var(--accent-green-soft)"
            : undefined,
        ],
        ["Session waste", formatBytes(stats.sessionWaste)],
        ["Connected peers", String(stats.connectedPeers)],
      ]
    : [];

  const cacheRows: Array<[string, string]> = stats
    ? [
        ["Read cache hits", dash(stats.cacheHitPct, (p) => `${p.toFixed(1)}%`)],
        ["Total buffer size", dash(stats.bufferSize, formatBytes)],
        [
          "Write cache overload",
          dash(stats.cacheOverloadPct, (p) => `${p.toFixed(1)}%`),
        ],
        ["Queued I/O jobs", dash(stats.queuedIo, (n) => String(n))],
      ]
    : [];

  return (
    <ModalBase
      title="Statistics"
      width={400}
      onCancel={closeDialog}
      onPrimary={closeDialog}
      footer={
        <Button variant="primary" onClick={closeDialog}>
          Close
        </Button>
      }
    >
      {!stats ? (
        <div className={forms.meta}>loading…</div>
      ) : (
        <div className={forms.col}>
          <div>
            <div className={forms.section} style={{ marginBottom: 8 }}>
              User Statistics
            </div>
            {userRows.map(([k, v, color]) => (
              <Row key={k} k={k} v={v} color={color} />
            ))}
          </div>
          <div>
            <div className={forms.section} style={{ marginBottom: 8 }}>
              Cache Statistics
            </div>
            {cacheRows.map(([k, v]) => (
              <Row key={k} k={k} v={v} />
            ))}
          </div>
        </div>
      )}
    </ModalBase>
  );
}

/** A single `key … value` row with a bottom divider. */
function Row({ k, v, color }: { k: string; v: string; color?: string }) {
  return (
    <div
      style={{
        display: "flex",
        justifyContent: "space-between",
        padding: "3px 0",
        borderBottom: "1px solid var(--border-row)",
      }}
    >
      <span style={{ color: "var(--text-muted)" }}>{k}</span>
      <span style={{ color: color ?? "var(--text-primary)" }}>{v}</span>
    </div>
  );
}
