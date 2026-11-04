import { arcPath, clip, curvePath, layoutLayers, textWidth } from "./layout";
import type { Point } from "./layout";
import type { Link } from "./logic";
import { weakThreshold } from "./map";
import type { MapBox, MapPort, MapScope } from "./map";
import type { Model } from "./model";
import { arrowFor, ENTER_CLASS, pinHover, setHover, svgEl, svgText, Viewport } from "./svgview";
import type { Tooltip } from "./ui";

export interface MapCallbacks {
  /** A click on a box (a node, or a pseudo-box id), a package, or the background (`null`). */
  onSelect(node: number | null, box?: string): void;
  /** A click on a line between two single boxes. */
  onSelectLink(s: number, t: number): void;
  /** A double click or Enter on a box: show the folder, or the neighbourhood of the module. */
  onDrill(node: number): void;
  /** A click on a port: show that folder. */
  onFocus(node: number): void;
}

export interface MapPaint {
  selected?: number;
  /** A pseudo-box or folded port that is selected. */
  selectedBox?: string;
  selectedLink?: { s: number; t: number };
}

export interface MapShow extends MapPaint {
  /** Everything that changes the picture: when it stays the same, only the selection is painted again. */
  key: string;
  /** Fit the picture to the screen (a new scope), instead of keeping the view. */
  refit: boolean;
  /** Draw the weak links too. */
  showWeak: boolean;
}

const BOX_H = 44;
const FRAME_PAD = 28;
const PILL_H = 24;
const CHIP_H = 22;

const boxWidth = (b: MapBox) => Math.min(240, Math.max(110, textWidth(b.label) + 44));

/** The id a selection uses for a pseudo-box. */
const pseudoId = (b: MapBox): string | undefined =>
  b.kind === "files" || b.kind === "folders" ? b.kind : undefined;

/** Boxes for the children of one folder, and the lines between them: the architecture map. */
export class MapView {
  readonly root = document.createElement("div");
  private readonly viewport: Viewport;
  private scope: MapScope | null = null;
  private lastKey: string | null = null;
  private scene: SVGGElement | null = null;
  private boxEls: SVGGElement[] = [];
  /** The lines and boxes that touch each box. */
  private touch = new Map<number, Element[]>();
  private linkEls: { el: SVGGElement; s: number; t: number }[] = [];
  private portEls: { el: SVGGElement; id: string }[] = [];
  /** How many weak links the picture leaves out unless they are asked for. */
  weakHidden = 0;

  constructor(
    host: HTMLElement,
    private readonly model: Model,
    private readonly cb: MapCallbacks,
    private readonly tip: Tooltip,
  ) {
    this.root.className = "map-host";
    host.append(this.root);
    this.viewport = new Viewport(this.root);
    this.viewport.svg.addEventListener("click", (e) => {
      const t = e.target as Element;
      if (t === this.viewport.svg || t.classList.contains("scope-frame")) cb.onSelect(null);
    });
    this.viewport.svg.setAttribute("aria-label", "Architecture map");
  }

  get folder(): number | undefined {
    return this.scope?.folder;
  }

  fit(): void {
    this.viewport.fit();
  }

  /** The nodes behind a pseudo-box or a folded port, for the side panel. */
  members(id: string): { title: string; nodes: number[] } | undefined {
    const s = this.scope;
    if (!s) return undefined;
    const box = s.boxes.find((b) => pseudoId(b) === id);
    if (box) return { title: box.label, nodes: box.members };
    const side = id === "ports-out" ? s.portsOut : id === "ports-in" ? s.portsIn : undefined;
    const port = side?.find((p) => p.node < 0);
    if (port?.nodes) {
      return { title: id === "ports-out" ? "Also depends on" : "Also used by", nodes: port.nodes };
    }
    return undefined;
  }

  show(scope: MapScope, o: MapShow): void {
    if (o.key === this.lastKey && this.scene) {
      this.paint(o);
      return;
    }
    this.lastKey = o.key;
    this.scope = scope;
    this.build(scope, o);
    this.paint(o);
  }

  private build(scope: MapScope, o: MapShow): void {
    const links = this.visibleLinks(scope, o.showWeak);
    this.weakHidden = scope.links.length - this.visibleLinks(scope, false).length;
    const lay = layoutLayers(
      scope.boxes.map((b) => ({ name: b.label, width: boxWidth(b) })),
      links.map((l) => [l.s, l.t] as [number, number]),
      { rowHeight: BOX_H, reverse: true },
    );

    // ports are pills on the top and bottom of the frame; the packages lane is below it
    const pillW = (p: MapPort) => Math.min(260, textWidth(`${p.label} · ${p.count}`) + 24);
    const rowW = (ports: MapPort[]) =>
      ports.reduce((s, p) => s + pillW(p), 0) + Math.max(0, ports.length - 1) * 12;
    const width = Math.max(
      lay.width + 2 * FRAME_PAD,
      rowW(scope.portsIn) + 24,
      rowW(scope.portsOut) + 24,
      scope.packages.length > 0 ? 420 : 0,
      scope.boxes.length === 0 ? 260 : 0,
    );
    const frameTop = scope.portsOut.length > 0 ? PILL_H / 2 + 6 : 0;
    const frameH = Math.max(lay.height, scope.boxes.length === 0 ? 40 : 0) + 2 * FRAME_PAD;
    const frameBottom = frameTop + frameH;
    const ox = (width - lay.width) / 2;
    const oy = frameTop + FRAME_PAD;
    const shift = (p: Point): Point => ({ x: p.x + ox, y: p.y + oy });

    const scene = svgEl("g", { class: o.refit ? `scene ${ENTER_CLASS}` : "scene" });
    scene.append(
      svgEl("rect", {
        class: "scope-frame",
        x: 0,
        y: frameTop,
        width,
        height: frameH,
        rx: 14,
      }),
    );
    if (scope.boxes.length === 0) {
      scene.append(
        svgText("Nothing to draw here", width / 2, frameTop + frameH / 2 + 4, "empty", {
          "text-anchor": "middle",
        }),
      );
    }

    // cycles
    for (const f of lay.frames) {
      const label = `cycle of ${f.nodes.length}`;
      scene.append(
        svgEl("rect", {
          class: "cycle-frame",
          x: f.box.x + ox,
          y: f.box.y + oy,
          width: f.box.w,
          height: f.box.h,
          rx: 10,
        }),
        svgText(label, f.box.x + ox + 10, f.box.y + oy + 12, "cycle-label"),
      );
    }

    const touch = new Map<number, Element[]>();
    const touching = (box: number, ...els: Element[]) => {
      const list = touch.get(box);
      if (list) list.push(...els);
      else touch.set(box, [...els]);
    };

    // boxes (made first so that links can name them, added last so that they sit on top)
    this.boxEls = scope.boxes.map((b, i) => {
      const r = lay.boxes[i];
      const g = svgEl("g", {
        class: `box kind-${b.kind}`,
        transform: `translate(${r.x + ox},${r.y + oy})`,
        tabindex: 0,
        role: "button",
        "aria-label": b.label,
      });
      g.append(svgEl("rect", { class: "box-rect", width: r.w, height: r.h, rx: 7 }));
      const badge = b.errors > 0 ? b.errors : b.violations;
      const badgeText = badge > 0 ? String(badge) : "";
      g.append(
        svgText(clip(b.label, r.w - 20 - textWidth(badgeText) - 4), 10, 18, "box-name"),
        svgText(
          b.kind === "mod" ? "module" : `${b.modules} module${b.modules === 1 ? "" : "s"}`,
          10,
          33,
          "box-sub",
        ),
      );
      if (badge > 0) {
        g.append(
          svgText(badgeText, r.w - 10, 18, `badge ${b.errors > 0 ? "sev-error" : "sev-warn"}`, {
            "text-anchor": "end",
          }),
        );
      }
      this.listenBox(g, b, i);
      return g;
    });

    // lines
    const linkLayer = svgEl("g", { class: "links" });
    const nodeOf = (box: number) => scope.boxes[box].node;
    this.linkEls = links.map((l, k) => {
      const route = lay.routes[k];
      const pts = route.points.map(shift);
      const d = route.inCycle && pts.length >= 2 ? arcPath(pts[0], pts[1]) : curvePath(pts);
      const bad = l.circular || l.error;
      const cls = [
        "link",
        bad ? "bad" : l.warn ? "warn" : "",
        l.typeOnly === l.count ? "type-only" : "",
      ]
        .filter(Boolean)
        .join(" ");
      const width = Math.min(8, 1 + Math.log2(l.count));
      const g = svgEl("g", { class: cls }, [
        svgEl("path", { class: "hit", d }),
        svgEl("path", {
          class: "line",
          d,
          "stroke-width": width,
          "marker-end": arrowFor(bad, l.warn && !bad),
        }),
      ]);
      touching(l.s, g, this.boxEls[l.t]);
      touching(l.t, g, this.boxEls[l.s]);
      g.addEventListener("pointerenter", () =>
        setHover(scene, [g, this.boxEls[l.s], this.boxEls[l.t]]),
      );
      g.addEventListener("pointermove", (e) =>
        this.tip.show(e.clientX, e.clientY, this.linkTip(scope, l)),
      );
      g.addEventListener("pointerleave", () => {
        setHover(scene, null);
        this.tip.hide();
      });
      g.addEventListener("click", (e) => {
        e.stopPropagation();
        if (nodeOf(l.s) >= 0 && nodeOf(l.t) >= 0) this.cb.onSelectLink(nodeOf(l.s), nodeOf(l.t));
      });
      linkLayer.append(g);
      return { el: g, s: l.s, t: l.t };
    });
    scene.append(linkLayer);

    // ports
    const portLayer = svgEl("g", { class: "ports" });
    this.portEls = [];
    const placePorts = (ports: MapPort[], y: number, side: "in" | "out") => {
      let x = (width - rowW(ports)) / 2;
      for (const p of ports) {
        const w = pillW(p);
        const id = `ports-${side}`;
        const g = svgEl("g", {
          class: `port${p.node < 0 ? " folded" : ""}`,
          transform: `translate(${x},${y - PILL_H / 2})`,
          tabindex: 0,
          role: "button",
        });
        g.append(
          svgEl("rect", { class: "port-rect", width: w, height: PILL_H, rx: PILL_H / 2 }),
          svgText(`${clip(p.label, w - 40)} · ${p.count}`, w / 2, 16, "port-text", {
            "text-anchor": "middle",
          }),
        );
        const cx = x + w / 2;
        const lines = p.boxes.map((pb) => {
          const r = lay.boxes[pb.box];
          const bx = r.x + ox + r.w / 2;
          // what is imported is on top: the pills of "out" are above the frame, those of "in" below it
          const from: Point = side === "in" ? { x: cx, y: y - PILL_H / 2 } : { x: bx, y: r.y + oy };
          const to: Point =
            side === "in" ? { x: bx, y: r.y + oy + r.h } : { x: cx, y: y + PILL_H / 2 };
          const line = svgEl("path", {
            class: "port-line",
            d: curvePath([from, to]),
            "stroke-width": Math.min(5, 1 + Math.log2(pb.count) * 0.7),
            "marker-end": arrowFor(false, false),
          });
          touching(pb.box, line, g);
          return line;
        });
        for (const line of lines) portLayer.append(line);
        const litBoxes = p.boxes.map((pb) => this.boxEls[pb.box]);
        g.addEventListener("pointerenter", () => setHover(scene, [g, ...lines, ...litBoxes]));
        g.addEventListener("pointermove", (e) =>
          this.tip.show(e.clientX, e.clientY, [
            p.node >= 0 ? this.model.path(p.node) : `${p.nodes?.length ?? 0} more`,
            `${p.count} import${p.count === 1 ? "" : "s"} ${side === "in" ? "into" : "out of"} this folder`,
            ...(p.node >= 0 ? ["Click to show it"] : []),
          ]),
        );
        g.addEventListener("pointerleave", () => {
          setHover(scene, null);
          this.tip.hide();
        });
        g.addEventListener("click", (e) => {
          e.stopPropagation();
          if (p.node >= 0) this.cb.onFocus(p.node);
          else this.cb.onSelect(null, id);
        });
        this.portEls.push({ el: g, id: p.node < 0 ? id : "" });
        portLayer.append(g);
        x += w + 12;
      }
    };
    placePorts(scope.portsOut, frameTop, "out");
    placePorts(scope.portsIn, frameBottom, "in");
    scene.append(portLayer);

    const boxLayer = svgEl("g", { class: "boxes" });
    boxLayer.append(...this.boxEls);
    scene.append(boxLayer);

    // packages
    let bottom = frameBottom + (scope.portsIn.length > 0 ? PILL_H / 2 : 0);
    if (scope.packages.length > 0) {
      const y0 = bottom + 22;
      const extra = scope.packageTotal - scope.packages.length;
      scene.append(
        svgText(
          `Packages · ${scope.packageTotal} used, ${scope.packageImports} imports`,
          0,
          y0,
          "lane-title",
        ),
      );
      let x = 0;
      let y = y0 + 10;
      const place = (label: string, count: number, module: number, files: string[] = []) => {
        const w = Math.min(220, textWidth(`${label} · ${count}`) + 20);
        if (x > 0 && x + w > width) {
          x = 0;
          y += CHIP_H + 6;
        }
        const g = svgEl(
          "g",
          { class: "pkg-chip", transform: `translate(${x},${y})`, tabindex: 0, role: "button" },
          [
            svgEl("rect", { class: "pkg-rect", width: w, height: CHIP_H, rx: 6 }),
            svgText(`${clip(label, w - 36)} · ${count}`, 8, 15, "pkg-text"),
          ],
        );
        g.addEventListener("click", (e) => {
          e.stopPropagation();
          if (module >= 0) this.cb.onSelect(this.model.nodeOfMod(module));
        });
        g.addEventListener("pointermove", (e) =>
          this.tip.show(e.clientX, e.clientY, [
            label,
            `${count} import${count === 1 ? "" : "s"}`,
            ...files.slice(0, 5),
            ...(files.length > 5 ? [`+${files.length - 5} more files`] : []),
          ]),
        );
        g.addEventListener("pointerleave", () => this.tip.hide());
        scene.append(g);
        x += w + 6;
      };
      for (const p of scope.packages) place(p.name, p.count, p.module, p.files);
      if (extra > 0) place(`+${extra} more`, 0, -1);
      bottom = y + CHIP_H;
    }

    // hover: a box lights its lines and what they touch, and the rest fades
    scope.boxes.forEach((_, i) => {
      this.boxEls[i].addEventListener("pointerenter", () =>
        setHover(scene, [this.boxEls[i], ...(touch.get(i) ?? [])]),
      );
      this.boxEls[i].addEventListener("pointerleave", () => setHover(scene, null));
    });

    this.scene = scene;
    this.touch = touch;
    this.viewport.set(scene, width, bottom, o.refit);
  }

  private listenBox(g: SVGGElement, b: MapBox, i: number): void {
    const id = pseudoId(b);
    g.addEventListener("click", (e) => {
      e.stopPropagation();
      // the second click of a double click would land on the new picture and open a box there
      if (e.detail > 1) return;
      // a click opens a folder or a module; Shift-click only selects it, and a pseudo-box can only be selected
      if (b.node >= 0 && !e.shiftKey) this.cb.onDrill(b.node);
      else this.cb.onSelect(b.node >= 0 ? b.node : null, id);
    });
    g.addEventListener("keydown", (e) => {
      if (e.key === "Enter" && b.node >= 0) {
        e.preventDefault();
        e.stopPropagation();
        this.cb.onDrill(b.node);
      }
    });
    g.addEventListener("pointermove", (e) =>
      this.tip.show(e.clientX, e.clientY, this.boxTip(b, i)),
    );
    g.addEventListener("pointerleave", () => this.tip.hide());
  }

  private boxTip(b: MapBox, i: number): string[] {
    const s = this.scope!;
    const out = s.links.filter((l) => l.s === i).reduce((n, l) => n + l.count, 0);
    const into = s.links.filter((l) => l.t === i).reduce((n, l) => n + l.count, 0);
    const lines = [
      b.node >= 0 ? this.model.path(b.node) || b.label : b.label,
      `${b.modules} module${b.modules === 1 ? "" : "s"}`,
      `imports ${out} · imported by ${into} (inside this folder)`,
    ];
    if (b.errors || b.violations) {
      lines.push(
        `${b.errors} error${b.errors === 1 ? "" : "s"}, ${b.violations - b.errors} warning${b.violations - b.errors === 1 ? "" : "s"}`,
      );
    }
    lines.push(
      b.node >= 0 ? "Click to open · Shift-click for details" : "Click to list what is in it",
    );
    return lines;
  }

  private linkTip(scope: MapScope, l: Link): string[] {
    const lines = [
      `${scope.boxes[l.s].label} → ${scope.boxes[l.t].label}`,
      `${l.count} import${l.count === 1 ? "" : "s"}${l.typeOnly ? ` (${l.typeOnly} type-only)` : ""}`,
    ];
    if (l.circular) lines.push("part of a cycle");
    if (l.error) lines.push("has errors");
    else if (l.warn) lines.push("has warnings");
    return lines;
  }

  /** The links that get drawn: all of them in a small scope, else the strongest and those with violations. */
  private visibleLinks(scope: MapScope, showWeak: boolean): Link[] {
    if (showWeak) return scope.links;
    const th = weakThreshold(scope.links);
    return scope.links.filter((l) => l.count >= th || l.circular || l.error || l.warn);
  }

  /** The selection: classes only, so a click never rebuilds the picture under the pointer. */
  private paint(o: MapPaint): void {
    const s = this.scope;
    if (!s) return;
    let pinned = -1;
    s.boxes.forEach((b, i) => {
      const on = b.node >= 0 ? b.node === o.selected : pseudoId(b) === o.selectedBox;
      this.boxEls[i]?.classList.toggle("selected", on);
      if (on) pinned = i;
    });
    // a selected box keeps its lines lit, like a hover does
    if (this.scene) {
      pinHover(
        this.scene,
        pinned >= 0 ? [this.boxEls[pinned], ...(this.touch.get(pinned) ?? [])] : null,
      );
    }
    for (const p of this.portEls)
      p.el.classList.toggle("selected", p.id !== "" && p.id === o.selectedBox);
    const sl = o.selectedLink;
    for (const l of this.linkEls) {
      const on = sl !== undefined && s.boxes[l.s].node === sl.s && s.boxes[l.t].node === sl.t;
      l.el.classList.toggle("selected", on);
    }
  }
}
