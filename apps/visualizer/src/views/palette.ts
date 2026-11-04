import type { Model } from "../data/model";
import { el, icon, nodeIcon } from "../ui/ui";

const RESULT_LIMIT = 50;
const isMac = /Mac|iPhone|iPad/i.test(
  globalThis.navigator?.platform || globalThis.navigator?.userAgent || "",
);

/** The spans `[start, end)` of `text` that the words of a search cover, sorted and not overlapping. */
export function matchRanges(text: string, words: readonly string[]): [number, number][] {
  const lower = text.toLowerCase();
  const found: [number, number][] = [];
  for (const w of words) {
    const i = w ? lower.indexOf(w) : -1;
    if (i >= 0) found.push([i, i + w.length]);
  }
  found.sort((a, b) => a[0] - b[0]);
  const merged: [number, number][] = [];
  for (const r of found) {
    const last = merged[merged.length - 1];
    if (last && r[0] <= last[1]) last[1] = Math.max(last[1], r[1]);
    else merged.push([r[0], r[1]]);
  }
  return merged;
}

function highlighted(text: string, words: readonly string[]): (Node | string)[] {
  const out: (Node | string)[] = [];
  let at = 0;
  for (const [a, b] of matchRanges(text, words)) {
    if (a > at) out.push(text.slice(at, a));
    out.push(el("mark", { text: text.slice(a, b) }));
    at = b;
  }
  if (at < text.length) out.push(text.slice(at));
  return out;
}

/**
 * The search of the header: a field that looks like an input but opens a palette over the page.
 * Ctrl+K or ⌘K opens it from anywhere, `/` too when no field has the focus.
 */
export class Palette {
  /** The button for the header. */
  readonly trigger: HTMLButtonElement;
  private readonly backdrop = el("div", { class: "palette-backdrop" });
  private readonly input = el("input", {
    class: "palette-input",
    attrs: {
      type: "text",
      role: "combobox",
      placeholder: "Search files and folders…",
      "aria-label": "Search files and folders",
      "aria-expanded": "true",
      "aria-controls": "palette-list",
      autocomplete: "off",
      spellcheck: "false",
    },
  });
  private readonly list = el("ul", {
    class: "palette-list plain",
    attrs: { id: "palette-list", role: "listbox" },
  });
  private readonly foot = el("div", { class: "palette-foot muted small" });
  private hits: number[] = [];
  private active = 0;
  private words: string[] = [];
  private returnTo: Element | null = null;

  constructor(
    private readonly model: Model,
    private readonly choose: (node: number) => void,
  ) {
    this.trigger = el("button", { class: "search-trigger", attrs: { "aria-haspopup": "dialog" } }, [
      icon("search"),
      el("span", { class: "search-trigger-text", text: "Search files and folders…" }),
      el("kbd", { text: isMac ? "⌘K" : "Ctrl K" }),
    ]);
    this.trigger.addEventListener("click", () => this.open());

    const dialog = el(
      "div",
      { class: "palette", attrs: { role: "dialog", "aria-modal": "true", "aria-label": "Search" } },
      [el("div", { class: "palette-field" }, [icon("search"), this.input]), this.list, this.foot],
    );
    // a click on a result must not take the focus from the field
    dialog.addEventListener("mousedown", (e) => {
      if (e.target !== this.input) e.preventDefault();
    });
    this.backdrop.append(dialog);
    this.backdrop.hidden = true;
    document.body.append(this.backdrop);

    this.backdrop.addEventListener("mousedown", (e) => {
      if (e.target === this.backdrop) this.close();
    });
    this.list.addEventListener("mousemove", (e) => {
      const i = this.indexOf(e.target);
      if (i >= 0 && i !== this.active) this.move(i);
    });
    this.list.addEventListener("click", (e) => {
      const i = this.indexOf(e.target);
      if (i >= 0) this.pick(i);
    });
    this.input.addEventListener("input", () => this.search());
    this.input.addEventListener("keydown", (e) => this.onInputKey(e));
    window.addEventListener("keydown", (e) => this.onKey(e), true);
  }

  get isOpen(): boolean {
    return !this.backdrop.hidden;
  }

  open(): void {
    if (this.isOpen) return;
    this.returnTo = document.activeElement;
    this.backdrop.hidden = false;
    this.input.value = "";
    this.search();
    this.input.focus();
  }

  close(): void {
    if (!this.isOpen) return;
    this.backdrop.hidden = true;
    if (this.returnTo instanceof HTMLElement) this.returnTo.focus({ preventScroll: true });
    this.returnTo = null;
  }

  private onKey(e: KeyboardEvent): void {
    if ((e.metaKey || e.ctrlKey) && !e.altKey && !e.shiftKey && e.key.toLowerCase() === "k") {
      e.preventDefault();
      e.stopPropagation();
      if (this.isOpen) this.close();
      else this.open();
      return;
    }
    const t = e.target;
    const typing =
      t instanceof HTMLInputElement ||
      t instanceof HTMLSelectElement ||
      t instanceof HTMLTextAreaElement;
    if (e.key === "/" && !e.metaKey && !e.ctrlKey && !e.altKey && !typing && !this.isOpen) {
      e.preventDefault();
      this.open();
    }
  }

  private onInputKey(e: KeyboardEvent): void {
    const n = this.hits.length;
    switch (e.key) {
      case "ArrowDown":
      case "ArrowUp":
        e.preventDefault();
        if (n > 0) this.move((this.active + (e.key === "ArrowDown" ? 1 : -1) + n) % n);
        break;
      case "PageDown":
      case "PageUp":
        e.preventDefault();
        if (n > 0) {
          const step = e.key === "PageDown" ? 8 : -8;
          this.move(Math.max(0, Math.min(n - 1, this.active + step)));
        }
        break;
      case "Enter":
        e.preventDefault();
        this.pick(this.active);
        break;
      case "Escape":
        e.preventDefault();
        // the first Escape clears the query, the second closes
        if (this.input.value) {
          this.input.value = "";
          this.search();
        } else {
          this.close();
        }
        break;
      case "Tab":
        e.preventDefault();
        break;
    }
  }

  private indexOf(target: EventTarget | null): number {
    const li = target instanceof Element ? target.closest("li[data-i]") : null;
    return li ? Number(li.getAttribute("data-i")) : -1;
  }

  private pick(i: number): void {
    const node = this.hits[i];
    if (node === undefined) return;
    this.close();
    this.choose(node);
  }

  private search(): void {
    const query = this.input.value;
    this.words = query.toLowerCase().split(/\s+/).filter(Boolean);
    this.hits = this.model.searchNodes(query, RESULT_LIMIT);
    this.active = 0;
    this.paint();
  }

  /** Moves the highlight without rebuilding the list, so the mouse keeps working. */
  private move(i: number): void {
    const items = this.list.children;
    items[this.active]?.classList.remove("active");
    items[this.active]?.setAttribute("aria-selected", "false");
    this.active = i;
    const li = items[i];
    li?.classList.add("active");
    li?.setAttribute("aria-selected", "true");
    this.input.setAttribute("aria-activedescendant", li?.id ?? "");
    li?.scrollIntoView({ block: "nearest" });
  }

  private paint(): void {
    const { model, words } = this;
    this.list.replaceChildren(
      ...this.hits.map((node, i) => {
        const name = model.name(node);
        const path = model.path(node);
        const dir = path.slice(0, path.length - name.length).replace(/\/$/, "");
        const errors = model.isDir(node)
          ? model.dirErrors[node]
          : model.modErrors[model.modOf(node)];
        const li = el(
          "li",
          {
            class: `palette-item${i === this.active ? " active" : ""}`,
            attrs: {
              id: `palette-item-${i}`,
              role: "option",
              "aria-selected": String(i === this.active),
              "data-i": String(i),
            },
          },
          [
            nodeIcon(model, node),
            el("span", { class: "palette-name" }, highlighted(name, words)),
            el("span", { class: "palette-dir muted" }, highlighted(dir, words)),
            ...(errors ? [el("span", { class: "dot sev-error", title: `${errors} errors` })] : []),
            ...(model.isDir(node)
              ? [
                  el("span", {
                    class: "palette-kind muted",
                    text: `${model.dirModules[node]} modules`,
                  }),
                ]
              : []),
          ],
        );
        return li;
      }),
    );
    this.input.setAttribute("aria-activedescendant", this.hits.length ? "palette-item-0" : "");
    if (words.length === 0) {
      this.list.replaceChildren(
        el("li", {
          class: "palette-empty muted",
          text: `${model.nMods} modules and ${model.nDirs - 1} folders. Type a name or part of a path.`,
        }),
      );
    } else if (this.hits.length === 0) {
      this.list.replaceChildren(el("li", { class: "palette-empty muted", text: "No match." }));
    }
    this.list.scrollTop = 0;
    this.foot.replaceChildren(
      ...[
        ["↑↓", "move"],
        ["↵", "open"],
        ["esc", "close"],
      ].map(([k, v]) => el("span", {}, [el("kbd", { text: k }), ` ${v}`])),
    );
  }
}
