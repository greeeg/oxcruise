import { describe, expect, it } from "vitest";
import { treeKey } from "./tree";
import type { TreeRow } from "./tree";

// src (open)           node 3
//   ui (closed)        node 4
//   a.ts               node 10
// lib (closed)         node 5
// README.md            node 11
const rows: TreeRow[] = [
  { node: 3, parent: 0, depth: 0, isDir: true, open: true },
  { node: 4, parent: 3, depth: 1, isDir: true, open: false },
  { node: 10, parent: 3, depth: 1, isDir: false, open: false },
  { node: 5, parent: 0, depth: 0, isDir: true, open: false },
  { node: 11, parent: 0, depth: 0, isDir: false, open: false },
];

describe("treeKey", () => {
  it("walks the rows and stops at the ends", () => {
    expect(treeKey(rows, 3, "ArrowDown")).toEqual({ select: 4 });
    expect(treeKey(rows, 11, "ArrowDown")).toEqual({ select: 11 });
    expect(treeKey(rows, 4, "ArrowUp")).toEqual({ select: 3 });
    expect(treeKey(rows, 3, "ArrowUp")).toEqual({ select: 3 });
    expect(treeKey(rows, 10, "Home")).toEqual({ select: 3 });
    expect(treeKey(rows, 10, "End")).toEqual({ select: 11 });
  });

  it("starts from the ends when nothing is selected, or the selection is not listed", () => {
    expect(treeKey(rows, undefined, "ArrowDown")).toEqual({ select: 3 });
    expect(treeKey(rows, undefined, "ArrowUp")).toEqual({ select: 11 });
    expect(treeKey(rows, 99, "ArrowDown")).toEqual({ select: 3 });
  });

  it("opens a closed folder with Right, then enters it", () => {
    expect(treeKey(rows, 4, "ArrowRight")).toEqual({ toggle: 4 });
    expect(treeKey(rows, 3, "ArrowRight")).toEqual({ select: 4 });
    expect(treeKey(rows, 10, "ArrowRight")).toBeNull();
  });

  it("does not enter an open folder that lists nothing", () => {
    const empty: TreeRow[] = [
      { node: 3, parent: 0, depth: 0, isDir: true, open: true },
      { node: 5, parent: 0, depth: 0, isDir: true, open: false },
    ];
    expect(treeKey(empty, 3, "ArrowRight")).toBeNull();
  });

  it("closes an open folder with Left, then goes to the parent", () => {
    expect(treeKey(rows, 3, "ArrowLeft")).toEqual({ toggle: 3 });
    expect(treeKey(rows, 10, "ArrowLeft")).toEqual({ select: 3 });
    expect(treeKey(rows, 4, "ArrowLeft")).toEqual({ select: 3 });
    // the parent is the root, which is no row
    expect(treeKey(rows, 11, "ArrowLeft")).toBeNull();
    expect(treeKey(rows, 5, "ArrowLeft")).toBeNull();
  });

  it("opens the row with Enter, and ignores other keys", () => {
    expect(treeKey(rows, 10, "Enter")).toEqual({ open: 10 });
    expect(treeKey(rows, undefined, "Enter")).toBeNull();
    expect(treeKey(rows, 10, "x")).toBeNull();
    expect(treeKey([], undefined, "ArrowDown")).toBeNull();
  });
});
