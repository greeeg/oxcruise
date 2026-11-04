import type { ViewState } from "./state";

/** How views ask the app to change the state (and with it the URL). */
export interface Nav {
  state(): ViewState;
  /** Merge `patch` into the state. `replace` rewrites the history entry instead of adding one. */
  go(patch: Partial<ViewState>, replace?: boolean): void;
  /**
   * Show the graph limited to a folder or module (a node id), or to the whole project for `null`.
   * A folder shows its map, a module its neighbourhood. It adds a history entry.
   */
  focus(node: number | null, extra?: Partial<ViewState>): void;
  /** Say something short, with an optional button. */
  toast(text: string, action?: { label: string; run: () => void }): void;
}
