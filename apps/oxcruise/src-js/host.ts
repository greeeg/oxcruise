// The four callbacks the native `run()` calls back into.

import { loadJsConfig } from "./config.ts";
import type { PluginRegistry } from "./plugins/load.ts";

export interface Host {
  loadPlugin(request: string): Promise<string>;
  runRules(request: { graph: Uint8Array; rules: string }): Promise<string>;
  renderReport(request: { name: string; resultJson: string }): Promise<string>;
  loadJsConfig(path: string): Promise<string>;
}

export function createHost(): Host {
  let runtime: Promise<typeof import("./plugins/index.ts")> | undefined;
  let registry: PluginRegistry | undefined;
  const plugins = async () => {
    runtime ??= import("./plugins/index.ts");
    const rt = await runtime;
    registry ??= new rt.PluginRegistry();
    return { rt, registry };
  };

  return {
    async loadPlugin(request) {
      const { rt, registry } = await plugins();
      return JSON.stringify(await rt.loadPlugin(JSON.parse(request), registry));
    },
    async runRules({ graph, rules }) {
      const { rt, registry } = await plugins();
      return rt.runRules(graph, rules, registry);
    },
    async renderReport({ name, resultJson }) {
      const { registry } = await plugins();
      return String(await registry.reporter(name)(JSON.parse(resultJson)));
    },
    loadJsConfig,
  };
}
