/**
 * Action strip (WE2-S3) — above the table: Resume / Pause / Remove · separator ·
 * queue up/down · right-aligned "n of m selected". Buttons disable on an empty
 * selection or while disconnected. Queue arrows map to rtorrent priority (the
 * honest "priority" tooltip, per plan §2).
 */

import { useTorrents } from "../store/torrents";
import { useUi } from "../store/ui";
import * as actions from "../actions";
import {
  PlayIcon,
  PauseIcon,
  RemoveIcon,
  UpIcon,
  DownIcon,
} from "../components/icons";

export function ActionStrip() {
  const selection = useUi((s) => s.selection);
  const total = useTorrents((s) => s.torrents.length);
  const connected = useTorrents((s) => s.connection.phase === "connected");

  const n = selection.size;
  const disabled = n === 0 || !connected;

  return (
    <div style={S.strip}>
      <Btn title="Resume" disabled={disabled} onClick={() => actions.resume()}>
        <PlayIcon size={12} />
      </Btn>
      <Btn title="Pause" disabled={disabled} onClick={() => actions.pause()}>
        <PauseIcon size={12} />
      </Btn>
      <Btn
        title="Remove"
        disabled={disabled}
        onClick={() => actions.requestRemove()}
      >
        <RemoveIcon size={13} />
      </Btn>

      <span style={S.sep} />

      <Btn
        title="Move up (priority)"
        disabled={disabled}
        onClick={() => actions.queueUp()}
      >
        <UpIcon size={12} />
      </Btn>
      <Btn
        title="Move down (priority)"
        disabled={disabled}
        onClick={() => actions.queueDown()}
      >
        <DownIcon size={12} />
      </Btn>

      <span style={{ flex: 1 }} />
      <span style={S.count}>
        {n} of {total} selected
      </span>
    </div>
  );
}

function Btn({
  title,
  disabled,
  onClick,
  children,
}: {
  title: string;
  disabled: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      style={{ ...S.btn, opacity: disabled ? 0.35 : 1 }}
      title={title}
      aria-label={title}
      disabled={disabled}
      onClick={onClick}
    >
      {children}
    </button>
  );
}

const S = {
  strip: {
    display: "flex",
    alignItems: "center",
    gap: 2,
    padding: "6px 10px",
    flex: "none",
    background: "var(--bg-app)",
    borderBottom: "1px solid var(--border-mid)",
  } as const,
  btn: {
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    width: 26,
    height: 24,
    border: "none",
    background: "none",
    borderRadius: 4,
    cursor: "pointer",
    color: "var(--text-muted)",
  } as const,
  sep: {
    width: 1,
    height: 16,
    background: "var(--border-strong)",
    margin: "0 5px",
  } as const,
  count: { fontSize: 10.5, color: "var(--text-dim)" } as const,
};
