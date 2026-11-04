import type { Direction } from "../graph/logic";

export type View = "graph" | "violations";

/** Everything the URL hash remembers, so a link restores the view. */
export interface ViewState {
  view: View;
  /** Graph: the folders and modules that are hidden (node ids). */
  hide?: number[];
  /** Graph: the folder or module the picture is limited to. */
  focus?: string;
  /** Graph, module focus: how far the neighbourhood reaches, and in which direction. */
  hops: number;
  dir: Direction;
  /** Graph: show the packages (the `(external)` folder). */
  pkgs: boolean;
  /** Graph: show the type-only dependencies. */
  types: boolean;
  /** The node in the side panel. */
  sel?: string;
  /** Graph: the link in the side panel, as `from>to` (paths). */
  link?: string;
  /** Graph: a pseudo-box or folded row whose members the side panel lists. */
  box?: string;
  /** Violations: filters. */
  rule?: string;
  sev?: string;
  /** Graph: the violation whose cycle is highlighted. */
  cycle?: number;
}

export const DEFAULT_STATE: ViewState = {
  view: "graph",
  hops: 2,
  dir: "both",
  pkgs: true,
  types: true,
};

const VIEWS: View[] = ["graph", "violations"];
const DIRECTIONS: Direction[] = ["imports", "importers", "both"];

/** `hasHops` tells whether a focus path is a module: only then do hops and direction matter. */
export function formatHash(s: ViewState, hasHops: (focus: string) => boolean = () => true): string {
  const p = new URLSearchParams();
  p.set("view", s.view);
  if (s.hide?.length) p.set("hide", s.hide.join(","));
  if (s.focus) {
    p.set("focus", s.focus);
    if (hasHops(s.focus)) {
      p.set("hops", String(s.hops));
      p.set("dir", s.dir);
    }
  }
  if (!s.pkgs) p.set("pkgs", "0");
  if (!s.types) p.set("types", "0");
  if (s.sel) p.set("sel", s.sel);
  if (s.link) p.set("link", s.link);
  if (s.box) p.set("box", s.box);
  if (s.rule) p.set("rule", s.rule);
  if (s.sev) p.set("sev", s.sev);
  if (s.cycle !== undefined) p.set("cycle", String(s.cycle));
  return `#${p.toString()}`;
}

function ids(value: string | null): number[] | undefined {
  if (value === null) return undefined;
  return value.split(",").filter(Boolean).map(Number).filter(Number.isInteger);
}

export function parseHash(hash: string): ViewState {
  const p = new URLSearchParams(hash.replace(/^#/, ""));
  const raw = p.get("view");
  // the Focus tab went into the Graph tab, and the Treemap tab into its map: old links keep working
  const view = (raw === "focus" || raw === "treemap" ? "graph" : raw) as View;
  const dir = p.get("dir") as Direction;
  const hops = Number(p.get("hops"));
  const cycle = p.get("cycle");
  const hide = ids(p.get("hide"));
  return {
    view: VIEWS.includes(view) ? view : DEFAULT_STATE.view,
    hide: hide?.length ? hide : undefined,
    focus: p.get("focus") || (raw === "treemap" ? p.get("path") : null) || undefined,
    hops: hops >= 1 && hops <= 3 ? Math.floor(hops) : DEFAULT_STATE.hops,
    dir: DIRECTIONS.includes(dir) ? dir : DEFAULT_STATE.dir,
    pkgs: p.get("pkgs") !== "0",
    types: p.get("types") !== "0",
    sel: p.get("sel") || undefined,
    link: p.get("link") || undefined,
    box: p.get("box") || undefined,
    rule: p.get("rule") || undefined,
    sev: p.get("sev") || undefined,
    cycle: cycle !== null && Number.isInteger(Number(cycle)) ? Number(cycle) : undefined,
  };
}
