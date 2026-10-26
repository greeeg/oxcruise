// Runs every active plugin rule over the graph in one pass.

import { GraphFacade } from "./graph.ts";
import type { PluginRegistry } from "./load.ts";
import type { Dependency, Graph, Module, OptionsSchema, Report, RuleContext, RuleVisitors } from "./types.ts";

export interface RuleSpec {
  id: number;
  name: string;
  severity: string;
  options: unknown;
}

interface Violation {
  id: number;
  module?: number;
  edge?: number;
  message: string;
}

/** Options with the defaults from `meta.schema` filled in. */
export function withDefaults(schema: OptionsSchema | undefined, options: unknown): Record<string, unknown> {
  const given = options && typeof options === "object" && !Array.isArray(options) ? (options as Record<string, unknown>) : {};
  const out: Record<string, unknown> = {};
  for (const [key, def] of Object.entries(schema ?? {})) {
    if ("default" in def) out[key] = def.default;
  }
  return { ...out, ...given };
}

interface Active {
  spec: RuleSpec;
  visitors: RuleVisitors;
}

/**
 * Returns JSON: `null` when no rule reported anything, otherwise `[{ id, module | edge, message }]`.
 * One call per run, with the finished graph.
 */
export function runRules(buffer: Uint8Array, specsJson: string, registry: PluginRegistry): string {
  const graph: Graph = new GraphFacade(buffer);
  const specs = JSON.parse(specsJson) as RuleSpec[];
  const violations: Violation[] = [];

  const active: Active[] = specs.map((spec) => {
    const { rule } = registry.rule(spec.name);
    const context: RuleContext = {
      options: withDefaults(rule.meta?.schema, spec.options),
      graph,
      ruleName: spec.name,
      report(report: Report) {
        const hasModule = "module" in report && report.module !== undefined;
        const hasDependency = "dependency" in report && report.dependency !== undefined;
        if (hasModule === hasDependency) {
          throw new Error(`rule \`${spec.name}\`: report() needs exactly one of \`module\` or \`dependency\``);
        }
        if (typeof report.message !== "string" || report.message === "") {
          throw new Error(`rule \`${spec.name}\`: report() needs a message`);
        }
        violations.push(
          hasModule
            ? { id: spec.id, module: (report as { module: Module }).module.index, message: report.message }
            : { id: spec.id, edge: (report as { dependency: Dependency }).dependency.index, message: report.message },
        );
      },
    };
    return { spec, visitors: guard(spec.name, () => rule.create(context)) };
  });

  const each = <K extends keyof RuleVisitors>(hook: K) =>
    active.filter((a) => typeof a.visitors[hook] === "function");

  const run = (name: string, fn: () => void) => guard(name, fn);

  for (const a of each("before")) run(a.spec.name, () => a.visitors.before!(graph));

  const onModule = each("module");
  if (onModule.length) {
    for (const m of graph.modules) for (const a of onModule) run(a.spec.name, () => a.visitors.module!(m));
  }
  const onDependency = each("dependency");
  if (onDependency.length) {
    for (const d of graph.edges) for (const a of onDependency) run(a.spec.name, () => a.visitors.dependency!(d));
  }
  for (const a of each("graph")) run(a.spec.name, () => a.visitors.graph!(graph));
  for (const a of each("after")) run(a.spec.name, () => a.visitors.after!(graph));

  return JSON.stringify(violations.length ? violations : null);
}

function guard<T>(rule: string, fn: () => T): T {
  try {
    return fn();
  } catch (e) {
    throw new Error(`plugin rule \`${rule}\` threw: ${e instanceof Error ? e.message : String(e)}`, { cause: e });
  }
}
