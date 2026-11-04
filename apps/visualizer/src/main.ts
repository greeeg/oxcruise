import "./app.css";
import { GraphPage } from "./pages";
import type { Members } from "./pages";
import { Model } from "./model";
import type { Nav } from "./nav";
import { Palette } from "./palette";
import { renderPanel } from "./panel";
import type { PanelTarget } from "./panel";
import { focusPatch } from "./scope";
import { DEFAULT_STATE, formatHash, parseHash } from "./state";
import type { View, ViewState } from "./state";
import type { VizData } from "./types";
import { el, Toast, Tooltip } from "./ui";
import { ViolationsView } from "./violations";

const TABS: [View, string][] = [
  ["graph", "Graph"],
  ["violations", "Violations"],
];

function boot(): void {
  const app = document.getElementById("app")!;
  const data = (globalThis as { OXCRUISE_DATA?: VizData }).OXCRUISE_DATA;
  if (!data) {
    app.textContent = "data.js is missing or could not be read. Run `oxcruise --visualize` again.";
    return;
  }
  if (data.version !== 1) {
    app.textContent = `This site has data version ${data.version}, and the viewer reads version 1.`;
    return;
  }
  const model = new Model(data);
  const tip = new Tooltip();
  const toast = new Toast();
  let state: ViewState = parseHash(location.hash);
  // hops and direction only matter for a module focus
  const format = (s: ViewState) => formatHash(s, (p) => model.moduleByPath(p) >= 0);

  const nav: Nav = {
    state: () => state,
    go(patch, replace = false) {
      const next = { ...state, ...patch };
      // a patch that sets a key to `undefined` removes it
      const hash = format(next);
      if (hash === location.hash) return;
      if (replace) history.replaceState(null, "", hash);
      else history.pushState(null, "", hash);
      state = parseHash(hash);
      render();
    },
    focus(node, extra) {
      nav.go(focusPatch(model, node, extra));
    },
    toast: (text, action) => toast.show(text, action),
  };

  // layout
  const { summary } = data;
  const chip = (text: string, cls = "") => el("span", { class: `chip ${cls}`, text });
  const tabs = new Map<View, HTMLButtonElement>();
  for (const [view, label] of TABS) {
    const b = el("button", { class: "tab", text: label });
    b.addEventListener("click", () => nav.go({ view }));
    tabs.set(view, b);
  }
  const palette = new Palette(model, (node) => nav.focus(node));
  const header = el("header", {}, [
    el("h1", { text: "oxcruise" }),
    el("nav", { class: "tabs" }, [...tabs.values()]),
    palette.trigger,
    el("div", { class: "summary" }, [
      chip(`${summary.totalModules} modules`),
      chip(`${summary.totalDependencies} deps`),
      chip(`${summary.error} errors`, summary.error ? "sev-error" : ""),
      chip(`${summary.warn} warnings`, summary.warn ? "sev-warn" : ""),
    ]),
  ]);
  const main = el("main", {});
  const stage = el("div", { class: "stage" });
  const side = el("aside", { class: "side" });
  main.append(stage, side);
  app.replaceChildren(header, main);

  const panes = new Map<View, HTMLElement>();
  for (const [view] of TABS) {
    const pane = el("div", { class: "pane" });
    pane.hidden = true;
    stage.append(pane);
    panes.set(view, pane);
  }

  let graph: GraphPage | null = null;
  let violations: ViolationsView | null = null;

  const render = () => {
    for (const [view, tab] of tabs) {
      tab.classList.toggle("active", view === state.view);
      panes.get(view)!.hidden = view !== state.view;
    }
    tip.hide();
    const pane = panes.get(state.view)!;
    let shown: number | null = state.sel ? model.nodeByPath(state.sel) : null;
    let link: { s: number; t: number } | undefined;
    let members: Members | undefined;
    switch (state.view) {
      case "graph":
        graph ??= new GraphPage(pane, model, nav, tip);
        graph.show();
        if (state.link) {
          const [s, t] = state.link.split(">").map((p) => model.nodeByPath(p));
          if (s >= 0 && t >= 0) link = { s, t };
        }
        shown ??= state.focus ? model.nodeByPath(state.focus) : null;
        members = state.box ? graph.members(state.box) : undefined;
        break;
      case "violations":
        violations ??= new ViolationsView(pane, model, nav);
        violations.show();
        break;
    }
    const target: PanelTarget = link
      ? { link }
      : members
        ? { members }
        : { node: shown !== null && shown >= 0 ? shown : null };
    renderPanel(side, model, target, nav);
    document.title = `oxcruise · ${TABS.find(([v]) => v === state.view)![1]}`;
  };

  window.addEventListener("popstate", () => {
    state = parseHash(location.hash);
    render();
  });
  window.addEventListener("hashchange", () => {
    const next = parseHash(location.hash);
    if (format(next) !== format(state)) {
      state = next;
      render();
    }
  });
  window.addEventListener("keydown", (e) => {
    const t = e.target;
    if (
      palette.isOpen ||
      t instanceof HTMLInputElement ||
      t instanceof HTMLSelectElement ||
      t instanceof HTMLTextAreaElement
    )
      return;
    if (state.view === "graph" && graph?.onKey(e)) e.preventDefault();
  });

  if (!location.hash) state = { ...DEFAULT_STATE };
  render();
}

boot();
