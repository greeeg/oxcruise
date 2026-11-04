import type { Model } from "../data/model";
import { el } from "../ui/ui";

export interface TreeActions {
  /** A click or Enter on a row: show the folder's map, or the module's neighbourhood. */
  onFocus(node: number): void;
  /** An arrow key moved to a row: show it in the panel, without changing the picture. */
  onSelect(node: number): void;
  /** Hide the node, or show it again when it is hidden. */
  onHide(node: number): void;
}

export interface TreeState {
  hidden: ReadonlySet<number>;
  /** The folder of the map, or the module of the neighbourhood: its folders are open. */
  focus?: number;
  selected?: number;
}

/** A row of the tree as the keys see it. */
export interface TreeRow {
  node: number;
  /** The folder above the row; the root is 0, which is never a row. */
  parent: number;
  depth: number;
  isDir: boolean;
  open: boolean;
}

/** What a key does in the tree: select a row, open or close a folder, or navigate to a row. */
export type TreeMove = { select: number } | { toggle: number } | { open: number } | null;

/**
 * The keys of a tree view (the WAI-ARIA pattern): Up and Down walk the rows, Right opens a folder and then
 * enters it, Left closes it and then goes to its parent, Home and End jump, Enter navigates.
 * With no selected row, Down and Home select the first row and Up and End the last.
 */
export function treeKey(
  rows: readonly TreeRow[],
  selected: number | undefined,
  key: string,
): TreeMove {
  if (rows.length === 0) return null;
  const at = selected === undefined ? -1 : rows.findIndex((r) => r.node === selected);
  const row = at >= 0 ? rows[at] : undefined;
  const pick = (i: number): TreeMove => ({
    select: rows[Math.max(0, Math.min(rows.length - 1, i))].node,
  });
  switch (key) {
    case "ArrowDown":
      return pick(at + 1);
    case "ArrowUp":
      return pick(at < 0 ? rows.length - 1 : at - 1);
    case "Home":
      return pick(0);
    case "End":
      return pick(rows.length - 1);
    case "ArrowRight":
      if (!row?.isDir) return null;
      if (!row.open) return { toggle: row.node };
      return rows[at + 1]?.parent === row.node ? pick(at + 1) : null;
    case "ArrowLeft": {
      if (!row) return null;
      if (row.isDir && row.open) return { toggle: row.node };
      const parent = rows.find((r) => r.node === row.parent);
      return parent ? { select: parent.node } : null;
    }
    case "Enter":
      return row ? { open: row.node } : null;
  }
  return null;
}

const KEYS = new Set(["ArrowDown", "ArrowUp", "ArrowLeft", "ArrowRight", "Home", "End", "Enter"]);

/** Modules listed per folder: more would make a huge page for little use. */
const MODULE_LIMIT = 200;

/**
 * The folders as a tree that navigates: a click on a row shows that folder. The folders around the focus
 * are open; the chevrons open and close others for as long as the focus stays. Closed folders are not
 * listed below, so it stays small.
 */
export class FolderTree {
  readonly root = el("div", { class: "tree", attrs: { role: "tree" } });
  private shownSelected: number | undefined;
  private state: TreeState | undefined;
  private readonly opened = new Set<number>();
  private readonly closed = new Set<number>();
  private forced = new Set<number>();
  private rows: TreeRow[] = [];

  constructor(
    private readonly model: Model,
    private readonly actions: TreeActions,
  ) {
    // the tree itself has the focus and points at a row, because a repaint replaces the rows
    this.root.tabIndex = 0;
    this.root.addEventListener("keydown", (e) => this.onKey(e));
  }

  private onKey(e: KeyboardEvent): void {
    if (e.ctrlKey || e.metaKey || e.altKey || e.shiftKey) return;
    if (!KEYS.has(e.key)) return;
    // the page also reads the arrows (Left goes up one level) and Enter
    e.preventDefault();
    e.stopPropagation();
    const move = treeKey(this.rows, this.state?.selected, e.key);
    if (!move) return;
    if ("toggle" in move) this.toggle(move.toggle);
    else if ("select" in move) this.actions.onSelect(move.select);
    else this.actions.onFocus(move.open);
  }

  update(state: TreeState): void {
    if (state.focus !== this.state?.focus) {
      this.closed.clear();
      this.forced = new Set(state.focus === undefined ? [] : this.model.ancestors(state.focus));
      if (state.focus !== undefined && this.model.isDir(state.focus)) this.forced.add(state.focus);
    }
    this.state = state;
    this.paint();
  }

  /** Close every folder, including the ones around the focus. */
  collapseAll(): void {
    this.opened.clear();
    for (const d of this.forced) this.closed.add(d);
    this.paint();
  }

  private isOpen(d: number): boolean {
    return !this.closed.has(d) && (this.forced.has(d) || this.opened.has(d));
  }

  private toggle(d: number): void {
    if (this.isOpen(d)) {
      this.closed.add(d);
      this.opened.delete(d);
    } else {
      this.closed.delete(d);
      this.opened.add(d);
    }
    this.paint();
  }

  private paint(): void {
    const { model } = this;
    const state = this.state!;
    const rows: HTMLElement[] = [];
    const keyRows: TreeRow[] = [];
    let selectedRow: HTMLElement | undefined;
    const add = (node: number, depth: number, hidden: boolean) => {
      const isDir = model.isDir(node);
      const isHidden = hidden || state.hidden.has(node);
      const open = isDir && this.isOpen(node);
      const errors = isDir ? model.dirErrors[node] : model.modErrors[model.modOf(node)];
      const attrs: Record<string, string> = {
        id: `tree-row-${node}`,
        role: "treeitem",
        "aria-level": String(depth + 1),
        "aria-selected": String(node === state.selected),
        style: `padding-left:${6 + depth * 14}px`,
      };
      if (isDir) attrs["aria-expanded"] = String(open);
      const row = el(
        "div",
        {
          class: `tree-row${node === state.selected ? " selected" : ""}${isHidden ? " is-hidden" : ""}${node === state.focus ? " is-focus" : ""}`,
          attrs,
        },
        [
          el("span", { class: "tree-chevron", text: isDir ? (open ? "▾" : "▸") : "" }),
          el("span", { class: "tree-name", text: model.name(node), title: model.path(node) }),
          ...(isDir
            ? [el("span", { class: "tree-count", text: String(model.dirModules[node]) })]
            : []),
          ...(errors ? [el("span", { class: "dot sev-error", title: `${errors} errors` })] : []),
          this.button("⊘", state.hidden.has(node) ? "Show" : "Hide", () =>
            this.actions.onHide(node),
          ),
        ],
      );
      row.addEventListener("click", () => this.actions.onFocus(node));
      if (isDir) {
        row.querySelector(".tree-chevron")!.addEventListener("click", (e) => {
          e.stopPropagation();
          this.toggle(node);
        });
      }
      if (node === state.selected) selectedRow = row;
      rows.push(row);
      keyRows.push({ node, parent: model.parent(node), depth, isDir, open });
      if (open) children(node, depth + 1, isHidden);
    };
    const children = (d: number, depth: number, hidden: boolean) => {
      for (const c of model.childDirs[d]) add(c, depth, hidden);
      const mods = model.childMods[d];
      for (const m of mods.slice(0, MODULE_LIMIT)) add(model.nodeOfMod(m), depth, hidden);
      if (mods.length > MODULE_LIMIT) {
        rows.push(
          el("div", {
            class: "tree-row muted",
            text: `and ${mods.length - MODULE_LIMIT} more modules`,
            attrs: { style: `padding-left:${6 + depth * 14}px` },
          }),
        );
      }
    };
    children(0, 0, false);
    this.root.replaceChildren(...rows);
    this.rows = keyRows;
    if (selectedRow) this.root.setAttribute("aria-activedescendant", selectedRow.id);
    else this.root.removeAttribute("aria-activedescendant");
    if (selectedRow && state.selected !== this.shownSelected) {
      selectedRow.scrollIntoView({ block: "nearest" });
    }
    this.shownSelected = state.selected;
  }

  private button(text: string, title: string, run: () => void): HTMLElement {
    const b = el("button", { class: "tree-action", text, title });
    b.addEventListener("click", (e) => {
      e.stopPropagation();
      run();
    });
    return b;
  }
}
