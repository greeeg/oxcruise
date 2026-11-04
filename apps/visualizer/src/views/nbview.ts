import type { Model } from "../data/model";
import type { Neighbourhood, NbEdge } from "../graph/neighbourhood";
import { arrowFor, ENTER_CLASS, pinHover, setHover, svgEl, svgText, Viewport } from "../ui/svgview";
import type { Tooltip } from "../ui/ui";

export interface NbCallbacks {
  /** A click on a module row (a node), a folded row (its id), or the background (`null`). */
  onSelect(node: number | null, box?: string): void;
  onSelectLink(s: number, t: number): void;
  /** A double click on a module: put it in the middle. */
  onRecentre(node: number): void;
  /** A double click on a folder header: the map of that folder. */
  onOpenFolder(dir: number): void;
}

export interface NbPaint {
  selected?: number;
  selectedBox?: string;
  selectedLink?: { s: number; t: number };
}

export interface NbShow extends NbPaint {
  key: string;
  refit: boolean;
}

const TOP = 30;

/** A module in the middle, its importers in columns to the left and its imports to the right. */
export class NeighbourhoodView {
  readonly root = document.createElement("div");
  private readonly viewport: Viewport;
  private nb: Neighbourhood | null = null;
  private lastKey: string | null = null;
  private itemEls: SVGGElement[] = [];
  private scene: SVGGElement | null = null;
  private touch = new Map<number, Element[]>();
  private edgeEls: SVGGElement[] = [];

  constructor(
    host: HTMLElement,
    private readonly model: Model,
    private readonly cb: NbCallbacks,
    private readonly tip: Tooltip,
  ) {
    this.root.className = "map-host";
    host.append(this.root);
    this.viewport = new Viewport(this.root);
    this.viewport.svg.setAttribute("aria-label", "Neighbourhood of a module");
    this.viewport.svg.addEventListener("click", (e) => {
      if (e.target === this.viewport.svg) cb.onSelect(null);
    });
  }

  fit(): void {
    this.viewport.fit();
  }

  /** The nodes behind a folded row, for the side panel. */
  members(id: string): { title: string; nodes: number[] } | undefined {
    const item = this.nb?.items.find((i) => i.id === id);
    return item?.members ? { title: item.label, nodes: item.members } : undefined;
  }

  show(nb: Neighbourhood, o: NbShow): void {
    if (o.key === this.lastKey && this.nb) {
      this.nb = nb;
      this.paint(o);
      return;
    }
    this.lastKey = o.key;
    this.nb = nb;
    this.build(nb, o);
    this.paint(o);
  }

  private build(nb: Neighbourhood, o: NbShow): void {
    const { model } = this;
    const scene = svgEl("g", { class: o.refit ? `scene ${ENTER_CLASS}` : "scene" });
    // column titles
    for (const c of nb.columns) {
      const title =
        c.col === 0
          ? nb.items.some((i) => i.col === 0 && i.module !== nb.focus)
            ? "Cycle"
            : "Module"
          : `${c.col < 0 ? "Importers" : "Imports"} · ${Math.abs(c.col)} hop${Math.abs(c.col) === 1 ? "" : "s"}`;
      scene.append(svgText(title, c.x + c.w / 2, 12, "lane-title", { "text-anchor": "middle" }));
    }
    const body = svgEl("g", { transform: `translate(0,${TOP})` });
    scene.append(body);

    const touch = new Map<number, Element[]>();
    const touching = (item: number, ...els: Element[]) => {
      const list = touch.get(item);
      if (list) list.push(...els);
      else touch.set(item, [...els]);
    };

    this.itemEls = nb.items.map((it) => {
      const cls = ["nb-item", `kind-${it.kind}`, it.focus ? "focus" : "", it.cyclic ? "cyclic" : ""]
        .filter(Boolean)
        .join(" ");
      const g = svgEl("g", { class: cls, transform: `translate(${it.x},${it.y})` });
      if (it.kind === "header") {
        g.append(svgText(it.label, 2, 13, "nb-header"));
        if (it.dir !== undefined) {
          g.setAttribute("tabindex", "0");
          g.setAttribute("role", "button");
          g.addEventListener("dblclick", (e) => {
            e.stopPropagation();
            this.cb.onOpenFolder(it.dir!);
          });
          g.addEventListener("pointermove", (e) =>
            this.tip.show(e.clientX, e.clientY, [
              it.label,
              "Double-click for the map of this folder",
            ]),
          );
          g.addEventListener("pointerleave", () => this.tip.hide());
        }
        return g;
      }
      g.setAttribute("tabindex", "0");
      g.setAttribute("role", "button");
      g.append(
        svgEl("rect", { class: "nb-rect", width: it.w, height: it.h, rx: 6 }),
        svgText(it.label, 10, it.h / 2 + 4, "nb-label"),
      );
      g.addEventListener("click", (e) => {
        e.stopPropagation();
        if (it.module !== undefined) this.cb.onSelect(model.nodeOfMod(it.module));
        else this.cb.onSelect(null, it.id);
      });
      if (it.module !== undefined) {
        g.addEventListener("dblclick", (e) => {
          e.stopPropagation();
          this.cb.onRecentre(model.nodeOfMod(it.module!));
        });
        g.addEventListener("keydown", (e) => {
          if (e.key !== "Enter") return;
          e.preventDefault();
          e.stopPropagation();
          this.cb.onRecentre(model.nodeOfMod(it.module!));
        });
        g.addEventListener("pointermove", (e) => {
          const m = it.module!;
          const lines = [
            model.modPaths[m],
            it.cyclic
              ? "in a cycle with the module in the middle"
              : `${Math.abs(it.col)} hop${Math.abs(it.col) === 1 ? "" : "s"} away`,
          ];
          if (model.modErrors[m] || model.modViolations[m]) {
            lines.push(
              `${model.modViolations[m]} violation${model.modViolations[m] === 1 ? "" : "s"}`,
            );
          }
          lines.push("Double-click to put it in the middle");
          this.tip.show(e.clientX, e.clientY, lines);
        });
      } else {
        g.addEventListener("pointermove", (e) =>
          this.tip.show(e.clientX, e.clientY, [it.label, "Click to list them"]),
        );
      }
      g.addEventListener("pointerleave", () => {
        this.tip.hide();
      });
      return g;
    });

    const edgeLayer = svgEl("g", { class: "links" });
    this.edgeEls = nb.edges.map((e: NbEdge) => {
      const bad = e.circular || e.error || e.back;
      const cls = ["link", bad ? "bad" : e.warn ? "warn" : "", e.typeOnly ? "type-only" : ""]
        .filter(Boolean)
        .join(" ");
      const g = svgEl("g", { class: cls }, [
        svgEl("path", { class: "hit", d: e.d }),
        svgEl("path", {
          class: "line",
          d: e.d,
          "stroke-width": 1.5,
          "marker-end": arrowFor(bad, e.warn && !bad),
        }),
      ]);
      const a = this.itemEls[e.from];
      const b = this.itemEls[e.to];
      touching(e.from, g, b);
      touching(e.to, g, a);
      g.addEventListener("pointerenter", () => setHover(scene, [g, a, b]));
      g.addEventListener("pointerleave", () => {
        setHover(scene, null);
        this.tip.hide();
      });
      g.addEventListener("pointermove", (ev) => {
        const m1 = nb.items[e.from].module!;
        const m2 = nb.items[e.to].module!;
        this.tip.show(ev.clientX, ev.clientY, [
          `${model.data.modules.name[m1]} → ${model.data.modules.name[m2]}`,
          ...(e.typeOnly ? ["type-only import"] : []),
          ...(e.back ? ["closes a cycle"] : []),
          "Click to see the import",
        ]);
      });
      g.addEventListener("click", (ev) => {
        ev.stopPropagation();
        this.cb.onSelectLink(
          model.nodeOfMod(nb.items[e.from].module!),
          model.nodeOfMod(nb.items[e.to].module!),
        );
      });
      edgeLayer.append(g);
      return g;
    });

    nb.items.forEach((it, i) => {
      if (it.kind !== "module") return;
      this.itemEls[i].addEventListener("pointerenter", () =>
        setHover(scene, [this.itemEls[i], ...(touch.get(i) ?? [])]),
      );
      this.itemEls[i].addEventListener("pointerleave", () => setHover(scene, null));
    });

    body.append(edgeLayer);
    const itemLayer = svgEl("g", { class: "boxes" });
    itemLayer.append(...this.itemEls);
    body.append(itemLayer);
    this.scene = scene;
    this.touch = touch;
    this.viewport.set(scene, nb.width, nb.height + TOP, o.refit);
  }

  private paint(o: NbPaint): void {
    const nb = this.nb;
    if (!nb) return;
    let pinned = -1;
    nb.items.forEach((it, i) => {
      const on =
        it.module !== undefined
          ? this.model.nodeOfMod(it.module) === o.selected
          : it.kind !== "header" && it.id === o.selectedBox;
      this.itemEls[i]?.classList.toggle("selected", on);
      // the middle module is selected by default, and lighting all its lines would dim the rest of every picture
      if (on && it.module !== nb.focus) pinned = i;
    });
    // a selected module keeps its lines lit, like a hover does
    if (this.scene) {
      pinHover(
        this.scene,
        pinned >= 0 ? [this.itemEls[pinned], ...(this.touch.get(pinned) ?? [])] : null,
      );
    }
    const sl = o.selectedLink;
    nb.edges.forEach((e, k) => {
      const on =
        sl !== undefined &&
        this.model.nodeOfMod(nb.items[e.from].module!) === sl.s &&
        this.model.nodeOfMod(nb.items[e.to].module!) === sl.t;
      this.edgeEls[k]?.classList.toggle("selected", on);
    });
  }
}
