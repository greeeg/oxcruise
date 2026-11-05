// Public types of the plugin API (`oxcruise/plugin`). Written by hand; the DepKind union is generated.

import type { DepKind } from "./kinds.generated.ts";

export type { DepKind };

/**
 * What a module is, with the names the config's `to.kind` uses: `local` is a file of the project,
 * `package` a file inside `node_modules` (resolved, not parsed), `builtin` a Node.js built-in and
 * `unresolved` an import that could not be resolved.
 */
export type ModuleKind = "local" | "package" | "builtin" | "unresolved";

export interface Loc {
  /** 1-based. */
  line: number;
  /** 1-based, in bytes. */
  col: number;
}

/** A node of the module graph: a project file, an installed package file, a built-in or an unresolved specifier. */
export interface Module {
  /** Index in `graph.modules`. */
  readonly index: number;
  /** Path relative to the cruise root, `/`-separated. For builtin and unresolved modules, the specifier. */
  readonly path: string;
  readonly kind: ModuleKind;
  /** Workspace package this module belongs to. */
  readonly package: string | undefined;
  /** Kept in the graph but its dependencies were not explored (`doNotFollow`, depth limit, node_modules). */
  readonly notFollowed: boolean;
  /** Dependencies of this module, sorted by target path, then specifier. */
  readonly dependencies: readonly Dependency[];
  /** Distinct modules that depend on this one. */
  readonly dependents: readonly Module[];
  readonly dependentCount: number;
  /** What the module exports, sorted by name. Empty when exports were not extracted (see `Graph.symbols`). */
  readonly exports: readonly Export[];
  is(kind: ModuleKind): boolean;
}

export type ExportKind = "local" | "reexport" | "star" | "namespace";

/** One export of a module. */
export interface Export {
  /** The module that declares it. */
  readonly module: Module;
  /** The name importers use: `default` for the default export, `*` for `export *`. */
  readonly name: string;
  readonly kind: ExportKind;
  /** A type or interface. */
  readonly isType: boolean;
  /**
   * Some module imports it, or it is part of an entry point's interface. Entry points are the
   * modules nothing imports, the files `package.json` publishes, the files the tools in
   * `package.json` load by convention, and plugin `entries`. It is computed with the default
   * options of `unused-exports` (it does not look at enum members, duplicate exports or a rule's
   * `entries`, `production` and `conventions`), so the rule is the authoritative report.
   */
  readonly used: boolean;
  /**
   * The members read as `Export.member` (`E.A` for an enum, `ns.a` for a namespace): `"*"` when the
   * export is used whole or by an entry point, so any member may be read; otherwise the names, sorted
   * (empty when nothing uses it).
   */
  readonly usedMembers: "*" | readonly string[];
  readonly loc: Loc | undefined;
  /** Module a re-export reads from. */
  readonly from: Module | undefined;
  /** Name read from `from`: `a` in `export { a as b } from "x"`. */
  readonly imported: string | undefined;
}

/** One import, export, require... of `from` pointing at `to`. */
export interface Dependency {
  /** Index in `graph.edges`; stable within a run. */
  readonly index: number;
  readonly from: Module;
  readonly to: Module;
  /** The specifier as written in the source. */
  readonly specifier: string;
  readonly kinds: readonly DepKind[];
  readonly loc: Loc | undefined;
  readonly typeOnly: boolean;
  /**
   * On a cycle of the whole graph, type-only edges included; `circular-dependencies` ignores type-only
   * edges by default (`includeTypeOnly`).
   */
  readonly circular: boolean;
  readonly unresolved: boolean;
  /**
   * What `from` uses of `to`: `"*"` for anything (a `require`, a dynamic `import()`, a namespace
   * passed around), otherwise export names. `E.A` is the member `A` of export `E`; `ns.a` the
   * export `a` read through the namespace import `ns`. Empty for a bare `import "x"`.
   */
  readonly names: "*" | readonly string[];
  /** Shortest cycle through this dependency, starting at `from`; undefined if not circular. */
  readonly cycle: readonly Module[] | undefined;
  is(kind: DepKind): boolean;
}

export interface Graph {
  readonly modules: readonly Module[];
  readonly edges: readonly Dependency[];
  /** Exports and used names were extracted, so `Module.exports`, `Dependency.names` and `importersOf` are meaningful. */
  readonly symbols: boolean;
  /**
   * Modules that use export `name` of `module`, directly or through `export { name } from`,
   * `export *` and `export * as ns`. Throws when exports were not extracted.
   */
  importersOf(module: Module, name: string): Module[];
  /** Workspace package names. */
  readonly packages: readonly string[];
  /** Distinct folders that contain at least one source module. */
  readonly folders: readonly string[];
  module(path: string): Module | undefined;
  /** Strongly connected components with more than one module (or a self-import). */
  cycles(): Module[][];
  /** Everything reachable from `from` through dependencies, `from` excluded. */
  reach(from: Module | readonly Module[]): Module[];
}

/** Report on a module, one of its dependencies, or one of its exports (needs `needs: ["symbols"]`). */
export type Report = { message: string } & (
  | { module: Module }
  | { dependency: Dependency }
  | { export: Export }
);

export interface RuleContext<Options = Record<string, unknown>> {
  /** Configured options with the defaults from `meta.schema` applied. */
  readonly options: Options;
  readonly graph: Graph;
  /** Fully qualified rule name, e.g. `acme/max-fan-out`. */
  readonly ruleName: string;
  /** The level the rule is configured with; what it reports gets this severity. */
  readonly severity: "error" | "warn";
  report(report: Report): void;
}

export interface RuleVisitors {
  before?(graph: Graph): void;
  module?(module: Module): void;
  dependency?(dependency: Dependency): void;
  graph?(graph: Graph): void;
  after?(graph: Graph): void;
}

/**
 * `{ max: { type: "number", default: 15 } }`: the options a rule takes, their `typeof` (`array` for
 * lists) and defaults. Options that are not listed, or of another type, are an error.
 */
export type OptionsSchema = Record<
  string,
  { type?: string; default?: unknown; description?: string }
>;

export interface RuleDefinition<Options = Record<string, unknown>> {
  meta?: { docs?: string; schema?: OptionsSchema };
  create(context: RuleContext<Options>): RuleVisitors;
}

/** The native `json` result, as passed to plugin reporters. */
export interface CruiseResultJson {
  version: number;
  modules: ModuleJson[];
  violations: ViolationJson[];
  summary: {
    error: number;
    warn: number;
    suppressed: number;
    /** The entries of `modules`; unresolved specifiers are not counted. */
    totalModules: number;
    totalDependencies: number;
  };
  warnings: string[];
}

export interface ModuleJson {
  path: string;
  kind: ModuleKind;
  package?: string;
  notFollowed: boolean;
  orphan: boolean;
  dependencies: DependencyJson[];
  /** Modules that import this one; a module that imports itself is left out. */
  dependents: string[];
  parseError?: string;
  /** Present when exports were extracted. */
  exports?: ExportJson[];
}

export interface ExportJson {
  name: string;
  kind: ExportKind;
  isType?: boolean;
  line: number;
  col: number;
  used: boolean;
}

export interface DependencyJson {
  to: string;
  toKind: ModuleKind;
  specifier: string;
  kinds: DepKind[];
  moduleSystem: string;
  line?: number;
  col?: number;
  /**
   * On a cycle of the whole graph, type-only edges included; `circular-dependencies` ignores type-only
   * edges by default (`includeTypeOnly`).
   */
  circular: boolean;
  /** The cycle this dependency is part of, starting here; empty when it is not circular. */
  cycle?: CycleHopJson[];
  /** The rules that reported this dependency. */
  rules?: RuleRefJson[];
  /** Present when exports were extracted: `"*"` or the names used. */
  names?: "*" | string[];
}

export interface CycleHopJson {
  module: string;
  /** Kinds of the dependency that leaves this module towards the next hop. */
  kinds: DepKind[];
}

export interface RuleRefJson {
  name: string;
  severity: "error" | "warn";
}

export interface ViolationJson {
  rule: string;
  severity: "error" | "warn";
  /**
   * `export`: an export of the module `from`. `file`: a file that is not a module (a
   * `package.json`), and `from` is its path.
   */
  kind: "dependency" | "module" | "cycle" | "reachability" | "export" | "file";
  from: string;
  to?: string;
  line?: number;
  col?: number;
  message: string;
  /** The modules of the cycle (`cycle`) or of the path to the target (`reachability`). */
  cycle?: CycleHopJson[];
  /** Length of the specifier as written, quotes included. */
  specifierLen?: number;
}

export type ReporterFunction = (result: CruiseResultJson) => string | Promise<string>;

export interface PluginDefinition {
  meta: { name: string };
  rules?: Record<string, RuleDefinition<any>>;
  reporters?: Record<string, ReporterFunction>;
  /**
   * Globs (relative to the cruise root) of modules the tools you use load without anyone importing
   * them, e.g. `["**\/*.stories.tsx", "app/routes/**"]`. They are entry points for `unused-exports`,
   * `unused-modules` and `Export.used`.
   */
  entries?: string[];
  /**
   * What the plugin reads beyond the module graph. `"symbols"` extracts the exports of every module
   * and the names each import uses (`Module.exports`, `Dependency.names`, `Graph.importersOf`),
   * which costs an extra walk over every file.
   */
  needs?: "symbols"[];
}
