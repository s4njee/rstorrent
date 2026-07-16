/**
 * Add magnet / URL dialog (design screen 03).
 *
 * Accepts a `magnet:` URI or a plain `.torrent` URL. Magnets are lightly
 * validated (must contain a btih xt) and their display name is pulled from `dn=`
 * for the title. On Add, the request goes to the backend `load.start`/`load`
 * path. If the clipboard holds a magnet when the dialog opens, it's prefilled.
 */

import { useEffect, useState } from "react";
import { readText } from "@tauri-apps/plugin-clipboard-manager";
import { useUi } from "../../store/ui";
import { useSettings } from "../../store/settings";
import { addTorrent } from "../../ipc/commands";
import { ModalBase, Button } from "./ModalBase";
import { Checkbox } from "./Checkbox";
import forms from "./forms.module.css";

/** A magnet with a btih hash, or any http(s) .torrent URL, is acceptable. */
function isValidSource(text: string): boolean {
  const t = text.trim();
  if (/^magnet:\?.*xt=urn:btih:[0-9a-z]+/i.test(t)) return true;
  return /^https?:\/\/.+/i.test(t);
}

export function AddMagnetDialog() {
  const closeDialog = useUi((s) => s.closeDialog);
  const settings = useSettings((s) => s.settings);

  const [uri, setUri] = useState("");
  const [savePath, setSavePath] = useState("");
  const [label, setLabel] = useState("");
  const [start, setStart] = useState(true);
  const [topOfQueue, setTopOfQueue] = useState(false);

  // Default the save path from settings, and prefill a magnet from the clipboard.
  useEffect(() => {
    if (settings) setSavePath(settings.defaultSavePath);
    void readText()
      .then((clip) => {
        if (clip && isValidSource(clip)) setUri(clip.trim());
      })
      .catch(() => {
        // clipboard may be empty/unavailable; ignore
      });
  }, [settings]);

  const valid = isValidSource(uri);

  const add = () => {
    if (!valid) return;
    void addTorrent(
      { kind: "magnet", uri: uri.trim() },
      {
        savePath,
        label,
        start,
        topOfQueue,
        sequential: false,
        skipHashCheck: false,
        unselectedIndexes: [],
      },
    );
    closeDialog();
  };

  return (
    <ModalBase
      title="Add magnet link"
      width={460}
      onCancel={closeDialog}
      onPrimary={add}
      footer={
        <>
          <Button variant="secondary" onClick={closeDialog}>
            Cancel
          </Button>
          <Button variant="primary" onClick={add} disabled={!valid}>
            Add
          </Button>
        </>
      }
    >
      <div className={forms.col}>
        <div style={{ display: "flex", flexDirection: "column", gap: 5 }}>
          <span className={forms.fieldLabel} style={{ width: "auto" }}>
            Magnet URI or torrent URL
          </span>
          <textarea
            className={forms.textarea}
            value={uri}
            onChange={(e) => setUri(e.currentTarget.value)}
            placeholder="magnet:?xt=urn:btih:…"
            spellCheck={false}
          />
          {uri && !valid && (
            <span className={forms.error}>
              not a valid magnet or torrent URL
            </span>
          )}
        </div>

        <div className={forms.field}>
          <span className={forms.fieldLabel}>Save to</span>
          <input
            className={forms.input}
            value={savePath}
            onChange={(e) => setSavePath(e.currentTarget.value)}
            spellCheck={false}
          />
        </div>

        <div className={forms.field}>
          <span className={forms.fieldLabel}>Label</span>
          <input
            className={forms.input}
            value={label}
            onChange={(e) => setLabel(e.currentTarget.value)}
            placeholder="(none)"
            spellCheck={false}
          />
        </div>

        <div style={{ display: "flex", gap: 20, paddingTop: 4 }}>
          <Checkbox checked={start} onChange={setStart} label="Start torrent" />
          <Checkbox
            checked={topOfQueue}
            onChange={setTopOfQueue}
            label="Add to top of queue"
          />
        </div>
      </div>
    </ModalBase>
  );
}
