/**
 * Recheck confirmation (WC9-S3).
 *
 * A forced recheck of an actively downloading torrent throws away in-flight
 * work and re-verifies what is on disk, so the console asks first when the
 * selection includes one. A stopped set skips this dialog entirely.
 */

import { useUi } from "../../store/ui";
import { useTorrents } from "../../store/torrents";
import { recheck } from "../../ipc/commands";
import { ModalBase, Button } from "./ModalBase";

export function RecheckDialog() {
  const closeDialog = useUi((s) => s.closeDialog);
  const selection = useUi((s) => s.selection);
  const torrents = useTorrents((s) => s.torrents);

  const selected = torrents.filter((t) => selection.has(t.hash));
  const downloading = selected.filter((t) => t.status === "downloading").length;

  const confirm = () => {
    void recheck([...selection]);
    closeDialog();
  };

  return (
    <ModalBase
      title="Force recheck"
      width={420}
      onCancel={closeDialog}
      onPrimary={confirm}
      footer={
        <>
          <Button variant="secondary" onClick={closeDialog}>
            Cancel
          </Button>
          <Button variant="primary" onClick={confirm}>
            Recheck
          </Button>
        </>
      }
    >
      <p>
        {downloading > 0
          ? `${downloading} of these torrents ${
              downloading === 1 ? "is" : "are"
            } still downloading. Rechecking discards in-flight progress and
            verifies the data on disk.`
          : "Recheck verifies the data on disk."}
      </p>
      <p>
        Recheck{" "}
        <b>
          {selected.length === 1
            ? selected[0]?.name
            : `${selected.length} torrents`}
        </b>
        ?
      </p>
    </ModalBase>
  );
}
