import type { PluginDefinition } from "../../../src-js/plugins/types.ts";

const plugin = {
  meta: { name: "acme" },
  rules: {
    // module visitor + options with defaults
    "max-fan-out": {
      meta: { docs: "Limit the number of local dependencies of a module", schema: { max: { type: "number", default: 15 } } },
      create(ctx) {
        return {
          module(m) {
            const n = m.dependencies.filter((d) => d.is("local")).length;
            if (n > (ctx.options.max as number)) {
              ctx.report({ module: m, message: `fan-out ${n} is greater than ${ctx.options.max}` });
            }
          },
        };
      },
    },
    // dependency visitor: reports on the import itself, so it gets a line and column
    "no-direct-db": {
      create(ctx) {
        return {
          dependency(d) {
            if (d.from.path.startsWith("src/ui/") && d.to.path.startsWith("src/db/")) {
              ctx.report({ dependency: d, message: `UI must not import the database layer (${d.to.path})` });
            }
          },
        };
      },
    },
    // graph visitor + before/after hooks
    "cycle-budget": {
      meta: { schema: { max: { type: "number", default: 0 } } },
      create(ctx) {
        let seen = 0;
        return {
          before() {
            seen = 0;
          },
          graph(g) {
            seen = g.cycles().length;
          },
          after(g) {
            if (seen > (ctx.options.max as number)) {
              const m = g.cycles()[0]![0]!;
              ctx.report({ module: m, message: `${seen} cycle(s) exceed the budget of ${ctx.options.max}` });
            }
          },
        };
      },
    },
  },
  reporters: {
    "json-lines": (result) =>
      result.violations.map((v) => JSON.stringify({ rule: v.rule, severity: v.severity, file: v.from, line: v.line ?? null })).join("\n") + "\n",
  },
} satisfies PluginDefinition;

export default plugin;
