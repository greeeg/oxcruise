// Programmatic API: run a cruise and ask questions about the result, e.g. in an architecture test.
//
//   const g = await cruise({ cwd, paths: ["src"] });
//   expect(g.module("src/ui/x.ts")?.dependsOn("src/db/**")).toBe(false);

import { runCaptured } from "./bindings.js";
import { matchesGlob } from "./glob.ts";
import { createHost } from "./host.ts";
import type { CruiseResultJson, DependencyJson, ModuleJson, ViolationJson } from "./plugins/types.ts";

export interface CruiseOptions {
  /** Directory to run in (default: the current directory). */
  cwd?: string;
  /** Entries; default: the config's `entries`, or `.` */
  paths?: string[];
  /** Config file; default: discovered in `cwd`. */
  config?: string;
  /** Ignore config files and use the recommended preset. */
  noConfig?: boolean;
  exclude?: string[];
  includeOnly?: string[];
  doNotFollow?: string[];
  maxDepth?: number;
  threads?: number;
}

export class ModuleHandle {
  private readonly result: CruiseResult;
  readonly data: ModuleJson;
  constructor(result: CruiseResult, data: ModuleJson) {
    this.result = result;
    this.data = data;
  }

  get path(): string {
    return this.data.path;
  }
  get kind(): ModuleJson["kind"] {
    return this.data.kind;
  }
  get dependencies(): readonly DependencyJson[] {
    return this.data.dependencies;
  }
  get dependents(): readonly string[] {
    return this.data.dependents;
  }
  get violations(): ViolationJson[] {
    return this.result.violations.filter((v) => v.from === this.path);
  }

  /** Does this module import a module whose path matches `target` (a path or glob)? */
  dependsOn(target: string): boolean {
    return this.data.dependencies.some((d) => matchesGlob(target, d.to));
  }

  /** Does anything matching `target` get imported by this module, directly or through other modules? */
  dependsOnTransitively(target: string): boolean {
    const seen = new Set<string>([this.path]);
    const queue = [this.path];
    for (let head = 0; head < queue.length; head++) {
      const m = this.result.module(queue[head]!);
      for (const d of m?.dependencies ?? []) {
        if (matchesGlob(target, d.to)) return true;
        if (!seen.has(d.to)) {
          seen.add(d.to);
          queue.push(d.to);
        }
      }
    }
    return false;
  }
}

export class CruiseResult {
  private readonly byPath: Map<string, ModuleJson>;
  readonly raw: CruiseResultJson;
  constructor(raw: CruiseResultJson) {
    this.raw = raw;
    this.byPath = new Map(raw.modules.map((m) => [m.path, m]));
  }

  get violations(): ViolationJson[] {
    return this.raw.violations;
  }
  get summary(): CruiseResultJson["summary"] {
    return this.raw.summary;
  }
  get modules(): ModuleHandle[] {
    return this.raw.modules.map((m) => new ModuleHandle(this, m));
  }

  module(path: string): ModuleHandle | undefined {
    const m = this.byPath.get(path);
    return m && new ModuleHandle(this, m);
  }

  /** Source modules whose path matches the glob. */
  modulesMatching(glob: string): ModuleHandle[] {
    return this.modules.filter((m) => m.kind === "source" && matchesGlob(glob, m.path));
  }
}

export async function cruise(options: CruiseOptions = {}): Promise<CruiseResult> {
  const args = ["--format", "json", "--color", "never"];
  const flag = (name: string, values: readonly string[] | undefined) => values?.forEach((v) => args.push(name, v));
  if (options.cwd) args.push("--cwd", options.cwd);
  if (options.config) args.push("--config", options.config);
  if (options.noConfig) args.push("--no-config");
  if (options.maxDepth !== undefined) args.push("--max-depth", String(options.maxDepth));
  if (options.threads !== undefined) args.push("--threads", String(options.threads));
  flag("--exclude", options.exclude);
  flag("--include-only", options.includeOnly);
  flag("--do-not-follow", options.doNotFollow);
  args.push(...(options.paths ?? []));

  const host = createHost();
  const outcome = await runCaptured(
    args,
    (r) => host.loadPlugin(r),
    (r) => host.runRules(r),
    (r) => host.renderReport(r),
    (p) => host.loadJsConfig(p),
  );
  // 1 means "found errors", which is a result; 2 means the run itself failed.
  if (outcome.exitCode > 1 && !outcome.stdout.trimStart().startsWith("{")) {
    throw new Error(outcome.stderr.trim() || `oxcruise exited with code ${outcome.exitCode}`);
  }
  return new CruiseResult(JSON.parse(outcome.stdout) as CruiseResultJson);
}
