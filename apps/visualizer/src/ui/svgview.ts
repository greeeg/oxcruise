import { select } from "d3-selection";
import { zoom, zoomIdentity } from "d3-zoom";
import type { D3ZoomEvent, ZoomBehavior } from "d3-zoom";

const NS = "http://www.w3.org/2000/svg";

/** An SVG element with attributes (numbers are written as they are) and children. */
export function svgEl<K extends keyof SVGElementTagNameMap>(
  tag: K,
  attrs: Record<string, string | number> = {},
  children: (Node | string)[] = [],
): SVGElementTagNameMap[K] {
  const e = document.createElementNS(NS, tag);
  for (const [k, v] of Object.entries(attrs)) e.setAttribute(k, String(v));
  e.append(...children);
  return e;
}

/** `<text>` with a position and a class. */
export function svgText(
  text: string,
  x: number,
  y: number,
  cls: string,
  attrs: Record<string, string | number> = {},
): SVGTextElement {
  return svgEl("text", { x, y, class: cls, ...attrs }, [text]);
}

/** The arrow heads. `fill` comes from CSS, so they follow the theme. */
function arrows(): SVGDefsElement {
  const marker = (id: string, cls: string) =>
    svgEl(
      "marker",
      {
        id,
        viewBox: "0 0 10 10",
        refX: 9,
        refY: 5,
        markerWidth: 9,
        markerHeight: 9,
        markerUnits: "userSpaceOnUse",
        orient: "auto",
      },
      [svgEl("path", { d: "M0,1 L10,5 L0,9 z", class: cls })],
    );
  return svgEl("defs", {}, [
    marker("arrow", "arrow"),
    marker("arrow-error", "arrow arrow-error"),
    marker("arrow-warn", "arrow arrow-warn"),
  ]);
}

export function arrowFor(error: boolean, warn: boolean): string {
  return `url(#${error ? "arrow-error" : warn ? "arrow-warn" : "arrow"})`;
}

/** A pannable, zoomable SVG. The scene inside is swapped, never moved: pan and zoom live on a wrapper. */
export class Viewport {
  readonly svg = svgEl("svg", { class: "viewport", role: "img" });
  private readonly pan = svgEl("g");
  private readonly behavior: ZoomBehavior<SVGSVGElement, unknown>;
  private size = { w: 0, h: 0 };
  private touched = false;
  private pendingFit = false;
  private readonly resizer: ResizeObserver;

  constructor(host: HTMLElement) {
    this.svg.append(arrows(), this.pan);
    host.append(this.svg);
    this.behavior = zoom<SVGSVGElement, unknown>()
      .scaleExtent([0.05, 6])
      // a double click drills in: it must not zoom
      .filter(
        (e: Event) => e.type !== "dblclick" && (!(e as MouseEvent).ctrlKey || e.type === "wheel"),
      )
      .on("zoom", (e: D3ZoomEvent<SVGSVGElement, unknown>) => {
        this.pan.setAttribute("transform", e.transform.toString());
        if (e.sourceEvent) this.touched = true;
      });
    select(this.svg).call(this.behavior).on("dblclick.zoom", null);
    this.resizer = new ResizeObserver(() => {
      if (this.pendingFit || !this.touched) this.fit();
    });
    this.resizer.observe(this.svg);
  }

  /** Put a new scene in; `refit` fits it to the screen, otherwise the view stays where the user left it. */
  set(scene: SVGGElement, w: number, h: number, refit: boolean): void {
    this.pan.replaceChildren(scene);
    this.size = { w, h };
    if (refit) {
      this.touched = false;
      this.fit();
    }
  }

  fit(): void {
    const { w, h } = this.size;
    const box = this.svg.getBoundingClientRect();
    if (w <= 0 || h <= 0) return;
    // hidden pane: fit again once it has a size
    this.pendingFit = box.width === 0 || box.height === 0;
    if (this.pendingFit) return;
    const margin = 32;
    const k = Math.min((box.width - 2 * margin) / w, (box.height - 2 * margin) / h, 1.5);
    const scale = Math.max(0.05, k);
    const t = zoomIdentity
      .translate((box.width - w * scale) / 2, Math.max(margin, (box.height - h * scale) / 2))
      .scale(scale);
    this.touched = false;
    select(this.svg).call(this.behavior.transform, t);
    this.touched = false;
  }

  destroy(): void {
    this.resizer.disconnect();
    this.svg.remove();
  }
}

/** The class that makes the scene grow in when the scope changes. */
export const ENTER_CLASS = "enter";

let pendingClear = 0;
/** The lit set that comes back when the pointer leaves: that of the selected box. */
const pinned = new WeakMap<Element, Element[]>();

/**
 * Lit and dimmed parts while the pointer is on one: `lit` keeps full colour, the rest fades.
 * Clearing waits for the next frame, so moving from one part to the next (a box, then the line beside it)
 * goes straight from one lit set to the other and never flashes through the plain picture.
 * When the scene has a pinned set (`pinHover`), that is what clearing goes back to.
 */
export function setHover(scene: SVGGElement, lit: Iterable<Element> | null): void {
  cancelAnimationFrame(pendingClear);
  const apply = () => {
    const shown = lit ?? pinned.get(scene) ?? null;
    for (const e of scene.querySelectorAll(".lit")) e.classList.remove("lit");
    scene.classList.toggle("hovering", shown !== null);
    if (shown) for (const e of shown) e.classList.add("lit");
  };
  if (lit === null) pendingClear = requestAnimationFrame(apply);
  else apply();
}

/** Keep `lit` lit (and the rest faded) while nothing is hovered, or `null` to stop. It shows at once. */
export function pinHover(scene: SVGGElement, lit: Element[] | null): void {
  if (lit) pinned.set(scene, lit);
  else pinned.delete(scene);
  setHover(scene, lit);
}
