/**
 * Shut-down-daemon confirmation (D13).
 *
 * `system.shutdown.normal` stops the rtorrent process, so this is guarded by a
 * confirm step. After it fires the connection drops and the app shows its
 * disconnected card until a daemon is running again.
 */

import { useState } from "react";
import { useUi } from "../../store/ui";
import { shutdownDaemon } from "../../ipc/commands";
import { ModalBase, Button } from "./ModalBase";
import forms from "./forms.module.css";

export function ShutdownDialog() {
  const closeDialog = useUi((s) => s.closeDialog);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const confirm = () => {
    if (busy) return;
    setBusy(true);
    setError(null);
    shutdownDaemon()
      .then(closeDialog)
      .catch((e: unknown) => {
        setError(String(e));
        setBusy(false);
      });
  };

  return (
    <ModalBase
      title="Shut down daemon"
      width={420}
      onCancel={closeDialog}
      onPrimary={confirm}
      footer={
        <>
          <Button variant="secondary" onClick={closeDialog} disabled={busy}>
            Cancel
          </Button>
          <Button variant="danger" onClick={confirm} disabled={busy}>
            {busy ? "Shutting down…" : "Shut down"}
          </Button>
        </>
      }
    >
      <div className={forms.col}>
        <div className={forms.meta}>
          This stops the rtorrent process. Active torrents will pause and the
          app will disconnect until the daemon is running again.
        </div>
        {error && (
          <div
            className={forms.meta}
            style={{ color: "var(--accent-red-soft, #ea6962)" }}
          >
            {error}
          </div>
        )}
      </div>
    </ModalBase>
  );
}
