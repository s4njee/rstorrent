// Tests for the Add-dialog file-tree logic: tree building, folder tri-state,
// and selected-size accounting.
import { describe, it, expect } from "vitest";
import { buildTree, folderState, leafIndexes, selectedSize } from "./filetree";
import type { FileNode } from "../ipc/types";

function f(path: string, size: number): FileNode {
  return { path, size, priority: 1, progress: 0, isDir: false };
}

const files: FileNode[] = [
  f("Fedora/Live.iso", 2000),
  f("Fedora/CHECKSUM", 100),
  f("Fedora/docs/README.txt", 50),
];

describe("buildTree", () => {
  it("nests folders and rolls up sizes", () => {
    const tree = buildTree(files);
    expect(tree).toHaveLength(1);
    const fedora = tree[0];
    expect(fedora.name).toBe("Fedora");
    expect(fedora.isDir).toBe(true);
    expect(fedora.size).toBe(2150); // 2000 + 100 + 50
    // Two files + one subfolder.
    expect(fedora.children.map((c) => c.name).sort()).toEqual([
      "CHECKSUM",
      "Live.iso",
      "docs",
    ]);
  });
  it("assigns leaf file indexes", () => {
    const tree = buildTree(files);
    expect(leafIndexes(tree[0]).sort()).toEqual([0, 1, 2]);
  });
});

describe("folderState (tri-state)", () => {
  const tree = buildTree(files);
  it("checked when all descendants selected", () => {
    expect(folderState(tree[0], new Set([0, 1, 2]))).toBe("checked");
  });
  it("unchecked when none selected", () => {
    expect(folderState(tree[0], new Set())).toBe("unchecked");
  });
  it("indeterminate when some selected", () => {
    expect(folderState(tree[0], new Set([0]))).toBe("indeterminate");
  });
});

describe("selectedSize", () => {
  it("sums only selected leaves", () => {
    expect(selectedSize(files, new Set([0, 1]))).toBe(2100);
    expect(selectedSize(files, new Set())).toBe(0);
  });
});
