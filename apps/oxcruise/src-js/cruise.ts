// Programmatic API: run a cruise and ask questions about the result, e.g. in an architecture test.
//
//   const g = await cruise({ cwd, paths: ["src"] });
//   expect(g.module("src/ui/x.ts")?.dependsOn("src/db/**")).toBe(false);

import { runCruise } from "./bindings.js";
import { matchesGlob } from "./glob.ts";
import { createHost } from "./host.ts";
import { GraphFacade } from "./plugins/graph.ts";
import type {
  CruiseResultJson,
  Dependency,
  Graph,
  Module,
  ModuleKind,
  ViolationJson,
} from "./plugins/types.ts";

export interface CruiseOptions {
  /** Directory to run in (default: the current directory). */
  cwd?: string;
  /** Entries; default: the config's `entries`, or `.` */
  paths?: string[];
  /** Config file; default: discovered in `cwd`. */
  config?: string;
  /** Globs of modules to leave out of the graph, on top of the config's `ignorePatterns`. */
  ignorePatterns?: string[];
  maxDepth?: number;
  threads?: number;
}

/** What `runCruise` returns next to the graph buffer. */
interface Report {
  violations: ViolationJson[];
  summary: CruiseResultJson["summary"];
}

export class ModuleHandle {
  private readonly result: CruiseResult;
  /** The module in the graph, the same object plugin rules get. */
  readonly module: Module;
  constructor(result: CruiseResult, module: Module) {
    this.result = result;
    this.module = module;
  }

  get path(): string {
    return this.module.path;
  }
  get kind(): ModuleKind {
    return this.module.kind;
  }
  get dependencies(): readonly Dependency[] {
    return this.module.dependencies;
  }
  get dependents(): readonly Module[] {
    return this.module.dependents;
  }
  get violations(): readonly ViolationJson[] {
    return this.result.violationsOf(this.path);
  }

  /** Does this module import a module whose path matches `target` (a path or glob)? */
  dependsOn(target: string): boolean {
    return this.module.dependencies.some((d) => matchesGlob(target, d.to.path));
  }

  /** Does anything matching `target` get imported by this module, directly or through other modules? */
  dependsOnTransitively(target: string): boolean {
    const seen = new Set<number>([this.module.index]);
    const queue = [this.module];
    for (let head = 0; head < queue.length; head++) {
      for (const d of queue[head]!.dependencies) {
        if (matchesGlob(target, d.to.path)) return true;
        if (!seen.has(d.to.index)) {
          seen.add(d.to.index);
          queue.push(d.to);
        }
      }
    }
    return false;
  }
}

export class CruiseResult {
  /** The whole graph, as plugin rules see it. */
  readonly graph: Graph;
  readonly violations: readonly ViolationJson[];
  readonly summary: CruiseResultJson["summary"];
  private readonly handles: (ModuleHandle | undefined)[];
  private all: readonly ModuleHandle[] | undefined;
  private byModule: Map<string, ViolationJson[]> | undefined;

  constructor(graph: Graph, report: Report) {
    this.graph = graph;
    this.violations = report.violations;
    this.summary = report.summary;
    this.handles = new Array(graph.modules.length);
  }

  /** Every module except unresolved specifiers; the handles are built once. */
  get modules(): readonly ModuleHandle[] {
    return (this.all ??= this.graph.modules
      .filter((m) => m.kind !== "unresolved")
      .map((m) => this.handle(m)));
  }

  module(path: string): ModuleHandle | undefined {
    const m = this.graph.module(path);
    return m && this.handle(m);
  }

  /** Source modules whose path matches the glob. */
  modulesMatching(glob: string): ModuleHandle[] {
    return this.modules.filter((m) => m.kind === "source" && matchesGlob(glob, m.path));
  }

  /** Violations reported on `path`. */
  violationsOf(path: string): readonly ViolationJson[] {
    if (!this.byModule) {
      this.byModule = new Map();
      for (const v of this.violations) {
        const list = this.byModule.get(v.from);
        if (list) list.push(v);
        else this.byModule.set(v.from, [v]);
      }
    }
    return this.byModule.get(path) ?? [];
  }

  private handle(m: Module): ModuleHandle {
    return (this.handles[m.index] ??= new ModuleHandle(this, m));
  }
}

export async function cruise(options: CruiseOptions = {}): Promise<CruiseResult> {
  const args: string[] = [];
  const flag = (name: string, values: readonly string[] | undefined) =>
    values?.forEach((v) => args.push(name, v));
  if (options.cwd) args.push("--cwd", options.cwd);
  if (options.config) args.push("--config", options.config);
  if (options.maxDepth !== undefined) args.push("--max-depth", String(options.maxDepth));
  if (options.threads !== undefined) args.push("--threads", String(options.threads));
  flag("--ignore-pattern", options.ignorePatterns);
  args.push(...(options.paths ?? []));

  const host = createHost();
  const outcome = await runCruise(
    args,
    (r) => host.loadPlugin(r),
    (r) => host.runRules(r),
    (r) => host.renderReport(r),
    (p) => host.loadJsConfig(p),
  );
  // Findings are a result; a run that could not produce a graph is an error.
  if (!outcome.graph || !outcome.report) {
    throw new Error(outcome.stderr.trim() || `oxcruise exited with code ${outcome.exitCode}`);
  }
  return new CruiseResult(new GraphFacade(outcome.graph), JSON.parse(outcome.report) as Report);
}
