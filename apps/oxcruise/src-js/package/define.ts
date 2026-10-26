import type { OxcruiseConfig } from "../config.generated.ts";
import type { PluginDefinition } from "../plugins/types.ts";

/** Type-check an `oxcruise.config.ts`. */
export function defineConfig(config: OxcruiseConfig): OxcruiseConfig {
  return config;
}

/** Type-check a plugin module; the default export of a `jsPlugins` entry. */
export function definePlugin<const P extends PluginDefinition>(plugin: P): P {
  return plugin;
}
