/**
 * The tri-state file tree the add modal shows for a `.torrent` (WC6-S3/S4).
 *
 * Extracted from the old `AddTorrentDialog` so the unified modal and any future
 * caller share one implementation. Pure tree/selection logic stays in
 * `utils/filetree`; this is only the presentation and its header links.
 */

import { useMemo } from "react";
import type { FileNode } from "../../ipc/types";
import { formatBytes } from "../../utils/format";
import {
  buildTree,
  folderState,
  selectedSize,
  type TreeNode,
} from "../../utils/filetree";
import forms from "./forms.module.css";
import styles from "./FileTree.module.css";

interface FileTreeProps {
  files: FileNode[];
  selected: Set<number>;
  expanded: Set<string>;
  onToggle: (node: TreeNode, on: boolean) => void;
  onExpand: (key: string) => void;
  onSetAll: (on: boolean) => void;
}

export function FileTree({
  files,
  selected,
  expanded,
  onToggle,
  onExpand,
  onSetAll,
}: FileTreeProps) {
  const tree = useMemo(() => buildTree(files), [files]);
  const totalSelected = selectedSize(files, selected);

  return (
    <div className={styles.contents}>
      <div className={styles.contentsHeader}>
        <span>Contents · {formatBytes(totalSelected)} selected</span>
        <span className={styles.selectLinks}>
          <a onClick={() => onSetAll(true)}>select all</a> ·{" "}
          <a onClick={() => onSetAll(false)}>none</a>
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
            onToggle={onToggle}
            onExpand={onExpand}
          />
        ))}
      </div>
    </div>
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
