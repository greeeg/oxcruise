// Runs every active plugin rule over the graph in one pass.

import { GraphFacade } from "./graph.ts";
import type { PluginRegistry } from "./load.ts";
import type {
  Dependency,
  Graph,
  Module,
  OptionsSchema,
  Report,
  RuleContext,
  RuleVisitors,
} from "./types.ts";

export interface RuleSpec {
  id: number;
  name: string;
  severity: "error" | "warn";
  options: unknown;
}

interface Violation {
  id: number;
  module?: number;
  /** With `module`: the position of the export in `module.exports`. */
  export?: number;
  edge?: number;
  message: string;
}

function typeOf(value: unknown): string {
  return Array.isArray(value) ? "array" : value === null ? "null" : typeof value;
}

/**
 * Options with the defaults from `meta.schema` filled in. With a schema, like for built-in rules,
 * an unknown key or a value of the wrong `type` is an error that names the rule.
 */
export function withDefaults(
  schema: OptionsSchema | undefined,
  options: unknown,
  rule = "plugin rule",
): Record<string, unknown> {
  const given =
    options && typeof options === "object" && !Array.isArray(options)
      ? (options as Record<string, unknown>)
      : {};
  if (schema) {
    for (const [key, value] of Object.entries(given)) {
      const def = schema[key];
      if (!def) {
        const known = Object.keys(schema);
        throw new Error(
          `invalid options for \`${rule}\`: unknown option \`${key}\`` +
            (known.length
              ? ` (known: ${known.map((k) => `\`${k}\``).join(", ")})`
              : " (it takes none)"),
        );
      }
      if (def.type !== undefined && typeOf(value) !== def.type) {
        throw new Error(
          `invalid options for \`${rule}\`: option \`${key}\` must be of type ${def.type}, got ${typeOf(value)}`,
        );
      }
    }
  }
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
 * Returns JSON: `null` when no rule reported anything, otherwise `[{ id, module | module + export | edge, message }]`.
 * One call per run, with the finished graph.
 */
export function runRules(buffer: Uint8Array, specsJson: string, registry: PluginRegistry): string {
  const graph: Graph = new GraphFacade(buffer);
  const specs = JSON.parse(specsJson) as RuleSpec[];
  const violations: Violation[] = [];

  const active: Active[] = specs.map((spec) => {
    const { rule } = registry.rule(spec.name);
    const context: RuleContext = {
      options: withDefaults(rule.meta?.schema, spec.options, spec.name),
      graph,
      ruleName: spec.name,
      severity: spec.severity,
      report(report: Report) {
        const targets = (["module", "dependency", "export"] as const).filter(
          (k) => (report as Record<string, unknown>)[k] !== undefined,
        );
        if (targets.length !== 1) {
          throw new Error(
            `rule \`${spec.name}\`: report() needs exactly one of \`module\`, \`dependency\` or \`export\``,
          );
        }
        if (typeof report.message !== "string" || report.message === "") {
          throw new Error(`rule \`${spec.name}\`: report() needs a message`);
        }
        const { message } = report;
        if ("dependency" in report && report.dependency !== undefined) {
          violations.push({ id: spec.id, edge: report.dependency.index, message });
        } else if ("export" in report && report.export !== undefined) {
          const { module } = report.export;
          const index = module.exports.indexOf(report.export);
          if (index < 0) {
            throw new Error(`rule \`${spec.name}\`: report() got an export of another graph`);
          }
          violations.push({ id: spec.id, module: module.index, export: index, message });
        } else {
          violations.push({
            id: spec.id,
            module: (report as { module: Module }).module.index,
            message,
          });
        }
      },
    };
    return { spec, visitors: guard(spec.name, () => rule.create(context)) };
  });

  const each = <K extends keyof RuleVisitors>(hook: K) =>
    active.filter((a) => typeof a.visitors[hook] === "function");

  for (const a of each("before")) guard(a.spec.name, () => a.visitors.before!(graph));

  // one `try` per rule rather than per call; a rule's own visits keep their order
  for (const a of each("module")) {
    guard(a.spec.name, () => {
      for (const m of graph.modules) a.visitors.module!(m);
    });
  }
  for (const a of each("dependency")) {
    guard(a.spec.name, () => {
      for (const d of graph.edges) a.visitors.dependency!(d);
    });
  }
  for (const a of each("graph")) guard(a.spec.name, () => a.visitors.graph!(graph));
  for (const a of each("after")) guard(a.spec.name, () => a.visitors.after!(graph));

  return JSON.stringify(violations.length ? violations : null);
}

function guard<T>(rule: string, fn: () => T): T {
  try {
    return fn();
  } catch (e) {
    throw new Error(
      `plugin rule \`${rule}\` threw: ${e instanceof Error ? e.message : String(e)}`,
      { cause: e },
    );
  }
}
