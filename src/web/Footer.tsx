/**
 * Status footer (WE2-S5). 26px, dim: daemon version + endpoint · dht nodes ·
 * spacer · torrent count · download/upload totals. All from the live snapshot.
 */

import { useTorrents } from "../store/torrents";
import { formatRate } from "../utils/format";

export function Footer() {
  const globals = useTorrents((s) => s.globals);
  const connection = useTorrents((s) => s.connection);
  const count = useTorrents((s) => s.torrents.length);

  const version = connection.daemonVersion ?? "—";

  return (
    <footer style={S.bar}>
      <span>
        rtorrent {version}
        {connection.endpoint ? ` · ${connection.endpoint}` : ""}
      </span>
      <span>dht: {globals.dhtNodes} nodes</span>
      <span style={{ flex: 1 }} />
      <span>{count} torrents</span>
      <span style={{ color: "var(--accent-cyan-bright)" }}>
        ↓ {formatRate(globals.downRate)}
      </span>
      <span style={{ color: "var(--accent-green-soft)" }}>
        ↑ {formatRate(globals.upRate)}
      </span>
    </footer>
  );
}

const S = {
  bar: {
    display: "flex",
    alignItems: "center",
    gap: 18,
    height: "var(--footer-height, 26px)",
    padding: "0 16px",
    flex: "none",
    boxSizing: "border-box",
    background: "var(--bg-panel)",
    borderTop: "1px solid var(--border-black)",
    fontSize: 10.5,
    color: "var(--text-dim)",
  } as const,
};
