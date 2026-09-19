/**
 * Set-location dialog (B12 / E15-S3).
 *
 * Sets the download directory for selected torrents with an optional
 * "Move files to new location" checkbox (defaulting to on when the daemon
 * is local). When moving, data on disk is safely relocated before updating
 * rtorrent's directory.
 */

import { useState } from "react";
import { useUi } from "../../store/ui";
import { useTorrents } from "../../store/torrents";
import { useSettings, isLocalhost } from "../../store/settings";
import { setLocation } from "../../ipc/commands";
import { capabilities } from "../../ipc/backend";
import { ModalBase, Button } from "./ModalBase";
import { Checkbox } from "./Checkbox";
import forms from "./forms.module.css";
import styles from "./SetLocationDialog.module.css";

export function SetLocationDialog() {
  const closeDialog = useUi((s) => s.closeDialog);
  const selection = useUi((s) => s.selection);
  const torrents = useTorrents((s) => s.torrents);
  const settings = useSettings((s) => s.settings);

  const selected = torrents.filter((t) => selection.has(t.hash));
  const isLocal = isLocalhost(settings?.transport);
  const canNative = capabilities().nativeDialogs;

  const [savePath, setSavePath] = useState(
    selected[0]?.savePath || settings?.defaultSavePath || "",
  );
  const [moveData, setMoveData] = useState(true);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const label =
    selected.length === 1 ? (
      <>
        Set location for <b>{selected[0].name}</b>
      </>
    ) : (
      <>
        Set location for <b>{selected.length} torrents</b>
      </>
    );

  const browse = async () => {
    if (!canNative) return;
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const dir = await open({
        directory: true,
        defaultPath: savePath || undefined,
      });
      if (typeof dir === "string") setSavePath(dir);
    } catch {
      // User cancelled dialog or plugin unavailable
    }
  };

  const confirm = async () => {
    const trimmed = savePath.trim();
    if (!trimmed || submitting) return;

    setSubmitting(true);
    setError(null);

    try {
      const targets =
        selected.length > 0 ? selected.map((t) => t.hash) : [...selection];
      for (const hash of targets) {
        await setLocation(hash, trimmed, moveData && isLocal);
      }
      closeDialog();
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : String(err));
      setSubmitting(false);
    }
  };

  return (
    <ModalBase
      title="Set location"
      width={460}
      onCancel={closeDialog}
      onPrimary={confirm}
      footer={
        <>
          <Button
            variant="secondary"
            onClick={closeDialog}
            disabled={submitting}
          >
            Cancel
          </Button>
          <Button
            variant="primary"
            onClick={() => void confirm()}
            disabled={!savePath.trim() || submitting}
          >
            {submitting ? "Moving…" : "Set Location"}
          </Button>
        </>
      }
    >
      <div className={styles.body}>
        <div className={styles.message}>{label}</div>

        <div className={forms.field}>
          <span className={forms.fieldLabel}>Location</span>
          <input
            className={forms.input}
            value={savePath}
            onChange={(e) => setSavePath(e.currentTarget.value)}
            placeholder="/path/to/download"
            disabled={submitting}
            autoFocus
          />
          {canNative && (
            <button
              type="button"
              className={forms.browse}
              onClick={() => void browse()}
              disabled={submitting}
            >
              Browse…
            </button>
          )}
        </div>

        <Checkbox
          checked={moveData && isLocal}
          onChange={setMoveData}
          disabled={!isLocal || submitting}
          label="Move files to new location"
          title={
            isLocal
              ? undefined
              : "unavailable: the daemon is not on this machine"
          }
        />

        {error && <div className={forms.error}>{error}</div>}
      </div>
    </ModalBase>
  );
}
