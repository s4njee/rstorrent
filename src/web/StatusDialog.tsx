/**
 * Status modal (WE5-S3) — the settings-icon target. Read-only: daemon
 * version/endpoint/health and the server version, plus a Sign out button. Full
 * web preferences are v2 (there are no browser-side settings yet).
 */

import { useEffect, useState } from "react";
import { useTorrents } from "../store/torrents";

interface Health {
  server: { version: string; displayName: string };
  daemon: {
    clientVersion?: string;
    apiVersion?: string;
    sessionPath?: string;
  } | null;
}

export function StatusDialog({
  onClose,
  onSignOut,
}: {
  onClose: () => void;
  onSignOut: () => void;
}) {
  const connection = useTorrents((s) => s.connection);
  const [health, setHealth] = useState<Health | null>(null);

  useEffect(() => {
    let cancelled = false;
    fetch("/api/health")
      .then((r) => (r.ok ? r.json() : null))
      .then((h) => !cancelled && setHealth(h))
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, []);

  const rows: Array<[string, string]> = [
    ["connection", connection.phase],
    ["endpoint", connection.endpoint || "—"],
    [
      "daemon",
      connection.daemonVersion ?? health?.daemon?.clientVersion ?? "—",
    ],
    ["api", health?.daemon?.apiVersion ?? "—"],
    ["session", health?.daemon?.sessionPath ?? "—"],
    ["server", health?.server.version ?? "—"],
  ];

  return (
    <div style={S.overlay} onMouseDown={onClose}>
      <div style={S.modal} onMouseDown={(e) => e.stopPropagation()}>
        <div style={S.header}>
          <span>Status</span>
          <button style={S.x} onClick={onClose} aria-label="Close">
            ✕
          </button>
        </div>
        <div style={S.body}>
          {rows.map(([k, v]) => (
            <div key={k} style={S.row}>
              <span style={S.key}>{k}</span>
              <span style={S.val} title={v}>
                {v}
              </span>
            </div>
          ))}
        </div>
        <div style={S.footer}>
          <button style={S.signout} onClick={onSignOut}>
            Sign out
          </button>
        </div>
      </div>
    </div>
  );
}

const S = {
  overlay: {
    position: "fixed",
    inset: 0,
    background: "rgba(0,0,0,0.5)",
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    zIndex: 100,
  } as const,
  modal: {
    width: 380,
    background: "var(--bg-panel)",
    border: "1px solid var(--border-mid)",
    borderRadius: 8,
    fontFamily: "var(--font-mono)",
    color: "var(--text-body)",
  } as const,
  header: {
    display: "flex",
    alignItems: "center",
    justifyContent: "space-between",
    padding: "10px 14px",
    borderBottom: "1px solid var(--border-black)",
    fontWeight: 600,
    color: "var(--text-primary)",
  } as const,
  x: {
    border: "none",
    background: "none",
    color: "var(--text-muted)",
    cursor: "pointer",
    fontSize: 12,
  } as const,
  body: {
    padding: "12px 14px",
    display: "flex",
    flexDirection: "column",
    gap: 6,
  } as const,
  row: { display: "flex", gap: 12, fontSize: 11 } as const,
  key: {
    width: 90,
    color: "var(--text-dim)",
    textTransform: "uppercase",
    fontSize: 9.5,
    letterSpacing: ".05em",
    paddingTop: 1,
  } as const,
  val: {
    flex: 1,
    whiteSpace: "nowrap",
    overflow: "hidden",
    textOverflow: "ellipsis",
  } as const,
  footer: {
    padding: "10px 14px",
    borderTop: "1px solid var(--border-black)",
    display: "flex",
    justifyContent: "flex-end",
  } as const,
  signout: {
    padding: "5px 12px",
    border: "1px solid var(--border-strong)",
    background: "var(--bg-track)",
    color: "var(--text-body)",
    borderRadius: 5,
    fontSize: 11,
    fontFamily: "inherit",
    cursor: "pointer",
  } as const,
};
