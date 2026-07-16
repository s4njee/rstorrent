/**
 * Build a nested folder tree from a torrent's flat file list, and the helpers
 * the Add-dialog tree needs for tri-state selection. Pure and unit-tested
 * (filetree.test.ts) so the folder/child checkbox logic is verifiable.
 */

import type { FileNode } from "../ipc/types";

export interface TreeNode {
  name: string;
  isDir: boolean;
  /** Total size of this node (leaf size, or sum of descendants for a folder). */
  size: number;
  /** Index into the original flat file array; only set on leaves. */
  fileIndex?: number;
  children: TreeNode[];
}

/** Build a tree from file paths like "Folder/sub/file.iso". */
export function buildTree(files: FileNode[]): TreeNode[] {
  const root: TreeNode = { name: "", isDir: true, size: 0, children: [] };

  files.forEach((file, fileIndex) => {
    const parts = file.path.split("/").filter(Boolean);
    let node = root;
    parts.forEach((part, depth) => {
      const isLeaf = depth === parts.length - 1;
      let child = node.children.find(
        (c) => c.name === part && c.isDir === !isLeaf,
      );
      if (!child) {
        child = {
          name: part,
          isDir: !isLeaf,
          size: isLeaf ? file.size : 0,
          fileIndex: isLeaf ? fileIndex : undefined,
          children: [],
        };
        node.children.push(child);
      }
      node = child;
    });
  });

  // Roll folder sizes up from their descendants.
  const sumSizes = (n: TreeNode): number => {
    if (!n.isDir) return n.size;
    n.size = n.children.reduce((s, c) => s + sumSizes(c), 0);
    return n.size;
  };
  root.children.forEach(sumSizes);
  return root.children;
}

/** All leaf file indexes under a node (a single index for a leaf). */
export function leafIndexes(node: TreeNode): number[] {
  if (!node.isDir) return node.fileIndex != null ? [node.fileIndex] : [];
  return node.children.flatMap(leafIndexes);
}

export type TriState = "checked" | "unchecked" | "indeterminate";

/** A folder's checkbox state given the set of selected leaf indexes. */
export function folderState(node: TreeNode, selected: Set<number>): TriState {
  const idxs = leafIndexes(node);
  if (idxs.length === 0) return "unchecked";
  const on = idxs.filter((i) => selected.has(i)).length;
  if (on === 0) return "unchecked";
  if (on === idxs.length) return "checked";
  return "indeterminate";
}

/** Total size of the currently-selected leaves. */
export function selectedSize(files: FileNode[], selected: Set<number>): number {
  return files.reduce((sum, f, i) => (selected.has(i) ? sum + f.size : sum), 0);
}
