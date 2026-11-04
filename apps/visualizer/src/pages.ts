import { MapView } from "./mapview";
import { mapScope, resolveFolder } from "./map";
import type { MapScope } from "./map";
import type { Direction } from "./logic";
import type { Model } from "./model";
import type { Nav } from "./nav";
import { NeighbourhoodView } from "./nbview";
import { layoutNeighbourhood } from "./neighbourhood";
import type { Neighbourhood } from "./neighbourhood";
import { scopeInfo } from "./scope";
import { FolderTree } from "./tree";
import { el, icon, Tooltip } from "./ui";

const HELP: [string, string][] = [
  ["Click", "a box on the map: open it. A module in a neighbourhood, or a line: select it."],
  ["Shift-click", "a box on the map: select it without opening it."],
  ["Double-click", "a module in a neighbourhood: put it in the middle."],
  ["Enter", "open the selection."],
  ["Esc, Backspace, ←", "go up one level."],
  ["F", "focus the selection, like a double click."],
  ["H", "hide the selection, or show it again."],
  ["Drag, wheel", "pan and zoom the picture."],
  ["Ctrl K, ⌘K, /", "search for a file or a folder."],
  [
    "Tab, ↑ ↓ → ←",
    "in the tree: Tab goes to it, ↑ ↓ select a row, → opens a folder, ← closes it or selects its parent.",
  ],
  ["Home, End, Enter", "in the tree: first row, last row, open the row."],
];

/** What the panel lists for a pseudo-box or a folded row. */
export interface Members {
  title: string;
  nodes: number[];
}

/**
 * The dependency graph: a folder tree, the picture, and the toolbar that limits it.
 * A folder shows the map of its children; a module shows what it imports and what imports it.
 */
export class GraphPage {
  readonly root = el("div", { class: "graph-page" });
  private readonly crumbs = el("div", { class: "crumbs" });
  private readonly info = el("span", { class: "muted small" });
  private readonly hops = el(
    "select",
    { class: "toolbar-select", title: "Hops" },
    [1, 2, 3].map((n) =>
      el("option", { text: `${n} hop${n > 1 ? "s" : ""}`, attrs: { value: String(n) } }),
    ),
  );
  private readonly dir = el("select", { class: "toolbar-select", title: "Direction" }, [
    el("option", { text: "Imports and importers", attrs: { value: "both" } }),
    el("option", { text: "Imports only", attrs: { value: "imports" } }),
    el("option", { text: "Importers only", attrs: { value: "importers" } }),
  ]);
  private readonly packages = el("button", {
    class: "btn toggle",
    text: "Packages",
    title: "Show the packages and built-in modules a folder uses",
  });
  private readonly types = el("button", {
    class: "btn toggle",
    text: "Type-only",
    title: "Count the type-only dependencies",
  });
  private readonly hiddenChip = el("button", { class: "btn" });
  private readonly weakChip = el("button", { class: "btn" });
  private readonly help = el("div", { class: "help-pop" }, [
    el("h3", { text: "Keys and mouse" }),
    ...HELP.map(([k, v]) => el("div", { class: "kv" }, [el("b", { class: "k", text: k }), v])),
    el("h3", { text: "Map" }),
    el("div", {
      text: "What is imported is on top, the modules that import it below. One line joins a pair of boxes, with an arrow to the imported one.",
    }),
    el("div", { text: "A thick line has more imports. A dashed line has only type-only imports." }),
    el("div", { text: "Red: part of a cycle, or an error. Orange: warnings only." }),
    el("div", { text: "A red frame groups folders that import each other." }),
    el("div", {
      text: "Pills above the frame are folders that the map imports. Pills below it are folders that import into it.",
    }),
    el("h3", { text: "Neighbourhood" }),
    el("div", {
      text: "The module is in the middle. Importers are on the left, imports on the right, by hops.",
    }),
    el("div", {
      text: "A red ring is a module that imports the middle module, and is imported by it too.",
    }),
  ]);
  private readonly tree: FolderTree;
  private readonly map: MapView;
  private readonly nbView: NeighbourhoodView;
  private active: "map" | "nb" = "map";
  private lastKey = "";
  private shownFocus: string | undefined | null = null;
  private showWeak = false;
  private lastMap: MapScope | null = null;
  private lastNb: Neighbourhood | null = null;
  /** The folder or module the picture shows, after skipping folders that hold one folder. */
  private current: number | undefined;

  constructor(
    host: HTMLElement,
    private readonly model: Model,
    private readonly nav: Nav,
    tip: Tooltip,
  ) {
    const change = () =>
      nav.go({ hops: Number(this.hops.value), dir: this.dir.value as Direction }, true);
    this.hops.addEventListener("change", change);
    this.dir.addEventListener("change", change);
    this.packages.addEventListener("click", () => nav.go({ pkgs: !nav.state().pkgs }, true));
    this.types.addEventListener("click", () => nav.go({ types: !nav.state().types }, true));
    this.hiddenChip.addEventListener("click", () => nav.go({ hide: undefined }, true));
    this.weakChip.addEventListener("click", () => {
      this.showWeak = !this.showWeak;
      this.show();
    });
    const fit = el("button", { class: "btn", text: "Fit" });
    fit.addEventListener("click", () => this.fit());
    const helpButton = el("button", { class: "btn", text: "?", title: "Keys and mouse" });
    helpButton.addEventListener("click", () => (this.help.hidden = !this.help.hidden));
    this.help.hidden = true;

    const select = (node: number | null, box?: string) =>
      nav.go({ sel: node === null ? undefined : model.path(node), link: undefined, box }, true);
    const selectLink = (s: number, t: number) =>
      nav.go({ link: `${model.path(s)}>${model.path(t)}`, sel: undefined, box: undefined }, true);

    const canvasHost = el("div", { class: "graph-host" });
    this.tree = new FolderTree(model, {
      onFocus: (n) => nav.focus(n),
      onSelect: (n) => select(n),
      onHide: (n) => this.hide(n),
    });
    const collapse = el(
      "button",
      {
        class: "tree-action",
        title: "Collapse all folders",
        attrs: { "aria-label": "Collapse all folders" },
      },
      [icon("collapse")],
    );
    collapse.addEventListener("click", () => this.tree.collapseAll());
    const treePane = el("div", { class: "tree-pane" }, [
      el("div", { class: "tree-head" }, [el("span", { text: "Folders" }), collapse]),
      this.tree.root,
    ]);
    const grip = el("div", { class: "tree-grip", title: "Drag to resize" });
    grip.addEventListener("pointerdown", (e) => {
      grip.setPointerCapture(e.pointerId);
      const move = (m: PointerEvent) => {
        const left = treePane.getBoundingClientRect().left;
        treePane.style.width = `${Math.max(140, Math.min(600, m.clientX - left))}px`;
      };
      const up = () => {
        grip.removeEventListener("pointermove", move);
        grip.removeEventListener("pointerup", up);
      };
      grip.addEventListener("pointermove", move);
      grip.addEventListener("pointerup", up);
    });

    this.root.append(
      el("div", { class: "toolbar" }, [
        this.crumbs,
        this.hops,
        this.dir,
        this.packages,
        this.types,
        this.weakChip,
        this.hiddenChip,
        this.info,
        fit,
        helpButton,
      ]),
      this.help,
      el("div", { class: "graph-body" }, [treePane, grip, canvasHost]),
    );
    host.append(this.root);

    this.map = new MapView(
      canvasHost,
      model,
      {
        onSelect: select,
        onSelectLink: selectLink,
        onDrill: (n) => nav.focus(n),
        onFocus: (n) => nav.focus(n),
      },
      tip,
    );
    this.nbView = new NeighbourhoodView(
      canvasHost,
      model,
      {
        onSelect: select,
        onSelectLink: selectLink,
        onRecentre: (n) => nav.focus(n),
        onOpenFolder: (d) => nav.focus(d),
      },
      tip,
    );
  }

  show(): void {
    const { model, nav } = this;
    const s = nav.state();
    const info = scopeInfo(model, s);
    const hidden = new Set(s.hide ?? []);
    const moduleScope = info.focus !== undefined && !model.isDir(info.focus);
    this.active = moduleScope ? "nb" : "map";
    this.map.root.hidden = moduleScope;
    this.nbView.root.hidden = !moduleScope;

    // a new focus is a new picture: fit it, and forget the weak links the user asked for
    const refit = this.shownFocus !== s.focus;
    if (refit) this.showWeak = false;
    this.shownFocus = s.focus;

    const sel = s.sel ? model.nodeByPath(s.sel) : -1;
    const selected = sel >= 0 ? sel : undefined;
    let selectedLink: { s: number; t: number } | undefined;
    if (s.link) {
      const [a, b] = s.link.split(">").map((p) => model.nodeByPath(p));
      if (a >= 0 && b >= 0) selectedLink = { s: a, t: b };
    }
    const paint = { selected, selectedBox: s.box, selectedLink };
    const hide = (s.hide ?? []).join(",");

    if (moduleScope) {
      const m = model.modOf(info.focus!);
      const key = ["nb", m, s.hops, s.dir, s.types, s.pkgs, hide, info.cycle.join(",")].join("|");
      if (key !== this.lastKey || !this.lastNb) {
        this.lastNb = layoutNeighbourhood(model, m, {
          hops: s.hops,
          dir: s.dir,
          types: s.types,
          pkgs: s.pkgs,
          hidden,
          cycle: info.cycle,
        });
      }
      this.lastKey = key;
      this.nbView.show(this.lastNb, { ...paint, key, refit });
      this.current = info.focus;
      const modules = this.lastNb.items.filter((i) => i.module !== undefined).length;
      this.info.textContent = `${modules} modules · ${this.lastNb.edges.length} lines`;
      this.weakChip.hidden = true;
    } else {
      const requested = info.focus ?? 0;
      const key = ["map", requested, s.types, s.pkgs, hide].join("|");
      if (key !== this.lastKey || !this.lastMap) {
        this.lastMap = mapScope(model, requested, { hidden, types: s.types, pkgs: s.pkgs });
      }
      this.lastKey = key;
      this.map.show(this.lastMap, {
        ...paint,
        key: `${key}|${this.showWeak}`,
        refit,
        showWeak: this.showWeak,
      });
      this.current = this.lastMap.folder;
      this.info.textContent = `${this.lastMap.boxes.length} boxes · ${this.lastMap.links.length} links`;
      const weak = this.map.weakHidden;
      this.weakChip.hidden = weak === 0;
      this.weakChip.textContent = this.showWeak
        ? "Show only the strong links"
        : `${weak} weak link${weak === 1 ? "" : "s"} hidden · show`;
    }

    this.tree.update({
      hidden,
      focus: this.current !== undefined && this.current > 0 ? this.current : undefined,
      selected,
    });

    // toolbar
    this.hops.hidden = this.dir.hidden = !moduleScope;
    this.hops.value = String(s.hops);
    this.dir.value = s.dir;
    this.packages.classList.toggle("on", s.pkgs);
    this.types.classList.toggle("on", s.types);
    this.hiddenChip.hidden = hidden.size === 0;
    this.hiddenChip.textContent = `${hidden.size} hidden · show all`;
    this.paintCrumbs(this.current, info.cycle.length);
  }

  fit(): void {
    (this.active === "nb" ? this.nbView : this.map).fit();
  }

  /** The nodes behind a pseudo-box or a folded row of the picture, for the side panel. */
  members(id: string): Members | undefined {
    return (this.active === "nb" ? this.nbView : this.map).members(id);
  }

  /** Keys of the page. Returns true when it used the key. */
  onKey(e: KeyboardEvent): boolean {
    if (e.ctrlKey || e.metaKey || e.altKey) return false;
    const { model, nav } = this;
    const s = nav.state();
    const sel = s.sel ? model.nodeByPath(s.sel) : -1;
    switch (e.key) {
      case "Escape":
      case "Backspace":
      case "ArrowLeft":
        if (e.key === "Escape" && !this.help.hidden) {
          this.help.hidden = true;
          return true;
        }
        return this.up();
      case "f":
      case "F":
      case "Enter":
        if (sel > 0) nav.focus(sel);
        return sel > 0;
      case "h":
      case "H":
        if (sel > 0) this.hide(sel);
        return sel > 0;
      case "?":
        this.help.hidden = !this.help.hidden;
        return true;
    }
    return false;
  }

  /** One level out: the folder of a module, or the nearest folder above the map that shows something else. */
  private up(): boolean {
    const { model, nav } = this;
    const hidden = new Set(nav.state().hide ?? []);
    const node = this.current;
    if (node === undefined || node === 0) return false;
    let p = model.parent(node);
    if (model.isDir(node)) {
      // folders that hold only this folder would draw the same map again
      while (p > 0 && resolveFolder(model, p, hidden) === node) p = model.parent(p);
      if (p === 0 && resolveFolder(model, 0, hidden) === node) return false;
    }
    nav.focus(p > 0 ? p : null);
    return true;
  }

  private hide(n: number): void {
    const hide = new Set(this.nav.state().hide ?? []);
    if (!hide.delete(n)) hide.add(n);
    this.nav.go({ hide: hide.size ? [...hide] : undefined, sel: undefined, box: undefined }, true);
  }

  private paintCrumbs(focus: number | undefined, cycle: number): void {
    const { model, nav } = this;
    const chain =
      focus === undefined || focus === 0
        ? []
        : [...model.ancestors(focus).reverse().slice(1), focus];
    const crumb = (label: string, node: number | null, current: boolean) => {
      const b = el("button", { class: "crumb", text: label, title: "Show this level" });
      b.disabled = current;
      b.addEventListener("click", () => nav.focus(node));
      return b;
    };
    const parts: (Node | string)[] = [crumb("project", null, focus === undefined || focus === 0)];
    for (const n of chain) {
      parts.push(
        el("span", { class: "crumb-sep", text: "›" }),
        crumb(model.name(n), n, n === focus),
      );
    }
    if (cycle) parts.push(el("span", { class: "muted small", text: ` · cycle of ${cycle}` }));
    this.crumbs.replaceChildren(...parts);
  }
}
