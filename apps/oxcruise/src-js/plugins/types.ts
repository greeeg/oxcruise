// Public types of the plugin API (`oxcruise/plugin`). Written by hand; the DepKind union is generated.

import type { DepKind } from "./kinds.generated.ts";

export type { DepKind };

export type ModuleKind = "source" | "stub" | "core" | "unresolved";

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
  /** Path relative to the cruise root, `/`-separated. For core and unresolved modules, the specifier. */
  readonly path: string;
  readonly kind: ModuleKind;
  /** Workspace package this module belongs to. */
  readonly package: string | undefined;
  /** Kept in the graph but its dependencies were not explored (`doNotFollow`, depth limit, npm). */
  readonly notFollowed: boolean;
  /** Dependencies of this module, in source order. */
  readonly dependencies: readonly Dependency[];
  /** Distinct modules that depend on this one. */
  readonly dependents: readonly Module[];
  readonly dependentCount: number;
  is(kind: ModuleKind): boolean;
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
  /** On a cycle of the whole graph (type-only edges included). */
  readonly circular: boolean;
  readonly unresolved: boolean;
  /** Shortest cycle through this dependency, starting at `from`; undefined if not circular. */
  readonly cycle: readonly Module[] | undefined;
  is(kind: DepKind): boolean;
}

export interface Graph {
  readonly modules: readonly Module[];
  readonly edges: readonly Dependency[];
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

export type Report = { message: string } & ({ module: Module } | { dependency: Dependency });

export interface RuleContext<Options = Record<string, unknown>> {
  /** Configured options with the defaults from `meta.schema` applied. */
  readonly options: Options;
  readonly graph: Graph;
  /** Fully qualified rule name, e.g. `acme/max-fan-out`. */
  readonly ruleName: string;
  report(report: Report): void;
}

export interface RuleVisitors {
  before?(graph: Graph): void;
  module?(module: Module): void;
  dependency?(dependency: Dependency): void;
  graph?(graph: Graph): void;
  after?(graph: Graph): void;
}

/** `{ max: { type: "number", default: 15 } }`: the options a rule takes and their defaults. */
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
    info: number;
    suppressed: number;
    total_modules: number;
    total_dependencies: number;
  };
  warnings: string[];
}

export interface ModuleJson {
  path: string;
  kind: ModuleKind;
  package?: string;
  not_followed: boolean;
  orphan: boolean;
  dependencies: DependencyJson[];
  dependents: string[];
}

export interface DependencyJson {
  to: string;
  to_kind: ModuleKind;
  specifier: string;
  kinds: DepKind[];
  module_system: string;
  line?: number;
  col?: number;
  circular: boolean;
}

export interface ViolationJson {
  rule: string;
  severity: "error" | "warn" | "info" | "ignore";
  kind: "dependency" | "module" | "cycle" | "reachability";
  from: string;
  to?: string;
  line?: number;
  col?: number;
  message: string;
}

export type ReporterFunction = (result: CruiseResultJson) => string | Promise<string>;

export interface PluginDefinition {
  meta: { name: string };
  rules?: Record<string, RuleDefinition<any>>;
  reporters?: Record<string, ReporterFunction>;
}
