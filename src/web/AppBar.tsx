/**
 * App bar (WE2-S2) — the web shell's top chrome, replacing the desktop title
 * bar + toolbar.
 *
 * Left→right: logo mark + `rtorrent / web` wordmark · Add (primary) + Magnet
 * (secondary) · spacer · search (240px, `/` focuses) · live ↓/↑ speeds ·
 * connection dot · settings (opens the Status modal — WE5) · avatar (sign-out
 * menu — WE5). Mutating affordances disable while disconnected.
 */

import { useEffect, useRef } from "react";
import { useTorrents } from "../store/torrents";
import { useUi } from "../store/ui";
import { formatRate } from "../utils/format";
import { AddIcon, MagnetIcon } from "../components/icons";

export function AppBar({
  displayName,
  onOpenStatus,
}: {
  displayName: string;
  onOpenStatus: () => void;
}) {
  const globals = useTorrents((s) => s.globals);
  const connection = useTorrents((s) => s.connection);
  const search = useUi((s) => s.search);
  const setSearch = useUi((s) => s.setSearch);
  const openDialog = useUi((s) => s.openDialog);
  const connected = connection.phase === "connected";

  const searchRef = useRef<HTMLInputElement>(null);

  // `/` focuses the search box (unless typing in another field).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "/") return;
      const el = document.activeElement;
      const typing =
        el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement;
      if (!typing) {
        e.preventDefault();
        searchRef.current?.focus();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <header style={S.bar}>
      <div style={S.brand}>
        <span style={S.logo}>r</span>
        <span style={S.wordmark}>
          rtorrent
          <span style={{ color: "var(--text-dim)", fontWeight: 400 }}>
            {" "}
            / web
          </span>
        </span>
      </div>

      <span style={S.sep} />

      <div style={{ display: "flex", gap: 4 }}>
        <button
          style={S.addBtn}
          disabled={!connected}
          onClick={() => openDialog("add-file")}
          title="Add torrent"
        >
          <AddIcon size={11} />
          Add
        </button>
        <button
          style={S.magnetBtn}
          disabled={!connected}
          onClick={() => openDialog("add-magnet")}
          title="Add magnet"
        >
          <MagnetIcon size={11} />
          Magnet
        </button>
      </div>

      <span style={{ flex: 1 }} />

      <input
        ref={searchRef}
        style={S.search}
        placeholder="/ search torrents"
        value={search}
        onChange={(e) => setSearch(e.currentTarget.value)}
      />

      <div style={S.speeds}>
        <span style={{ color: "var(--accent-cyan-bright)" }}>
          ↓ {formatRate(globals.downRate)}
        </span>
        <span style={{ color: "var(--accent-green-soft)" }}>
          ↑ {formatRate(globals.upRate)}
        </span>
      </div>

      <span style={S.sep} />

      <div style={S.right}>
        <span style={S.connWrap}>
          <span style={S.dot(connected)} />
          <span
            style={{
              color: connected
                ? "var(--accent-green-soft)"
                : "var(--accent-red)",
              fontSize: 10.5,
            }}
          >
            {connected ? "connected" : "disconnected"}
          </span>
        </span>
        <button
          style={S.iconBtn}
          title="Status"
          aria-label="Status"
          onClick={onOpenStatus}
        >
          <SettingsGlyph />
        </button>
        <span
          style={{ ...S.avatar, cursor: "pointer" }}
          title={`${displayName} — open status / sign out`}
          onClick={onOpenStatus}
        >
          {displayName.slice(0, 2).toLowerCase()}
        </span>
      </div>
    </header>
  );
}

function SettingsGlyph() {
  return (
    <svg width="14" height="14" viewBox="0 0 12 12" aria-hidden>
      <path
        d="M1 3h10M1 6h10M1 9h10"
        stroke="var(--text-muted)"
        strokeWidth="1.3"
      />
    </svg>
  );
}

const S = {
  bar: {
    display: "flex",
    alignItems: "center",
    gap: 14,
    height: "var(--appbar-height, 46px)",
    padding: "0 16px",
    flex: "none",
    background: "var(--bg-panel)",
    borderBottom: "1px solid var(--border-black)",
  } as const,
  brand: { display: "flex", alignItems: "center", gap: 9 } as const,
  logo: {
    width: 22,
    height: 22,
    borderRadius: 5,
    background: "var(--bg-selected)",
    border: "1px solid var(--accent-cyan)",
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    color: "var(--accent-cyan-bright)",
    fontWeight: 700,
    fontSize: 12,
  } as const,
  wordmark: {
    fontWeight: 700,
    color: "var(--text-primary)",
    fontSize: 12.5,
    letterSpacing: ".02em",
  } as const,
  sep: { width: 1, height: 20, background: "var(--border-strong)" } as const,
  addBtn: {
    display: "flex",
    alignItems: "center",
    gap: 6,
    padding: "5px 12px",
    border: "1px solid var(--accent-cyan)",
    background: "var(--bg-selected)",
    color: "var(--accent-cyan-bright)",
    borderRadius: 5,
    fontSize: 11,
    fontWeight: 600,
    fontFamily: "inherit",
    cursor: "pointer",
  } as const,
  magnetBtn: {
    display: "flex",
    alignItems: "center",
    gap: 6,
    padding: "5px 12px",
    border: "1px solid var(--border-strong)",
    background: "var(--bg-track)",
    color: "var(--text-body)",
    borderRadius: 5,
    fontSize: 11,
    fontFamily: "inherit",
    cursor: "pointer",
  } as const,
  search: {
    width: 240,
    padding: "5px 11px",
    border: "1px solid var(--border-strong)",
    borderRadius: 5,
    background: "var(--bg-field)",
    fontSize: 11,
    color: "var(--text-body)",
    fontFamily: "inherit",
  } as const,
  speeds: {
    display: "flex",
    alignItems: "center",
    gap: 14,
    fontSize: 10.5,
  } as const,
  right: { display: "flex", alignItems: "center", gap: 10 } as const,
  connWrap: { display: "flex", alignItems: "center", gap: 6 } as const,
  dot: (ok: boolean) =>
    ({
      width: 7,
      height: 7,
      borderRadius: "50%",
      background: ok ? "var(--accent-green)" : "var(--accent-red)",
    }) as const,
  iconBtn: {
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    width: 26,
    height: 26,
    border: "none",
    background: "none",
    borderRadius: 5,
    cursor: "pointer",
  } as const,
  avatar: {
    width: 24,
    height: 24,
    borderRadius: "50%",
    background: "var(--bg-track)",
    border: "1px solid var(--border-strong)",
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    color: "var(--text-muted)",
    fontSize: 10,
  } as const,
};
