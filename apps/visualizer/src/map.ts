import { aggregate, boundaryDeps, modulesUnder } from "./logic";
import type { Link } from "./logic";
import type { Model } from "./model";
import { EDGE_TYPE_ONLY } from "./types";

/** A map never draws more boxes than this: the rest is merged into a pseudo-box. */
export const MAX_BOXES = 40;
/** Ports shown on each side of the frame; the rest folds into `+N`. */
export const PORT_LIMIT = 8;
/** Packages named in the lane; the rest folds into `+N`. */
export const PACKAGE_LIMIT = 14;

export interface MapOptions {
  /** Folders and modules (node ids) that are not drawn. */
  hidden?: ReadonlySet<number>;
  /** Count the type-only imports. */
  types?: boolean;
  /** Fill the packages lane. */
  pkgs?: boolean;
}

export type BoxKind = "dir" | "mod" | "files" | "folders";

export interface MapBox {
  kind: BoxKind;
  /** The folder or module (a node id); -1 for a pseudo-box. */
  node: number;
  label: string;
  modules: number;
  errors: number;
  violations: number;
  /** The nodes the box stands for: itself, or the many that a pseudo-box merges. */
  members: number[];
}

export interface MapPort {
  /** The folder or module outside the scope; -1 for the folded `+N` port. */
  node: number;
  label: string;
  /** Imports across the frame. */
  count: number;
  /** Which boxes the imports touch, and how many each. */
  boxes: { box: number; count: number }[];
  /** For the folded port: what it stands for. */
  nodes?: number[];
}

export interface PackageChip {
  name: string;
  count: number;
  /** A module of the package, to select. */
  module: number;
  /** The files of the package that are imported, most imported first. */
  files: string[];
}

export interface MapScope {
  /** The folder the map shows, after skipping folders that hold only one folder. */
  folder: number;
  boxes: MapBox[];
  links: Link[];
  /** What the scope uses, and what uses it, outside of it. */
  portsOut: MapPort[];
  portsIn: MapPort[];
  packages: PackageChip[];
  /** Packages in total, and imports of them. */
  packageTotal: number;
  packageImports: number;
}

/** The `(external)` folder, or -1. */
export function externalDir(model: Model): number {
  for (const d of model.childDirs[0]) if (model.data.dirs.name[d] === "(external)") return d;
  return -1;
}

function hiddenModules(model: Model, hidden: ReadonlySet<number> | undefined): Uint8Array | null {
  if (!hidden || hidden.size === 0) return null;
  const out = new Uint8Array(model.nMods);
  for (const n of hidden) {
    if (model.isDir(n)) for (const m of modulesUnder(model, n)) out[m] = 1;
    else out[model.modOf(n)] = 1;
  }
  return out;
}

/** The folders (not `(external)`, not hidden) and modules (not hidden) directly in `d`. */
function children(model: Model, d: number, hidden: ReadonlySet<number> | undefined) {
  const external = d === 0 ? externalDir(model) : -1;
  const keep = (n: number) => !hidden?.has(n);
  return {
    dirs: model.childDirs[d].filter((c) => c !== external && keep(c)),
    mods: model.childMods[d].filter((m) => keep(model.nodeOfMod(m))),
  };
}

/** The folder a map of `folder` shows: it skips a folder that holds one folder and no files. */
export function resolveFolder(model: Model, folder: number, hidden?: ReadonlySet<number>): number {
  let d = folder;
  for (;;) {
    const c = children(model, d, hidden);
    if (c.dirs.length !== 1 || c.mods.length > 0) return d;
    d = c.dirs[0];
  }
}

/** `a/b/c` for a folder that holds only one folder, down to the end of the chain. Gives the box its label. */
function chainLabel(model: Model, d: number, hidden: ReadonlySet<number> | undefined): string {
  const names = [model.name(d)];
  for (let x = d; ;) {
    const c = children(model, x, hidden);
    if (c.dirs.length !== 1 || c.mods.length > 0) break;
    x = c.dirs[0];
    names.push(model.name(x));
  }
  return names.join("/");
}

function nodeBox(model: Model, node: number, label: string): MapBox {
  if (model.isDir(node)) {
    return {
      kind: "dir",
      node,
      label,
      modules: model.dirModules[node],
      errors: model.dirErrors[node],
      violations: model.dirViolations[node],
      members: [node],
    };
  }
  const m = model.modOf(node);
  return {
    kind: "mod",
    node,
    label,
    modules: 1,
    errors: model.modErrors[m],
    violations: model.modViolations[m],
    members: [node],
  };
}

function merged(kind: "files" | "folders", boxes: MapBox[]): MapBox {
  const sum = (f: (b: MapBox) => number) => boxes.reduce((s, b) => s + f(b), 0);
  const modules = sum((b) => b.modules);
  return {
    kind,
    node: -1,
    label:
      kind === "files" ? `files (${modules})` : `+${boxes.length} folders (${modules} modules)`,
    modules,
    errors: sum((b) => b.errors),
    violations: sum((b) => b.violations),
    members: boxes.flatMap((b) => b.members),
  };
}

/** The boxes of a folder: its folders and modules, merged into pseudo-boxes when there are too many. */
export function mapBoxes(
  model: Model,
  folder: number,
  hidden?: ReadonlySet<number>,
  max = MAX_BOXES,
): MapBox[] {
  const c = children(model, folder, hidden);
  const dirs = c.dirs
    .map((d) => nodeBox(model, d, chainLabel(model, d, hidden)))
    .sort((a, b) => (a.label < b.label ? -1 : a.label > b.label ? 1 : 0));
  let mods = c.mods.map((m) => nodeBox(model, model.nodeOfMod(m), model.data.modules.name[m]));
  mods.sort((a, b) => (a.label < b.label ? -1 : a.label > b.label ? 1 : 0));
  let folders = dirs;
  const extra: MapBox[] = [];
  if (folders.length + mods.length > max && mods.length >= 2) {
    extra.push(merged("files", mods));
    mods = [];
  }
  const room = max - mods.length - extra.length - 1;
  if (folders.length + mods.length + extra.length > max) {
    const bySize = [...folders].sort(
      (a, b) => b.modules - a.modules || (a.label < b.label ? -1 : a.label > b.label ? 1 : 0),
    );
    const keep = new Set(bySize.slice(0, room));
    extra.push(merged("folders", bySize.slice(room)));
    folders = folders.filter((b) => keep.has(b));
  }
  return [...folders, ...mods, ...extra];
}

/** Links with fewer imports than the returned number are weak. `limit` links are kept at most (ties stay). */
export function weakThreshold(links: readonly Link[], limit = 150): number {
  if (links.length <= limit) return 1;
  const counts = links.map((l) => l.count).sort((a, b) => b - a);
  return counts[limit - 1];
}

export function mapScope(model: Model, requested: number, opts: MapOptions = {}): MapScope {
  const { hidden, types = true, pkgs = true } = opts;
  const folder = resolveFolder(model, requested, hidden);
  const boxes = mapBoxes(model, folder, hidden);
  const hiddenMod = hiddenModules(model, hidden);

  // module -> box in O(folders + modules): walk each box's subtree once
  const dirBox = new Int32Array(model.nDirs).fill(-1);
  const modBox = new Int32Array(model.nMods).fill(-1);
  boxes.forEach((box, i) => {
    for (const n of box.members) {
      if (!model.isDir(n)) {
        modBox[model.modOf(n)] = i;
        continue;
      }
      const stack = [n];
      while (stack.length > 0) {
        const d = stack.pop()!;
        dirBox[d] = i;
        stack.push(...model.childDirs[d]);
      }
    }
  });
  const boxOf = (m: number): number => {
    if (hiddenMod?.[m]) return -1;
    const d = model.modDir[m];
    return d === folder ? modBox[m] : dirBox[d];
  };

  const { kind, package: pkgOf, name } = model.data.modules;
  const inner: number[] = [];
  const pkgByName = new Map<string, PackageChip>();
  const fileCounts = new Map<string, Map<string, number>>();
  let packageImports = 0;
  for (const m of modulesUnder(model, folder)) {
    if (kind[m] !== 0 || boxOf(m) < 0) continue;
    for (let i = model.outStart[m]; i < model.outStart[m + 1]; i++) {
      const e = model.outEdge[i];
      if (!types && model.edgeFlags[e] & EDGE_TYPE_ONLY) continue;
      const t = model.edgeTo[e];
      if (kind[t] === 0) {
        inner.push(e);
        continue;
      }
      if (!pkgs || hiddenMod?.[t]) continue;
      const label = pkgOf[t] >= 0 ? model.data.packages[pkgOf[t]] : name[t];
      const chip = pkgByName.get(label);
      if (chip) chip.count++;
      else pkgByName.set(label, { name: label, count: 1, module: t, files: [] });
      const file = model.path(model.nodeOfMod(t));
      const files = fileCounts.get(label) ?? new Map<string, number>();
      files.set(file, (files.get(file) ?? 0) + 1);
      fileCounts.set(label, files);
      packageImports++;
    }
  }
  const links = aggregate(model, inner, boxOf);
  for (const chip of pkgByName.values()) {
    chip.files = [...fileCounts.get(chip.name)!]
      .sort((a, b) => b[1] - a[1] || (a[0] < b[0] ? -1 : 1))
      .map(([file]) => file);
  }
  const packages = [...pkgByName.values()].sort(
    (a, b) => b.count - a.count || (a.name < b.name ? -1 : a.name > b.name ? 1 : 0),
  );

  const portsOut: MapPort[] = [];
  const portsIn: MapPort[] = [];
  if (folder > 0) {
    const external = externalDir(model);
    const nodeHidden = (n: number) => {
      if (!hidden || hidden.size === 0) return false;
      if (hidden.has(n)) return true;
      return model.ancestors(n).some((a) => hidden.has(a));
    };
    const deps = boundaryDeps(model, folder);
    const build = (
      groups: typeof deps.out,
      end: (e: number) => number,
      target: MapPort[],
    ): void => {
      const ports: MapPort[] = [];
      for (const g of groups) {
        const isPackage =
          g.node === external || (!model.isDir(g.node) && kind[model.modOf(g.node)] !== 0);
        if (isPackage || nodeHidden(g.node)) continue;
        const edges = types
          ? g.edges
          : g.edges.filter((e) => !(model.edgeFlags[e] & EDGE_TYPE_ONLY));
        if (edges.length === 0) continue;
        const byBox = new Map<number, number>();
        for (const e of edges) {
          const b = boxOf(end(e));
          if (b >= 0) byBox.set(b, (byBox.get(b) ?? 0) + 1);
        }
        ports.push({
          node: g.node,
          label: model.path(g.node),
          count: edges.length,
          boxes: [...byBox].map(([box, count]) => ({ box, count })).sort((a, b) => a.box - b.box),
        });
      }
      ports.sort((a, b) => b.count - a.count || a.node - b.node);
      target.push(...ports.slice(0, PORT_LIMIT));
      const rest = ports.slice(PORT_LIMIT);
      if (rest.length > 0) {
        const byBox = new Map<number, number>();
        for (const p of rest)
          for (const b of p.boxes) byBox.set(b.box, (byBox.get(b.box) ?? 0) + b.count);
        target.push({
          node: -1,
          label: `+${rest.length}`,
          count: rest.reduce((s, p) => s + p.count, 0),
          boxes: [...byBox].map(([box, count]) => ({ box, count })).sort((a, b) => a.box - b.box),
          nodes: rest.map((p) => p.node),
        });
      }
    };
    build(deps.out, (e) => model.edgeFrom[e], portsOut);
    build(deps.in, (e) => model.edgeTo[e], portsIn);
  }

  return {
    folder,
    boxes,
    links,
    portsOut,
    portsIn,
    packages: packages.slice(0, PACKAGE_LIMIT),
    packageTotal: packages.length,
    packageImports,
  };
}
