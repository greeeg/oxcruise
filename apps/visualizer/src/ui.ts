import type { Model } from "./model";

/** Small DOM helpers shared by the views. */

export function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  props: { class?: string; text?: string; title?: string; attrs?: Record<string, string> } = {},
  children: (Node | string)[] = [],
): HTMLElementTagNameMap[K] {
  const e = document.createElement(tag);
  if (props.class) e.className = props.class;
  if (props.text !== undefined) e.textContent = props.text;
  if (props.title) e.title = props.title;
  for (const [k, v] of Object.entries(props.attrs ?? {})) e.setAttribute(k, v);
  e.append(...children);
  return e;
}

/** One tooltip for the whole page. */
export class Tooltip {
  private readonly node = el("div", { class: "tooltip" });
  constructor() {
    this.node.hidden = true;
    document.body.append(this.node);
  }
  show(x: number, y: number, lines: string[]): void {
    this.node.replaceChildren(
      ...lines.map((l, i) => el("div", { class: i === 0 ? "tip-title" : "tip-line", text: l })),
    );
    this.node.hidden = false;
    const r = this.node.getBoundingClientRect();
    const left = Math.min(x + 14, window.innerWidth - r.width - 8);
    const top = Math.min(y + 14, window.innerHeight - r.height - 8);
    this.node.style.transform = `translate(${Math.max(8, left)}px, ${Math.max(8, top)}px)`;
  }
  hide(): void {
    this.node.hidden = true;
  }
}

/** A short message at the bottom of the page, with an optional button. */
export class Toast {
  private readonly node = el("div", { class: "toast", attrs: { role: "status" } });
  private timer = 0;
  constructor() {
    this.node.hidden = true;
    document.body.append(this.node);
  }
  show(text: string, action?: { label: string; run: () => void }): void {
    const children: (Node | string)[] = [text];
    if (action) {
      const b = el("button", { class: "btn", text: action.label });
      b.addEventListener("click", () => {
        this.hide();
        action.run();
      });
      children.push(b);
    }
    this.node.replaceChildren(...children);
    this.node.hidden = false;
    clearTimeout(this.timer);
    this.timer = window.setTimeout(() => this.hide(), 8000);
  }
  hide(): void {
    this.node.hidden = true;
  }
}

const ICONS = {
  folder:
    "M1.5 3.5A1 1 0 0 1 2.5 2.5h3.2l1.3 1.5h6.5a1 1 0 0 1 1 1v7a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1z",
  file: "M3.5 1.5h5.6l3.4 3.4v9.1a.5.5 0 0 1-.5.5h-8.5a.5.5 0 0 1-.5-.5v-12a.5.5 0 0 1 .5-.5zM9 1.8V5h3.2",
  search: "M7 12.2a5.2 5.2 0 1 0 0-10.4 5.2 5.2 0 0 0 0 10.4zM10.8 10.8l3.4 3.4",
  collapse: "M2.5 5.5h8.5v8h-8.5zM5.5 5.5v-3h8v8h-2.5M4.5 9.5h4.5",
  package: "M8 1.5l6 3v7l-6 3-6-3v-7zM2 4.5l6 3 6-3M8 7.5v7",
} as const;

/** A small outline icon that follows the text colour. */
export function icon(kind: keyof typeof ICONS): SVGSVGElement {
  const ns = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(ns, "svg");
  svg.setAttribute("viewBox", "0 0 16 16");
  svg.setAttribute("class", `icon icon-${kind}`);
  svg.setAttribute("aria-hidden", "true");
  const path = document.createElementNS(ns, "path");
  path.setAttribute("d", ICONS[kind]);
  svg.append(path);
  return svg;
}

/** The icon of a node: a folder, a package or a file. */
export function nodeIcon(model: Model, node: number): SVGSVGElement {
  if (model.isDir(node)) return icon("folder");
  return icon(model.data.modules.kind[model.modOf(node)] === 1 ? "package" : "file");
}
