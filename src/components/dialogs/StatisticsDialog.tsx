/**
 * Statistics dialog (design screen 05) with a Daemon health tab (D16).
 *
 * The Statistics tab shows two groups of `key … value` rows — User and Cache —
 * loaded via `get_statistics`. The Daemon tab (D16) surfaces what the daemon
 * reports about itself (version, session path, cache/socket/file limits) via
 * `daemon_health`. Values the backend can't provide come back null/0 and render
 * as "—".
 */

import { useEffect, useState } from "react";
import { useUi } from "../../store/ui";
import { getStatistics, daemonHealth } from "../../ipc/commands";
import type { DaemonHealth, Statistics } from "../../ipc/types";
import { formatBytes } from "../../utils/format";
import { ModalBase, Button } from "./ModalBase";
import forms from "./forms.module.css";

type Tab = "stats" | "daemon";

/** Format a nullable number, or an em-dash when unavailable. */
const dash = (v: number | null, fmt: (n: number) => string) =>
  v == null ? "—" : fmt(v);

export function StatisticsDialog() {
  const closeDialog = useUi((s) => s.closeDialog);
  const [tab, setTab] = useState<Tab>("stats");
  const [stats, setStats] = useState<Statistics | null>(null);
  const [health, setHealth] = useState<DaemonHealth | null>(null);

  useEffect(() => {
    void getStatistics().then(setStats);
    void daemonHealth().then(setHealth);
  }, []);

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
      <div className={forms.col}>
        <div style={{ display: "flex", gap: 6 }}>
          <TabButton active={tab === "stats"} onClick={() => setTab("stats")}>
            Statistics
          </TabButton>
          <TabButton active={tab === "daemon"} onClick={() => setTab("daemon")}>
            Daemon
          </TabButton>
        </div>

        {tab === "stats" ? (
          <StatsTab stats={stats} />
        ) : (
          <DaemonTab health={health} />
        )}
      </div>
    </ModalBase>
  );
}

function StatsTab({ stats }: { stats: Statistics | null }) {
  if (!stats) return <div className={forms.meta}>loading…</div>;

  const userRows: Array<[string, string, string?]> = [
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
  ];

  const cacheRows: Array<[string, string]> = [
    ["Read cache hits", dash(stats.cacheHitPct, (p) => `${p.toFixed(1)}%`)],
    ["Total buffer size", dash(stats.bufferSize, formatBytes)],
    [
      "Write cache overload",
      dash(stats.cacheOverloadPct, (p) => `${p.toFixed(1)}%`),
    ],
    ["Queued I/O jobs", dash(stats.queuedIo, (n) => String(n))],
  ];

  return (
    <>
      <Section title="User Statistics">
        {userRows.map(([k, v, color]) => (
          <Row key={k} k={k} v={v} color={color} />
        ))}
      </Section>
      <Section title="Cache Statistics">
        {cacheRows.map(([k, v]) => (
          <Row key={k} k={k} v={v} />
        ))}
      </Section>
    </>
  );
}

function DaemonTab({ health }: { health: DaemonHealth | null }) {
  if (!health) return <div className={forms.meta}>loading…</div>;

  const num = (n: number) => (n > 0 ? String(n) : "—");
  const rows: Array<[string, string]> = [
    ["rtorrent version", health.clientVersion || "—"],
    ["XML-RPC API", health.apiVersion || "—"],
    ["Session path", health.sessionPath || "—"],
    [
      "Piece cache",
      health.memoryMax > 0
        ? `${formatBytes(health.memoryCurrent)} / ${formatBytes(health.memoryMax)}`
        : "—",
    ],
    ["Open sockets", num(health.openSockets)],
    ["Max open sockets", num(health.maxOpenSockets)],
    ["Max open files", num(health.maxOpenFiles)],
    ["HTTP max open", num(health.httpMaxOpen)],
  ];

  return (
    <Section title="Daemon">
      {rows.map(([k, v]) => (
        <Row key={k} k={k} v={v} />
      ))}
    </Section>
  );
}

function TabButton({
  active,
  onClick,
  children,
}: {
  active: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      onClick={onClick}
      style={{
        background: "none",
        border: "none",
        borderBottom: active
          ? "2px solid var(--accent-cyan, #7daea3)"
          : "2px solid transparent",
        color: active ? "var(--text-primary)" : "var(--text-muted)",
        padding: "2px 2px 4px",
        cursor: "pointer",
        fontSize: 12,
      }}
    >
      {children}
    </button>
  );
}

function Section({
  title,
  children,
}: {
  title: string;
  children: React.ReactNode;
}) {
  return (
    <div>
      <div className={forms.section} style={{ marginBottom: 8 }}>
        {title}
      </div>
      {children}
    </div>
  );
}

/** A single `key … value` row with a bottom divider. */
function Row({ k, v, color }: { k: string; v: string; color?: string }) {
  return (
    <div
      style={{
        display: "flex",
        justifyContent: "space-between",
        gap: 12,
        padding: "3px 0",
        borderBottom: "1px solid var(--border-row)",
      }}
    >
      <span style={{ color: "var(--text-muted)", whiteSpace: "nowrap" }}>
        {k}
      </span>
      <span
        style={{
          color: color ?? "var(--text-primary)",
          textAlign: "right",
          wordBreak: "break-all",
        }}
      >
        {v}
      </span>
    </div>
  );
}
