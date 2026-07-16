import { useState } from "react";
import { setTorrentLimits } from "../../ipc/commands";
import { useTorrents } from "../../store/torrents";
import { useUi } from "../../store/ui";
import { Button, ModalBase } from "./ModalBase";
import forms from "./forms.module.css";

/** Custom per-torrent download/upload limits. Values are KiB/s; zero leaves
 * that direction unlimited. Two zeroes clear the named-throttle assignment. */
export function RateLimitDialog() {
  const close = useUi((state) => state.closeDialog);
  const selection = useUi((state) => state.selection);
  const torrents = useTorrents((state) => state.torrents);
  const selected =
    selection.size === 1
      ? torrents.find((torrent) => torrent.hash === [...selection][0])
      : null;
  const [down, setDown] = useState(() =>
    String(
      selected?.downRateLimit != null ? selected.downRateLimit / 1024 : 1024,
    ),
  );
  const [up, setUp] = useState(() =>
    String(selected?.upRateLimit != null ? selected.upRateLimit / 1024 : 1024),
  );
  const [error, setError] = useState("");
  const [saving, setSaving] = useState(false);

  const parse = (value: string) => {
    const parsed = Number(value);
    return Number.isSafeInteger(parsed) && parsed >= 0 ? parsed : null;
  };

  const apply = async () => {
    const downKb = parse(down);
    const upKb = parse(up);
    if (downKb == null || upKb == null) {
      setError("enter whole numbers greater than or equal to zero");
      return;
    }
    setSaving(true);
    setError("");
    try {
      await setTorrentLimits([...selection], downKb, upKb);
      close();
    } catch (cause) {
      setError(String(cause));
      setSaving(false);
    }
  };

  return (
    <ModalBase
      title="Per-torrent rate limit"
      width={390}
      onCancel={close}
      onPrimary={() => void apply()}
      footer={
        <>
          <Button variant="secondary" onClick={close}>
            Cancel
          </Button>
          <Button
            variant="primary"
            disabled={saving || selection.size === 0}
            onClick={() => void apply()}
          >
            {saving ? "Applying…" : "Apply"}
          </Button>
        </>
      }
    >
      <div className={forms.col}>
        <label className={forms.field}>
          <span className={forms.fieldLabel}>download</span>
          <input
            className={forms.input}
            type="number"
            min="0"
            step="1"
            value={down}
            onChange={(event) => setDown(event.currentTarget.value)}
          />
          <span className={forms.meta}>KiB/s</span>
        </label>
        <label className={forms.field}>
          <span className={forms.fieldLabel}>upload</span>
          <input
            className={forms.input}
            type="number"
            min="0"
            step="1"
            value={up}
            onChange={(event) => setUp(event.currentTarget.value)}
          />
          <span className={forms.meta}>KiB/s</span>
        </label>
        <div className={forms.meta}>
          0 means unlimited; setting both to 0 clears the torrent limit.
        </div>
        {error && <div className={forms.error}>{error}</div>}
      </div>
    </ModalBase>
  );
}
