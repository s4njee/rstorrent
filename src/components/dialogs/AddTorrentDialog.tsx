/**
 * Add-torrent dialog (design screen 02).
 *
 * On open it prompts for a `.torrent` file (native picker), parses its metadata
 * in Rust, and shows: the torrent name/size, a save-path (with Browse), a label,
 * download options, and a Contents file tree with tri-state checkboxes. Files
 * left unchecked are passed as `unselectedIndexes` (loaded at priority 0).
 *
 * "Rename torrent" and "Sequential download" are shown disabled — neither is
 * supported by the current backend path; see plan.md non-goals.
 */

import { useEffect, useMemo, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { useUi } from "../../store/ui";
import { useSettings } from "../../store/settings";
import { addTorrent, readTorrentMetadata } from "../../ipc/commands";
import type { TorrentMeta } from "../../ipc/types";
import { formatBytes } from "../../utils/format";
import {
  buildTree,
  folderState,
  leafIndexes,
  selectedSize,
  type TreeNode,
} from "../../utils/filetree";
import { ModalBase, Button } from "./ModalBase";
import { Checkbox } from "./Checkbox";
import forms from "./forms.module.css";
import styles from "./AddTorrentDialog.module.css";

export function AddTorrentDialog() {
  const closeDialog = useUi((s) => s.closeDialog);
  const external = useUi((s) => s.externalAddRequest);
  const settings = useSettings((s) => s.settings);
  const externalPath =
    external?.source.kind === "file" ? external.source.path : null;

  const [path, setPath] = useState<string | null>(null);
  const [meta, setMeta] = useState<TorrentMeta | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [savePath, setSavePath] = useState("");
  const [label, setLabel] = useState("");
  const [start, setStart] = useState(true);
  const [skipHash, setSkipHash] = useState(false);
  const [topOfQueue, setTopOfQueue] = useState(false);
  const [adding, setAdding] = useState(false);
  // Selected file indexes (all selected initially).
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [expanded, setExpanded] = useState<Set<string>>(new Set());

  // Apply the configured destination even if settings finish loading after us.
  useEffect(() => {
    if (settings) setSavePath(settings.defaultSavePath);
  }, [settings]);

  // Finder-provided files skip the picker and go straight to metadata parsing.
  const started = useRef(false);
  useEffect(() => {
    if (started.current) return;
    started.current = true;
    void (async () => {
      const chosen =
        externalPath ??
        (await open({
          multiple: false,
          filters: [{ name: "Torrent", extensions: ["torrent"] }],
        }));
      if (typeof chosen !== "string") {
        closeDialog();
        return;
      }
      setPath(chosen);
      try {
        const m = await readTorrentMetadata(chosen);
        setMeta(m);
        setLabel("");
        setSelected(new Set(m.files.map((_, i) => i)));
        // Expand top-level folders by default.
        setExpanded(
          new Set(
            buildTree(m.files)
              .filter((n) => n.isDir)
              .map((n) => n.name),
          ),
        );
      } catch (e) {
        setError(String(e));
      }
    })();
  }, [closeDialog, externalPath]);

  const tree = useMemo(() => (meta ? buildTree(meta.files) : []), [meta]);

  const browse = async () => {
    const dir = await open({ directory: true });
    if (typeof dir === "string") setSavePath(dir);
  };

  // C11: when the typed label exactly matches a configured default, pre-fill the
  // save path from it. Only on an exact match, so it never fights free typing.
  const onLabelChange = (value: string) => {
    setLabel(value);
    const preset = settings?.labelDefaults.find(
      (d) => d.label === value && d.savePath,
    );
    if (preset) setSavePath(preset.savePath);
  };

  /** Toggle every leaf under a node on/off together. */
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

  const add = async () => {
    if (adding || !path || !meta) return;
    const unselectedIndexes = meta.files
      .map((_, i) => i)
      .filter((i) => !selected.has(i));
    setAdding(true);
    setError(null);
    try {
      await addTorrent(
        { kind: "file", path },
        {
          savePath,
          label,
          start,
          topOfQueue,
          sequential: false,
          skipHashCheck: skipHash,
          unselectedIndexes,
        },
      );
      closeDialog();
    } catch (e) {
      setError(String(e));
      setAdding(false);
    }
  };

  const totalSelected = meta ? selectedSize(meta.files, selected) : 0;
  const cancel = () => {
    if (!adding) closeDialog();
  };

  return (
    <ModalBase
      title="Add torrent"
      width={620}
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
            disabled={!meta || adding}
          >
            {adding ? "Adding…" : "Add"}
          </Button>
        </>
      }
    >
      {error && <div className={forms.error}>{error}</div>}
      {!meta && !error && <div className={forms.meta}>reading torrent…</div>}

      {meta && (
        <div className={forms.col}>
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

          <div className={forms.field}>
            <span className={forms.fieldLabel}>Save to</span>
            <input
              className={forms.input}
              value={savePath}
              onChange={(e) => setSavePath(e.currentTarget.value)}
              spellCheck={false}
            />
            <button className={forms.browse} onClick={browse}>
              Browse…
            </button>
          </div>

          <div className={forms.field}>
            <span className={forms.fieldLabel}>Label</span>
            <input
              className={forms.input}
              value={label}
              onChange={(e) => onLabelChange(e.currentTarget.value)}
              placeholder="(none)"
              spellCheck={false}
            />
            <Checkbox
              checked={false}
              onChange={() => {}}
              disabled
              label="Rename torrent"
              title="not supported yet"
            />
          </div>

          <div className={forms.checkGrid}>
            <Checkbox
              checked={start}
              onChange={setStart}
              label="Start torrent"
            />
            <Checkbox
              checked={false}
              onChange={() => {}}
              disabled
              label="Sequential download"
              title="not supported by rtorrent"
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
          </div>

          <div className={styles.contents}>
            <div className={styles.contentsHeader}>
              <span>Contents · {formatBytes(totalSelected)} selected</span>
              <span className={styles.selectLinks}>
                <a onClick={() => setAll(true)}>select all</a> ·{" "}
                <a onClick={() => setAll(false)}>none</a>
              </span>
            </div>
            <div className={styles.tree}>
              {tree.map((node) => (
                <TreeRow
                  key={node.name}
                  node={node}
                  depth={0}
                  selected={selected}
                  expanded={expanded}
                  onToggle={toggleNode}
                  onExpand={(name) =>
                    setExpanded((prev) => {
                      const next = new Set(prev);
                      if (next.has(name)) next.delete(name);
                      else next.add(name);
                      return next;
                    })
                  }
                />
              ))}
            </div>
          </div>
        </div>
      )}
    </ModalBase>
  );
}

/** One row of the file tree, recursing into expanded folders. */
function TreeRow({
  node,
  depth,
  selected,
  expanded,
  onToggle,
  onExpand,
  path = "",
}: {
  node: TreeNode;
  depth: number;
  selected: Set<number>;
  expanded: Set<string>;
  onToggle: (node: TreeNode, on: boolean) => void;
  onExpand: (key: string) => void;
  path?: string;
}) {
  const key = `${path}/${node.name}`;
  const isOpen = expanded.has(node.name) || expanded.has(key);
  const state = node.isDir
    ? folderState(node, selected)
    : selected.has(node.fileIndex!)
      ? "checked"
      : "unchecked";
  const checked = state === "checked";
  const mark = state === "checked" ? "✓" : state === "indeterminate" ? "–" : "";

  return (
    <>
      <div className={styles.treeRow} style={{ paddingLeft: 10 + depth * 20 }}>
        <span
          className={`${forms.box} ${checked ? forms.checked : ""}`}
          onClick={() => onToggle(node, state !== "checked")}
        >
          {mark}
        </span>
        {node.isDir ? (
          <span className={styles.twisty} onClick={() => onExpand(key)}>
            {isOpen ? "▾" : "▸"}
          </span>
        ) : (
          <span className={styles.twisty} />
        )}
        <span className={styles.name}>{node.name}</span>
        <span className={styles.size}>{formatBytes(node.size)}</span>
      </div>
      {node.isDir &&
        isOpen &&
        node.children.map((child) => (
          <TreeRow
            key={child.name}
            node={child}
            depth={depth + 1}
            selected={selected}
            expanded={expanded}
            onToggle={onToggle}
            onExpand={onExpand}
            path={key}
          />
        ))}
    </>
  );
}
