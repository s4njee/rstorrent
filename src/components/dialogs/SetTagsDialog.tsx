/**
 * Edit tags on the selection (V3-10).
 *
 * Tags are many-to-many and stored in `d.custom=tags`. The dialog works by
 * add/remove rather than an absolute set: "some torrents have it" has no single
 * answer to write back, whereas "add X to all" and "remove Y from all" are both
 * unambiguous. The chips show the union across the selection; clicking one marks
 * it for removal, and the field adds new ones.
 */

import { useMemo, useState } from "react";
import { useTorrents } from "../../store/torrents";
import { useUi } from "../../store/ui";
import { addTags, removeTags, setTags } from "../../ipc/commands";
import { normaliseTags, tagChip, unionTags } from "../../utils/tags";
import { reportFailure } from "../../store/notices";
import { ModalBase, Button } from "./ModalBase";
import forms from "./forms.module.css";

export function SetTagsDialog({ onClose }: { onClose: () => void }) {
  const torrents = useTorrents((s) => s.torrents);
  const selection = useUi((s) => s.selection);
  const [draft, setDraft] = useState("");
  const [toAdd, setToAdd] = useState<string[]>([]);
  const [toRemove, setToRemove] = useState<Set<string>>(new Set());
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const selected = [...selection];
  const selectedTorrents = useMemo(
    () => torrents.filter((t) => selection.has(t.hash)),
    [torrents, selection],
  );

  /** The union of tags across the selection, plus any staged additions. */
  const union = useMemo(() => {
    const present = unionTags(selectedTorrents.map((t) => t.tags ?? []));
    return normaliseTags([...present, ...toAdd]).sort((a, b) =>
      a.localeCompare(b),
    );
  }, [selectedTorrents, toAdd]);

  const stage = (value: string) => {
    const added = normaliseTags([value]);
    if (added.length === 0) return;
    setToAdd((current) => normaliseTags([...current, ...added]));
    // Adding a tag that was staged for removal cancels the removal.
    setToRemove((current) => {
      const next = new Set(current);
      for (const tag of added) next.delete(tag);
      return next;
    });
    setDraft("");
  };

  const toggleRemove = (tag: string) => {
    setToRemove((current) => {
      const next = new Set(current);
      if (next.has(tag)) next.delete(tag);
      else next.add(tag);
      return next;
    });
    // A staged addition that is then marked for removal is dropped.
    setToAdd((current) =>
      current.filter((t) => t.toLowerCase() !== tag.toLowerCase()),
    );
  };

  const apply = () => {
    if (selected.length === 0) {
      onClose();
      return;
    }
    const removes = [...toRemove];
    const adds = toAdd.filter(
      (tag) => !removes.some((r) => r.toLowerCase() === tag.toLowerCase()),
    );
    setBusy(true);
    setError(null);
    (async () => {
      if (removes.length > 0) await removeTags(selected, removes);
      if (adds.length > 0) await addTags(selected, adds);
    })()
      .then(onClose)
      .catch((cause: unknown) => {
        setBusy(false);
        setError(cause instanceof Error ? cause.message : String(cause));
        reportFailure("Could not update the tags", cause);
      });
  };

  const clearAll = () => {
    if (selected.length === 0) {
      onClose();
      return;
    }
    setTags(selected, [])
      .then(onClose)
      .catch((cause: unknown) => {
        setError(cause instanceof Error ? cause.message : String(cause));
        reportFailure("Could not clear the tags", cause);
      });
  };

  return (
    <ModalBase
      title="Edit tags"
      width={440}
      onCancel={onClose}
      onPrimary={apply}
      footer={
        <>
          <Button variant="secondary" onClick={onClose} disabled={busy}>
            Cancel
          </Button>
          <Button variant="secondary" onClick={clearAll} disabled={busy}>
            Clear all
          </Button>
          <Button variant="primary" onClick={apply} disabled={busy}>
            {busy ? "Applying…" : "Apply"}
          </Button>
        </>
      }
    >
      <div className={forms.col}>
        <div className={forms.field}>
          <label className={forms.fieldLabel} htmlFor="add-tags-input">
            Add tags
          </label>
          <input
            id="add-tags-input"
            className={forms.input}
            value={draft}
            onChange={(event) => setDraft(event.currentTarget.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" || event.key === ",") {
                event.preventDefault();
                event.stopPropagation();
                stage(draft);
              }
            }}
            placeholder="comma-separated, Enter to add"
            spellCheck={false}
            autoFocus
          />
        </div>

        {union.length > 0 && (
          <div className={forms.field}>
            <span className={forms.fieldLabel}>Tags</span>
            <div
              className={forms.grow}
              style={{ display: "flex", flexWrap: "wrap", gap: 6 }}
            >
              {union.map((tag) => {
                const style = tagChip(tag);
                const marked = toRemove.has(tag);
                return (
                  <button
                    key={tag}
                    type="button"
                    className={forms.chip}
                    aria-pressed={!marked}
                    title={marked ? `Keep ${tag}` : `Remove ${tag}`}
                    style={{
                      background: style.background,
                      color: style.color,
                      textDecoration: marked ? "line-through" : undefined,
                      opacity: marked ? 0.55 : 1,
                    }}
                    onClick={() => toggleRemove(tag)}
                  >
                    {tag}
                  </button>
                );
              })}
            </div>
          </div>
        )}

        <span className={forms.meta}>
          {selected.length} torrent{selected.length === 1 ? "" : "s"} selected.
          Click a tag to mark it for removal; tags you add apply to all of them.
          Tags follow the daemon session.
        </span>
        {error && <span className={forms.error}>{error}</span>}
      </div>
    </ModalBase>
  );
}
