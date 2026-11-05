import { boundaryDeps, groupByFolder, linkEdges } from "../graph/logic";
import type { Model } from "../data/model";
import type { Nav } from "../state/nav";
import { EDGE_CIRCULAR, MODULE_KIND_NAMES } from "../data/types";
import { el, icon, nodeIcon } from "../ui/ui";

const LIST_LIMIT = 60;
/** A group of imports with at most this many items starts open. */
const OPEN_LIMIT = 12;
const GROUP_LIMIT = 80;

/** What the panel shows: a module or a folder, or the imports behind a link. */
export type PanelTarget =
  | { node: number | null }
  | { link: { s: number; t: number } }
  | { members: { title: string; nodes: number[] } };

/** The side panel of a module, a folder or a link. */
export function renderPanel(host: HTMLElement, model: Model, target: PanelTarget, nav: Nav): void {
  host.replaceChildren();
  if ("link" in target) return linkPanel(host, model, target.link.s, target.link.t, nav);
  if ("members" in target) return membersPanel(host, model, target.members, nav);
  const node = target.node;
  if (node === null || node < 0) {
    host.append(
      el("p", { class: "muted", text: "Click a module, a folder or a line to see its details." }),
    );
    return;
  }
  if (model.isDir(node)) return folderPanel(host, model, node, nav);
  modulePanel(host, model, model.modOf(node), nav);
}

function row(label: string, value: string | Node): HTMLElement {
  return el("div", { class: "kv" }, [
    el("span", { class: "k", text: label }),
    typeof value === "string" ? el("span", { text: value }) : value,
  ]);
}

/** A link that focuses the graph on a folder or module. */
function nodeLink(model: Model, nav: Nav, node: number, text = model.path(node)): HTMLElement {
  const a = el("a", { class: "link", text, title: model.path(node), attrs: { href: "#" } });
  a.addEventListener("click", (e) => {
    e.preventDefault();
    nav.focus(node);
  });
  return a;
}

function pathLink(model: Model, nav: Nav, m: number): HTMLElement {
  return nodeLink(model, nav, model.nodeOfMod(m));
}

function button(text: string, run: () => void, title?: string): HTMLElement {
  const b = el("button", { class: "btn", text, title });
  b.addEventListener("click", run);
  return b;
}

/** Focus and Hide for a node. */
function scopeButtons(nav: Nav, node: number): HTMLElement {
  const hidden = new Set(nav.state().hide ?? []);
  const isHidden = hidden.has(node);
  return el("div", { class: "buttons" }, [
    button(
      "◎ Focus",
      () => nav.focus(node),
      "Show what is in this folder, or the neighbourhood of this module (F)",
    ),
    button(
      isHidden ? "Show" : "⊘ Hide",
      () => {
        if (!hidden.delete(node)) hidden.add(node);
        nav.go({ view: "graph", hide: hidden.size ? [...hidden] : undefined }, true);
      },
      "Remove it from the graph (H)",
    ),
  ]);
}

/** A `<details>` that starts open when the list is short. */
function group(summary: (Node | string)[], items: HTMLElement[], open: boolean): HTMLElement {
  const d = el("details", { class: "group" }, [
    el("summary", {}, summary),
    el(
      "ul",
      { class: "plain" },
      items.slice(0, LIST_LIMIT).map((i) => el("li", {}, [i])),
    ),
  ]);
  if (items.length > LIST_LIMIT) {
    d.append(el("p", { class: "muted small", text: `and ${items.length - LIST_LIMIT} more` }));
  }
  d.open = open;
  return d;
}

function badge(text: string, cls = ""): HTMLElement {
  return el("span", { class: `badge ${cls}`, text });
}

/** What sets an import apart: type-only, dynamic, part of a cycle. */
function badges(model: Model, e: number): HTMLElement[] {
  const out: HTMLElement[] = [];
  if (e < 0) return out;
  if (model.edgeKinds[e] & 1) out.push(badge("type"));
  if (model.edgeKinds[e] & 2) out.push(badge("dynamic"));
  if (model.edgeFlags[e] & EDGE_CIRCULAR) out.push(badge("cycle", "bad"));
  return out;
}

/** The part of a path below the folder `base`; the whole path for anything else. */
function relative(model: Model, node: number, base: number): string {
  const path = model.path(node);
  const prefix = base > 0 ? `${model.dirPaths[base]}/` : "";
  return prefix && path.startsWith(prefix) ? path.slice(prefix.length) : path;
}

/** The module pairs behind one line of the graph. */
function linkPanel(host: HTMLElement, model: Model, s: number, t: number, nav: Nav): void {
  const edges = linkEdges(model, s, t, !nav.state().types);
  const typeOnly = edges.filter((e) => model.edgeFlags[e] & 2).length;
  host.append(
    el("h2", { text: `${model.name(s)} → ${model.name(t)}` }),
    el("div", { class: "path", text: `${model.path(s)} → ${model.path(t)}` }),
    el("div", {
      text: `${edges.length} import${edges.length === 1 ? "" : "s"}${typeOnly ? ` (${typeOnly} type-only)` : ""}`,
    }),
  );
  host.append(
    el("div", { class: "buttons" }, [
      button(model.isDir(s) ? "◎ Focus source folder" : "◎ Focus source", () => nav.focus(s)),
    ]),
  );

  const bySource = new Map<number, number[]>();
  for (const e of edges) {
    const list = bySource.get(model.edgeFrom[e]);
    if (list) list.push(e);
    else bySource.set(model.edgeFrom[e], [e]);
  }
  const sources = [...bySource].sort((a, b) => b[1].length - a[1].length);
  for (const [m, list] of sources.slice(0, GROUP_LIMIT)) {
    host.append(
      group(
        [pathLink(model, nav, m), el("span", { class: "muted", text: ` · ${list.length}` })],
        list.map((e) =>
          el("span", { class: "dep-row" }, [
            pathLink(model, nav, model.edgeTo[e]),
            ...badges(model, e),
          ]),
        ),
        sources.length * 2 <= OPEN_LIMIT || list.length <= 3,
      ),
    );
  }
  if (sources.length > GROUP_LIMIT) {
    host.append(
      el("p", { class: "muted", text: `and ${sources.length - GROUP_LIMIT} more modules` }),
    );
  }
}

function section(title: string, count: number, items: HTMLElement[]): HTMLElement {
  const shown = items.slice(0, LIST_LIMIT);
  const more =
    items.length > shown.length
      ? [el("li", { class: "muted", text: `and ${items.length - shown.length} more` })]
      : [];
  return el("section", {}, [
    el("h3", { text: `${title} (${count})` }),
    count === 0
      ? el("p", { class: "muted", text: "None" })
      : el("ul", { class: "plain" }, [...shown.map((i) => el("li", {}, [i])), ...more]),
  ]);
}

/** The folders and modules that a pseudo-box merges, or that a folded row hides. Each one can be focused. */
function membersPanel(
  host: HTMLElement,
  model: Model,
  members: { title: string; nodes: number[] },
  nav: Nav,
): void {
  const sorted = [...members.nodes].sort((a, b) => model.path(a).localeCompare(model.path(b)));
  host.append(
    el("h2", { text: members.title }),
    el("div", { class: "muted", text: `${sorted.length} in all. Click one to focus it.` }),
    section(
      "Members",
      sorted.length,
      sorted.map((n) =>
        el("span", {}, [
          nodeLink(model, nav, n),
          model.isDir(n) ? el("span", { class: "muted", text: ` · ${model.dirModules[n]}` }) : "",
        ]),
      ),
    ),
  );
}

/** One line of a dependency list, with the lower-case text that the filter searches. */
interface ListRow {
  text: string;
  node: HTMLElement;
}

interface ListGroup {
  title: (Node | string)[];
  rows: ListRow[];
}

/** The list that stays selected while the selection changes, so the same side of the graph stays in view. */
let depTab: "out" | "in" = "out";
/** Lists with more rows than this get a filter. */
const FILTER_LIMIT = 10;

function renderGroups(groups: ListGroup[], query: string): HTMLElement[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  const shown = groups
    .map((g) => ({
      title: g.title,
      rows: words.length ? g.rows.filter((r) => words.every((w) => r.text.includes(w))) : g.rows,
    }))
    .filter((g) => g.rows.length > 0);
  if (shown.length === 0) return [el("p", { class: "muted", text: "No match." })];
  const total = shown.reduce((n, g) => n + g.rows.length, 0);
  const open = words.length > 0 || total <= OPEN_LIMIT || shown.length === 1;
  const out = shown.slice(0, GROUP_LIMIT).map((g) =>
    group(
      [...g.title, el("span", { class: "pill", text: String(g.rows.length) })],
      g.rows.map((r) => r.node),
      open,
    ),
  );
  if (shown.length > GROUP_LIMIT) {
    out.push(el("p", { class: "muted", text: `and ${shown.length - GROUP_LIMIT} more` }));
  }
  return out;
}

interface DepTabSpec {
  id: "out" | "in";
  label: string;
  /** Imports behind the tab. */
  count: number;
  groups: () => ListGroup[];
}

/**
 * Two lists on tabs: what the node depends on, and what uses it. One is in view at a time, with a filter
 * when it is long and a button that opens or closes every group. The tab bar goes at the end of `top`,
 * and the returned section is the list.
 */
function depBlock(tabs: [DepTabSpec, DepTabSpec], top: HTMLElement): HTMLElement {
  const bar = el("div", { class: "seg", attrs: { role: "tablist" } });
  const body = el("div", { class: "dep-body" });
  const buttons = new Map<string, HTMLButtonElement>();
  for (const t of tabs) {
    const b = el("button", { class: "seg-tab", attrs: { role: "tab" } }, [
      t.label,
      el("span", { class: "pill", text: String(t.count) }),
    ]);
    b.addEventListener("click", () => {
      depTab = t.id;
      paint(true);
    });
    buttons.set(t.id, b);
    bar.append(b);
  }
  bar.addEventListener("keydown", (e) => {
    if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
    // the page reads the arrows too
    e.stopPropagation();
    e.preventDefault();
    depTab = depTab === "out" ? "in" : "out";
    paint(true);
    buttons.get(depTab)!.focus();
  });

  /** `exact`: the user chose the tab just now, so show it even when it is empty. */
  const paint = (exact = false) => {
    let tab = tabs.find((t) => t.id === depTab) ?? tabs[0];
    // a remembered side with nothing in it would show "None" when the other side has something
    if (!exact && tab.count === 0) tab = tabs.find((t) => t !== tab && t.count > 0) ?? tab;
    for (const [id, b] of buttons) {
      b.classList.toggle("active", id === tab.id);
      b.setAttribute("aria-selected", String(id === tab.id));
      b.tabIndex = id === tab.id ? 0 : -1;
    }
    if (tab.count === 0) {
      body.replaceChildren(el("p", { class: "muted", text: "None" }));
      return;
    }
    const groups = tab.groups();
    const list = el("div", { class: "dep-list" }, renderGroups(groups, ""));
    const tools = el("div", { class: "dep-tools" });
    const rows = groups.reduce((n, g) => n + g.rows.length, 0);
    if (rows > FILTER_LIMIT) {
      const filter = el("input", {
        class: "dep-filter",
        attrs: { type: "search", placeholder: `Filter ${rows}…`, "aria-label": "Filter the list" },
      });
      filter.addEventListener("input", () =>
        list.replaceChildren(...renderGroups(groups, filter.value)),
      );
      filter.addEventListener("keydown", (e) => {
        if (e.key === "Escape" && filter.value) {
          e.stopPropagation();
          filter.value = "";
          list.replaceChildren(...renderGroups(groups, ""));
        }
      });
      tools.append(filter);
    }
    if (groups.length > 1) {
      const toggle = el("button", {
        class: "btn",
        text: "Expand all",
        title: "Open or close every group",
      });
      toggle.addEventListener("click", () => {
        const all = [...list.querySelectorAll("details")];
        const open = all.some((d) => !d.open);
        for (const d of all) d.open = open;
        toggle.textContent = open ? "Collapse all" : "Expand all";
      });
      tools.append(toggle);
    }
    body.replaceChildren(...(tools.childElementCount ? [tools] : []), list);
  };
  paint();
  // the tabs end the sticky top of the panel; the lists scroll under it
  top.append(bar);
  return el("section", { class: "deps" }, [body]);
}

function folderPanel(host: HTMLElement, model: Model, d: number, nav: Nav): void {
  const top = el("div", { class: "side-top" });
  host.append(top);
  top.append(
    el("h2", { class: "head" }, [icon("folder"), d === 0 ? "project" : model.name(d)]),
    el("div", { class: "path", text: model.dirPaths[d] || "." }),
    row("Modules", String(model.dirModules[d])),
    row("Errors", String(model.dirErrors[d])),
    row("Warnings", String(model.dirViolations[d] - model.dirErrors[d])),
  );
  if (d > 0) top.append(scopeButtons(nav, d));
  if (d === 0) return;

  const deps = boundaryDeps(model, d);
  // the importer is on the first line and the imported module on the second, whichever way the tab looks
  const spec = (id: "out" | "in", label: string, groups: typeof deps.out): DepTabSpec => ({
    id,
    label,
    count: groups.reduce((n, g) => n + g.edges.length, 0),
    groups: () =>
      groups.map((g) => {
        const base = model.isDir(g.node) ? g.node : model.parent(g.node);
        return {
          title: [nodeIcon(model, g.node), nodeLink(model, nav, g.node)],
          rows: g.edges.map((e) => {
            const from = model.nodeOfMod(model.edgeFrom[e]);
            const to = model.nodeOfMod(model.edgeTo[e]);
            const [fromBase, toBase] = id === "out" ? [d, base] : [base, d];
            return {
              text: `${model.path(from)} ${model.path(to)}`.toLowerCase(),
              node: el("span", { class: "pair" }, [
                nodeLink(model, nav, from, relative(model, from, fromBase)),
                el("span", { class: "pair-to" }, [
                  "→ ",
                  nodeLink(model, nav, to, relative(model, to, toBase)),
                  ...badges(model, e),
                ]),
              ]),
            };
          }),
        };
      }),
  });
  host.append(depBlock([spec("out", "Depends on", deps.out), spec("in", "Used by", deps.in)], top));
}

/** The modules at the other end of some imports, one group per folder. */
function moduleList(
  model: Model,
  nav: Nav,
  modules: number[],
  edgeOf: (other: number) => number,
): ListGroup[] {
  return groupByFolder(model, modules).map((g) => ({
    title: [icon("folder"), nodeLink(model, nav, g.dir, model.dirPaths[g.dir] || "(project root)")],
    rows: g.modules.map((m) => {
      const node = model.nodeOfMod(m);
      const errors = model.modErrors[m];
      return {
        text: model.modPaths[m].toLowerCase(),
        node: el("span", { class: "dep-row" }, [
          nodeIcon(model, node),
          nodeLink(model, nav, node, model.data.modules.name[m]),
          ...badges(model, edgeOf(m)),
          ...(errors ? [el("span", { class: "dot sev-error", title: `${errors} errors` })] : []),
        ]),
      };
    }),
  }));
}

function modulePanel(host: HTMLElement, model: Model, m: number, nav: Nav): void {
  const d = model.data;
  const top = el("div", { class: "side-top" });
  host.append(top);
  top.append(
    el("h2", { class: "head" }, [nodeIcon(model, model.nodeOfMod(m)), d.modules.name[m]]),
    el("div", { class: "path", text: model.modPaths[m] }),
    row("Kind", MODULE_KIND_NAMES[d.modules.kind[m]] ?? "local"),
  );
  if (d.modules.package[m] >= 0) top.append(row("Package", d.packages[d.modules.package[m]]));
  if (d.modules.flags[m] & 2)
    top.append(row("Orphan", "nothing imports it, and it imports nothing"));
  if (d.modules.flags[m] & 4) top.append(row("Parse error", "the file could not be parsed"));
  if (d.modules.flags[m] & 1) top.append(row("Not followed", "the graph stops here"));

  top.append(scopeButtons(nav, model.nodeOfMod(m)));

  const imports: number[] = [];
  for (let i = model.outStart[m]; i < model.outStart[m + 1]; i++)
    imports.push(model.edgeTo[model.outEdge[i]]);
  const importers: number[] = [];
  for (let i = model.inStart[m]; i < model.inStart[m + 1]; i++)
    importers.push(model.edgeFrom[model.inEdge[i]]);

  const violations = (model.violationsOf.get(m) ?? []).map((i) => {
    const v = d.violations[i];
    const item = el("span", {}, [
      el("b", { class: `sev-${v.level}`, text: d.rules[v.rule] }),
      `: ${v.message}`,
    ]);
    return item;
  });
  host.append(
    depBlock(
      [
        {
          id: "out",
          label: "Imports",
          count: imports.length,
          groups: () => moduleList(model, nav, imports, (to) => model.findEdge(m, to)),
        },
        {
          id: "in",
          label: "Imported by",
          count: importers.length,
          groups: () => moduleList(model, nav, importers, (from) => model.findEdge(from, m)),
        },
      ],
      top,
    ),
    section("Violations", violations.length, violations),
  );

  const exports = d.exports?.[m];
  if (d.symbols && exports) {
    const items = exports.map(([name, used, isType]) =>
      el("span", { class: used ? "" : "unused" }, [
        `${name}${isType ? " (type)" : ""}`,
        el("em", { text: used ? "" : "  unused" }),
      ]),
    );
    host.append(section("Exports", items.length, items));
  }
}
