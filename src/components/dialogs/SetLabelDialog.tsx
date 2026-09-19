/**
 * Set label: applies one label to the whole selection, or clears it.
 *
 * The toolbar has no room for the context menu's hover submenu, and a dialog is
 * the better shape for it anyway — the existing labels are offered as chips so a
 * typo cannot fork one label into two, and the same field takes a new one.
 */

import { useState } from "react";
import { useTorrents } from "../../store/torrents";
import { useUi } from "../../store/ui";
import { setLabel } from "../../ipc/commands";
import { ModalBase, Button } from "./ModalBase";
import { reportFailure } from "../../store/notices";
import forms from "./forms.module.css";

export function SetLabelDialog({ onClose }: { onClose: () => void }) {
  const torrents = useTorrents((s) => s.torrents);
  const selection = useUi((s) => s.selection);
  // `draft`, not `label`: `setLabel` below is the command that applies it.
  const [draft, setDraft] = useState("");
  const [error, setError] = useState<string | null>(null);

  // Distinct labels already in use, for the chips.
  const existing = [
    ...new Set(torrents.map((torrent) => torrent.label).filter(Boolean)),
  ].sort();

  const selected = [...selection];
  const apply = (value: string) => {
    if (selected.length === 0) return;
    setLabel(selected, value.trim())
      .then(onClose)
      .catch((cause: unknown) => {
        setError(cause instanceof Error ? cause.message : String(cause));
        reportFailure("Could not set the label", cause);
      });
  };

  return (
    <ModalBase
      title="Set label"
      width={420}
      onCancel={onClose}
      onPrimary={() => apply(draft)}
      footer={
        <>
          <Button variant="secondary" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="secondary" onClick={() => apply("")}>
            Clear
          </Button>
          <Button variant="primary" onClick={() => apply(draft)}>
            Apply
          </Button>
        </>
      }
    >
      <div className={forms.col}>
        <div className={forms.field}>
          <span className={forms.fieldLabel}>Label</span>
          <input
            className={forms.input}
            value={draft}
            onChange={(event) => setDraft(event.currentTarget.value)}
            placeholder="label"
            spellCheck={false}
            autoFocus
          />
        </div>
        {existing.length > 0 && (
          <div className={forms.field}>
            <span className={forms.fieldLabel}>In use</span>
            <div
              className={forms.grow}
              style={{ display: "flex", flexWrap: "wrap", gap: 6 }}
            >
              {existing.map((name) => (
                <button
                  key={name}
                  type="button"
                  className={forms.chip}
                  onClick={() => setDraft(name)}
                >
                  {name}
                </button>
              ))}
            </div>
          </div>
        )}
        <span className={forms.meta}>
          Applies to {selected.length} torrent
          {selected.length === 1 ? "" : "s"}. Clear removes the label.
        </span>
        {error && <span className={forms.error}>{error}</span>}
      </div>
    </ModalBase>
  );
}
