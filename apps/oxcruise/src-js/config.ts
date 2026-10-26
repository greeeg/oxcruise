// Evaluates a JS/TS config inside this Node process and hands it to Rust as JSON.

import { pathToFileURL } from "node:url";

export async function loadJsConfig(path: string): Promise<string> {
  let mod: { default?: unknown };
  try {
    mod = (await import(pathToFileURL(path).href)) as { default?: unknown };
  } catch (e) {
    const hint = /\.[cm]?ts$/.test(path) ? " (TypeScript configs need Node >= 22.18)" : "";
    throw new Error(`cannot evaluate ${path}: ${e instanceof Error ? e.message : String(e)}${hint}`);
  }
  let config: unknown = mod.default ?? mod;
  if (typeof config === "function") config = (config as () => unknown)();
  config = await config;
  // CJS interop: `module.exports = {...}` imported as `{ default: {...} }`
  if (config && typeof config === "object" && "default" in config && Object.keys(config).length === 1) {
    config = (config as { default: unknown }).default;
  }
  return JSON.stringify(config ?? {});
}
