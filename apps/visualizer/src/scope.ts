import type { Model } from "./model";
import type { ViewState } from "./state";

/** What the focus of a view state limits the graph to. */
export interface ScopeInfo {
  /** The focused folder or module; `undefined` for the whole project. */
  focus?: number;
  /** The violation cycle to show, with a module focus. */
  cycle: number[];
}

export function scopeInfo(model: Model, s: ViewState): ScopeInfo {
  const node = s.focus ? model.nodeByPath(s.focus) : -1;
  const focus = node > 0 ? node : undefined;
  if (focus === undefined || model.isDir(focus)) return { focus, cycle: [] };
  const violation = s.cycle !== undefined ? model.data.violations[s.cycle] : undefined;
  return { focus, cycle: violation?.cycle ?? [] };
}

/** The state change that limits the graph to `node` (a folder or module), or to the project for `null`. */
export function focusPatch(
  model: Model,
  node: number | null,
  extra: Partial<ViewState> = {},
): Partial<ViewState> {
  if (node === null || node <= 0) {
    return {
      view: "graph",
      focus: undefined,
      cycle: undefined,
      link: undefined,
      box: undefined,
      ...extra,
    };
  }
  const path = model.path(node);
  return {
    view: "graph",
    focus: path,
    sel: path,
    link: undefined,
    box: undefined,
    cycle: undefined,
    ...extra,
  };
}
