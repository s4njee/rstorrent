/**
 * The unified add modal (WC6).
 *
 * One modal serves every add flow, replacing the old `AddTorrentDialog` and
 * `AddMagnetDialog`. A segmented control switches between the magnet/URL pane
 * and the `.torrent` file pane (a dropzone); a `.torrent` is inspected to show
 * the tri-state contents tree, and the shared options (destination, label,
 * start, skip hash check) apply to whatever is added.
 *
 * A `.torrent` comes in as a browser `File` (`<input type="file">`, a drop, or
 * a queued external add) → `POST /api/torrents/inspect` for the tree, then
 * `POST /api/torrents/file` to add it.
 *
 * SCoped honestly for a queue of more than one file: the contents tree and its
 * per-file deselection describe the **first** file only; a multi-file add uses
 * the shared options and selects every file in each torrent (rtorrent has no
 * cross-torrent selection to apply).
 */

import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type ChangeEvent,
  type DragEvent,
} from "react";
import { useUi } from "../../store/ui";
import { useSettings } from "../../store/settings";
import { useTorrents } from "../../store/torrents";
import { addTorrent, addTracker } from "../../ipc/commands";
import { webInspectTorrent, webUploadTorrent } from "../../ipc/web";
import type { TorrentMeta } from "../../ipc/types";
import { formatBytes } from "../../utils/format";
import {
  magnetHash,
  magnetName,
  parseSourceLines,
} from "../../utils/addSource";
import { detectDuplicate, type AddCandidate } from "../../utils/duplicates";
import { buildTree, leafIndexes, type TreeNode } from "../../utils/filetree";
import { ModalBase, Button } from "./ModalBase";
import { Checkbox } from "./Checkbox";
import { FileTree } from "./FileTree";
import forms from "./forms.module.css";
import styles from "./AddModal.module.css";

type Mode = "magnet" | "file";

/** One queued `.torrent` file. */
interface QueueItem {
  key: string;
  file: File;
  name: string;
}

/** Queue entry for a browser `File`. */
function uploadItem(file: File): QueueItem {
  return { key: `upload:${file.name}`, file, name: file.name };
}

/** Read clipboard text; null when the browser denies it. */
async function readClipboardText(): Promise<string | null> {
  try {
    return await navigator.clipboard.readText();
  } catch {
    return null;
  }
}

export function AddModal({ initialMode }: { initialMode: Mode }) {
  const closeDialog = useUi((s) => s.closeDialog);
  const external = useUi((s) => s.externalAddRequest);
  const settings = useSettings((s) => s.settings);
  const torrents = useTorrents((s) => s.torrents);

  const [mode, setMode] = useState<Mode>(initialMode);
  const [uriText, setUriText] = useState("");
  const [queue, setQueue] = useState<QueueItem[]>([]);
  const [meta, setMeta] = useState<TorrentMeta | null>(null);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [dragOver, setDragOver] = useState(false);

  const [savePath, setSavePath] = useState("");
  const [label, setLabel] = useState("");
  const [start, setStart] = useState(true);
  const [skipHash, setSkipHash] = useState(false);
  const [topOfQueue, setTopOfQueue] = useState(false);
  const [adding, setAdding] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const fileInputRef = useRef<HTMLInputElement>(null);

  const labels = useMemo(
    () => [...new Set(torrents.map((t) => t.label).filter(Boolean))].sort(),
    [torrents],
  );

  useEffect(() => {
    if (settings) setSavePath(settings.defaultSavePath);
  }, [settings]);

  /** Apply parsed metadata to the tree and selection. */
  const applyMeta = (m: TorrentMeta) => {
    setMeta(m);
    setSelected(new Set(m.files.map((_, i) => i)));
    setExpanded(
      new Set(
        buildTree(m.files)
          .filter((n) => n.isDir)
          .map((n) => n.name),
      ),
    );
  };

  /** Queue a source and inspect it (the first item drives the tree). */
  const queueSource = async (item: QueueItem) => {
    setQueue((prev) =>
      prev.some((q) => q.key === item.key) ? prev : [...prev, item],
    );
    setMode("file");
    setError(null);
    try {
      applyMeta(await webInspectTorrent(item.file));
    } catch (e) {
      setError(String(e));
    }
  };

  // External deep-link / drop / paste sources prefill on open; a manual open
  // shows the dropzone and waits.
  const started = useRef(false);
  useEffect(() => {
    if (started.current) return;
    started.current = true;
    void (async () => {
      const source = external?.source;
      if (source?.kind === "magnet") {
        setMode("magnet");
        setUriText(source.uri);
        return;
      }
      if (source?.kind === "upload") {
        await queueSource(uploadItem(source.file));
        return;
      }
      // No external source: offer a magnet on the clipboard, as before.
      void readClipboardText().then((clip) => {
        if (clip && parseSourceLines(clip).some((line) => line.valid)) {
          setUriText(clip.trim());
        }
      });
    })();
    // Mount-only: the dialog is keyed per open.
    // eslint-disable-next-line react-hooks/exhaustive-deps -- intentional once-per-open
  }, []);

  const onLabelChange = (value: string) => {
    setLabel(value);
    const preset = settings?.labelDefaults.find(
      (d) => d.label === value && d.savePath,
    );
    if (preset) setSavePath(preset.savePath);
  };

  const pickFiles = () => {
    fileInputRef.current?.click();
  };

  const onFileInputChange = (e: ChangeEvent<HTMLInputElement>) => {
    const files = Array.from(e.target.files ?? []);
    void (async () => {
      for (const file of files) await queueSource(uploadItem(file));
    })();
    e.target.value = "";
  };

  const onDrop = (e: DragEvent<HTMLDivElement>) => {
    e.preventDefault();
    setDragOver(false);
    const files = Array.from(e.dataTransfer?.files ?? []);
    void (async () => {
      for (const file of files) await queueSource(uploadItem(file));
    })();
  };

  const removeQueued = (key: string) => {
    setQueue((prev) => prev.filter((item) => item.key !== key));
    // A single remaining item re-inspects to keep the tree honest; clearing to
    // none drops it.
    setMeta(null);
    setSelected(new Set());
    setExpanded(new Set());
  };

  const toggleNode = (node: TreeNode, on: boolean) => {
    const idxs = leafIndexes(node);
    setSelected((prev) => {
      const next = new Set(prev);
      idxs.forEach((i) => (on ? next.add(i) : next.delete(i)));
      return next;
    });
  };

  const setAll = (on: boolean) => {
    setSelected(on && meta ? new Set(meta.files.map((_, i) => i)) : new Set());
  };

  const magnetLines = parseSourceLines(uriText);
  const validLines = magnetLines.filter((line) => line.valid);
  const invalidLines = magnetLines.filter((line) => !line.valid);

  const baseOptions = {
    savePath,
    label,
    start,
    topOfQueue,
    sequential: false,
    skipHashCheck: skipHash,
  };

  const addMagnets = async () => {
    const failures: string[] = [];
    for (const line of validLines) {
      try {
        await addTorrent(
          { kind: "magnet", uri: line.value },
          { ...baseOptions, skipHashCheck: false, unselectedIndexes: [] },
        );
      } catch (e) {
        failures.push(`${line.value}: ${String(e)}`);
      }
    }
    return failures;
  };

  const addFiles = async () => {
    const failures: string[] = [];
    // Per-file selection describes the first torrent only (see the module note).
    const multi = queue.length > 1;
    const unselectedIndexes = multi
      ? []
      : meta
        ? meta.files.map((_, i) => i).filter((i) => !selected.has(i))
        : [];
    for (const item of queue) {
      try {
        await webUploadTorrent(item.file, {
          ...baseOptions,
          unselectedIndexes,
        });
      } catch (e) {
        failures.push(`${item.name}: ${String(e)}`);
      }
    }
    return failures;
  };

  const canAdd = mode === "magnet" ? validLines.length > 0 : queue.length > 0;

  // Duplicate detection (V3-13): never add silently. The check runs as the form
  // changes, against the library the store already holds.
  const candidate: AddCandidate =
    mode === "file" && meta && queue.length === 1
      ? {
          hash: meta.infoHash,
          name: meta.name,
          size: meta.size,
          destination: savePath,
        }
      : mode === "magnet" && validLines.length === 1
        ? {
            hash: magnetHash(validLines[0].value),
            name: magnetName(validLines[0].value),
            size: null,
            destination: savePath,
          }
        : {};
  const duplicate = detectDuplicate(candidate, torrents);

  const showExisting = (hash: string) => {
    const ui = useUi.getState();
    ui.select(hash);
    ui.setSearch("");
    closeDialog();
  };

  /** Add this torrent's trackers to the existing one (exact duplicates only). */
  const mergeTrackers = () => {
    if (!duplicate || duplicate.kind !== "exact" || !meta) return;
    const existing = duplicate.hash;
    void Promise.allSettled(
      meta.trackers.map((url) => addTracker(existing, url)),
    ).finally(closeDialog);
  };

  const add = async () => {
    // An exact duplicate is never submitted; the buttons beside the warning
    // are the only way forward.
    if (adding || !canAdd || duplicate?.kind === "exact") return;
    setAdding(true);
    setError(null);
    try {
      const failures =
        mode === "magnet" ? await addMagnets() : await addFiles();
      if (failures.length > 0) {
        setError(failures.join("\n"));
        setAdding(false);
        return;
      }
      closeDialog();
    } catch (e) {
      setError(String(e));
      setAdding(false);
    }
  };

  const cancel = () => {
    if (!adding) closeDialog();
  };

  // Keep an external source's kind in step if the dialog was opened for one.
  useEffect(() => {
    const kind = external?.source.kind;
    if (kind === "magnet") setMode("magnet");
    if (kind === "upload") setMode("file");
  }, [external]);

  return (
    <ModalBase
      title="Add torrent"
      width={660}
      onCancel={cancel}
      onPrimary={() => void add()}
      footer={
        <>
          <Button variant="secondary" onClick={cancel} disabled={adding}>
            Cancel
          </Button>
          <Button
            variant="primary"
            onClick={() => void add()}
            disabled={!canAdd || adding || duplicate?.kind === "exact"}
          >
            {adding ? "Adding…" : "Add"}
          </Button>
        </>
      }
    >
      <input
        ref={fileInputRef}
        type="file"
        multiple
        accept=".torrent,application/x-bittorrent"
        style={{ display: "none" }}
        onChange={onFileInputChange}
      />

      <div className={forms.col}>
        <div className={styles.segmented} role="group" aria-label="Add source">
          <button
            type="button"
            className={styles.segment}
            aria-pressed={mode === "magnet"}
            onClick={() => setMode("magnet")}
          >
            Magnet / URL
          </button>
          <button
            type="button"
            className={styles.segment}
            aria-pressed={mode === "file"}
            onClick={() => setMode("file")}
          >
            .torrent file
          </button>
        </div>

        {error && <div className={forms.error}>{error}</div>}

        {duplicate && (
          <div className={styles.duplicate} role="alert">
            {duplicate.kind === "exact" ? (
              <>
                <span>
                  Already in your library as <b>{duplicate.name}</b>.
                </span>
                <span className={styles.duplicateActions}>
                  <button
                    type="button"
                    className={styles.duplicateAction}
                    onClick={() => showExisting(duplicate.hash)}
                  >
                    Show it
                  </button>
                  {mode === "file" && meta && meta.trackers.length > 0 && (
                    <button
                      type="button"
                      className={styles.duplicateAction}
                      onClick={mergeTrackers}
                    >
                      Merge trackers
                    </button>
                  )}
                </span>
              </>
            ) : duplicate.kind === "sameNameAndSize" ? (
              <span>
                Another torrent has the same name and size:{" "}
                <b>{duplicate.name}</b>.
              </span>
            ) : (
              <span>
                The destination folder is already used by{" "}
                <b>{duplicate.name}</b> ({duplicate.path}).
              </span>
            )}
          </div>
        )}

        {mode === "magnet" ? (
          <div style={{ display: "flex", flexDirection: "column", gap: 5 }}>
            <label
              className={forms.fieldLabel}
              style={{ width: "auto" }}
              htmlFor="add-source"
            >
              Magnet URI or torrent URL — one per line
            </label>
            <textarea
              id="add-source"
              className={styles.textarea}
              value={uriText}
              onChange={(e) => setUriText(e.currentTarget.value)}
              placeholder={
                "magnet:?xt=urn:btih:…\nhttps://example.test/a.torrent"
              }
              spellCheck={false}
            />
            {invalidLines.map((line) => (
              <span key={line.value} className={styles.lineError}>
                not a valid magnet or torrent URL: {line.value}
              </span>
            ))}
            <span className={forms.meta}>
              {validLines.length} valid{" "}
              {validLines.length === 1 ? "entry" : "entries"}
            </span>
          </div>
        ) : (
          <>
            <div
              className={styles.dropzone}
              data-drag={dragOver}
              onClick={pickFiles}
              onDragOver={(e) => {
                e.preventDefault();
                setDragOver(true);
              }}
              onDragLeave={() => setDragOver(false)}
              onDrop={onDrop}
              role="button"
              tabIndex={0}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === " ") {
                  e.preventDefault();
                  pickFiles();
                }
              }}
            >
              <span>Drop .torrent files here</span>
              <span>or click to choose</span>
            </div>

            {queue.length > 0 && (
              <div className={styles.queue}>
                {queue.map((item) => (
                  <div className={styles.queueItem} key={item.key}>
                    <span className={styles.queueName}>{item.name}</span>
                    <button
                      type="button"
                      className={styles.remove}
                      aria-label={`Remove ${item.name}`}
                      onClick={() => removeQueued(item.key)}
                    >
                      ×
                    </button>
                  </div>
                ))}
              </div>
            )}

            {meta && queue.length === 1 && (
              <>
                <div className={forms.field}>
                  <span className={forms.fieldLabel}>Torrent</span>
                  <span className={forms.value} title={meta.name}>
                    {meta.name}
                  </span>
                  <span className={forms.meta}>
                    {formatBytes(meta.size)} · {meta.files.length} file
                    {meta.files.length === 1 ? "" : "s"}
                  </span>
                </div>
                <FileTree
                  files={meta.files}
                  selected={selected}
                  expanded={expanded}
                  onToggle={toggleNode}
                  onExpand={(key) =>
                    setExpanded((prev) => {
                      const next = new Set(prev);
                      if (next.has(key)) next.delete(key);
                      else next.add(key);
                      return next;
                    })
                  }
                  onSetAll={setAll}
                />
              </>
            )}
          </>
        )}

        <div className={forms.field}>
          <label className={forms.fieldLabel} htmlFor="add-destination">
            Destination
          </label>
          <input
            id="add-destination"
            className={forms.input}
            value={savePath}
            onChange={(e) => setSavePath(e.currentTarget.value)}
            spellCheck={false}
          />
        </div>

        <div className={forms.field}>
          <label className={forms.fieldLabel} htmlFor="add-label">
            Label
          </label>
          <input
            id="add-label"
            className={forms.input}
            list="add-known-labels"
            value={label}
            onChange={(e) => onLabelChange(e.currentTarget.value)}
            placeholder="(none)"
            spellCheck={false}
          />
          <datalist id="add-known-labels">
            {labels.map((known) => (
              <option key={known} value={known} />
            ))}
          </datalist>
        </div>

        <div className={forms.checkGrid}>
          <Checkbox
            checked={start}
            onChange={setStart}
            label="Start immediately"
          />
          <Checkbox
            checked={skipHash}
            onChange={setSkipHash}
            label="Skip hash check"
          />
          <Checkbox
            checked={topOfQueue}
            onChange={setTopOfQueue}
            label="Add to top of queue"
          />
          <Checkbox
            checked={false}
            onChange={() => {}}
            disabled
            label="Sequential download"
            title="not supported by rtorrent"
          />
        </div>
      </div>
    </ModalBase>
  );
}
