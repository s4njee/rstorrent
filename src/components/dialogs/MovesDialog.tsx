/**
 * Moves dialog (V3-14): every unfinished move-on-complete entry with its
 * state, byte progress and error, plus Cancel (live) / Retry (failed).
 *
 * The list hydrates on open and follows the `moves://update` nudge, so a
 * finished move disappears promptly without polling.
 */

import { useEffect } from "react";
import { useUi } from "../../store/ui";
import { activeMoves, movePercent, useMoves } from "../../store/moves";
import { cancelMove, getMoves, retryMove } from "../../ipc/commands";
import type { MoveState, MoveStatus } from "../../ipc/types";
import { formatBytes } from "../../utils/format";
import { ModalBase, Button } from "./ModalBase";
import forms from "./forms.module.css";

const STATE_LABEL: Record<MoveState, string> = {
  pending: "Queued",
  in_progress: "Moving",
  done: "Done",
  failed: "Failed",
  cancelled: "Cancelled",
};

function progressText(m: MoveStatus): string {
  const pct = movePercent(m);
  if (m.state !== "in_progress" || pct === null) return "";
  return ` · ${pct}% (${formatBytes(m.doneBytes)} of ${formatBytes(m.totalBytes)})`;
}

function Row({ m }: { m: MoveStatus }) {
  const live = m.state === "pending" || m.state === "in_progress";
  const retryable = m.state === "failed" || m.state === "cancelled";
  return (
    <div className={forms.field} style={{ alignItems: "flex-start" }}>
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ overflow: "hidden", textOverflow: "ellipsis" }}>
          {m.name}{" "}
          <span className={forms.meta}>
            {STATE_LABEL[m.state]}
            {progressText(m)}
          </span>
        </div>
        <div
          className={forms.meta}
          style={{ overflow: "hidden", textOverflow: "ellipsis" }}
        >
          {m.src} → {m.dst}
        </div>
        {m.error && <div className={forms.error}>{m.error}</div>}
      </div>
      {live && (
        <button
          type="button"
          className={forms.browse}
          title="Cancel move (the torrent resumes in place)"
          onClick={() => void cancelMove(m.id).catch(() => {})}
        >
          Cancel
        </button>
      )}
      {retryable && (
        <button
          type="button"
          className={forms.browse}
          title="Drop this entry so the next tick re-plans from daemon truth"
          onClick={() => void retryMove(m.hash).catch(() => {})}
        >
          Retry
        </button>
      )}
    </div>
  );
}

export function MovesDialog() {
  const closeDialog = useUi((s) => s.closeDialog);
  const moves = useMoves((s) => s.moves);
  const set = useMoves((s) => s.set);

  useEffect(() => {
    void getMoves().then(set, () => {});
  }, [set]);
  const live = activeMoves(moves);

  return (
    <ModalBase
      title={live.length > 0 ? `Moves (${live.length} active)` : "Moves"}
      width={560}
      onCancel={closeDialog}
      onPrimary={closeDialog}
      footer={
        <Button variant="primary" onClick={closeDialog}>
          Close
        </Button>
      }
    >
      <div className={forms.col}>
        {moves.length === 0 && (
          <span className={forms.meta}>
            No moves in flight or awaiting retry. Completed moves are in the
            log.
          </span>
        )}
        {moves.map((m) => (
          <Row key={m.id} m={m} />
        ))}
      </div>
    </ModalBase>
  );
}
