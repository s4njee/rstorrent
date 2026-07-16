/**
 * Remove-confirmation dialog (design screen 07).
 *
 * Confirms removing the selected torrent(s) from the transfer list, with an
 * optional "also delete downloaded files" checkbox in the danger palette. The
 * delete-data option is disabled when the daemon isn't local (we can't reach its
 * files). Enter = Remove, Esc = Cancel.
 */

import { useState } from "react";
import { useUi } from "../../store/ui";
import { useTorrents } from "../../store/torrents";
import { useSettings, isLocalhost } from "../../store/settings";
import { remove } from "../../ipc/commands";
import { formatBytes } from "../../utils/format";
import { ModalBase, Button } from "./ModalBase";
import styles from "./RemoveDialog.module.css";

export function RemoveDialog() {
  const closeDialog = useUi((s) => s.closeDialog);
  const selection = useUi((s) => s.selection);
  const torrents = useTorrents((s) => s.torrents);
  const settings = useSettings((s) => s.settings);

  const [deleteData, setDeleteData] = useState(false);

  const selected = torrents.filter((t) => selection.has(t.hash));
  const totalSize = selected.reduce((sum, t) => sum + t.size, 0);
  const canDeleteData = isLocalhost(settings?.transport);

  const label =
    selected.length === 1 ? (
      <>
        Remove <b>{selected[0].name}</b> from the transfer list?
      </>
    ) : (
      <>
        Remove <b>{selected.length} torrents</b> from the transfer list?
      </>
    );

  const confirm = () => {
    void remove([...selection], deleteData && canDeleteData);
    closeDialog();
  };

  return (
    <ModalBase
      title="Remove torrent"
      width={400}
      onCancel={closeDialog}
      onPrimary={confirm}
      footer={
        <>
          <Button variant="secondary" onClick={closeDialog}>
            Cancel
          </Button>
          <Button variant="danger" onClick={confirm}>
            Remove
          </Button>
        </>
      }
    >
      <div className={styles.row}>
        <div className={styles.badge}>!</div>
        <div className={styles.message}>
          <div>{label}</div>
          <label
            className={`${styles.check} ${canDeleteData ? "" : styles.disabled}`}
            title={
              canDeleteData
                ? undefined
                : "unavailable: the daemon isn't on this machine"
            }
          >
            <span className={styles.box}>
              {deleteData && canDeleteData ? "✓" : ""}
            </span>
            <input
              type="checkbox"
              hidden
              disabled={!canDeleteData}
              checked={deleteData && canDeleteData}
              onChange={(e) => setDeleteData(e.currentTarget.checked)}
            />
            Also delete downloaded files ({formatBytes(totalSize)})
          </label>
        </div>
      </div>
    </ModalBase>
  );
}
