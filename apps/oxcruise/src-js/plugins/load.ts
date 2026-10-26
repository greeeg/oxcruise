// Resolving, importing and validating plugin modules.

import { createRequire } from "node:module";
import { isAbsolute, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import type { PluginDefinition } from "./types.ts";

export interface LoadedPlugin {
  /** Name its rules are registered under: the alias, else `meta.name`. */
  name: string;
  def: PluginDefinition;
  /** Absolute path (or package entry) it was loaded from. */
  source: string;
}

export class PluginRegistry {
  readonly plugins = new Map<string, LoadedPlugin>();

  /** `acme/max-fan-out` -> rule. */
  rule(fullName: string) {
    for (const p of this.plugins.values()) {
      const prefix = `${p.name}/`;
      if (fullName.startsWith(prefix)) {
        const rule = p.def.rules?.[fullName.slice(prefix.length)];
        if (rule) return { plugin: p, rule };
      }
    }
    throw new Error(`no loaded plugin provides the rule \`${fullName}\``);
  }

  reporter(fullName: string) {
    for (const p of this.plugins.values()) {
      const prefix = `${p.name}/`;
      if (fullName.startsWith(prefix)) {
        const fn = p.def.reporters?.[fullName.slice(prefix.length)];
        if (fn) return fn;
      }
    }
    throw new Error(`no loaded plugin provides the reporter \`${fullName}\``);
  }
}

/** Candidate package names for a bare specifier: `acme` -> `oxcruise-plugin-acme`, then `acme`. */
export function packageCandidates(specifier: string): string[] {
  if (specifier.startsWith("@")) {
    const [scope, name, ...rest] = specifier.split("/");
    if (name && !name.startsWith("oxcruise-plugin")) {
      return [[scope, `oxcruise-plugin-${name}`, ...rest].join("/"), specifier];
    }
    return [specifier];
  }
  return specifier.startsWith("oxcruise-plugin-") ? [specifier] : [`oxcruise-plugin-${specifier}`, specifier];
}

export function resolveSpecifier(specifier: string, cwd: string): string {
  if (specifier.startsWith(".") || isAbsolute(specifier)) return resolve(cwd, specifier);
  const require = createRequire(resolve(cwd, "noop.js"));
  const tried: string[] = [];
  for (const candidate of packageCandidates(specifier)) {
    try {
      return require.resolve(candidate);
    } catch {
      tried.push(candidate);
    }
  }
  throw new Error(`cannot find plugin \`${specifier}\` from ${cwd} (tried ${tried.join(", ")})`);
}

const NAME = /^[a-z0-9][a-z0-9._-]*$/;

/** Throws with a message naming the plugin and exactly what is wrong. */
export function validatePlugin(value: unknown, source: string): PluginDefinition {
  const where = `plugin ${source}`;
  if (typeof value !== "object" || value === null) {
    throw new Error(`${where}: the default export must be an object created with definePlugin()`);
  }
  const p = value as Record<string, unknown>;
  const meta = p.meta as Record<string, unknown> | undefined;
  if (typeof meta?.name !== "string" || !NAME.test(meta.name)) {
    throw new Error(`${where}: \`meta.name\` must be a lowercase string like "acme"`);
  }
  for (const key of Object.keys(p)) {
    if (!["meta", "rules", "reporters"].includes(key)) {
      throw new Error(`${where}: unknown property \`${key}\` (expected meta, rules, reporters)`);
    }
  }
  for (const [name, rule] of Object.entries((p.rules ?? {}) as Record<string, unknown>)) {
    if (!NAME.test(name)) throw new Error(`${where}: invalid rule name \`${name}\``);
    if (typeof (rule as { create?: unknown } | null)?.create !== "function") {
      throw new Error(`${where}: rule \`${name}\` must have a \`create(context)\` function`);
    }
  }
  for (const [name, fn] of Object.entries((p.reporters ?? {}) as Record<string, unknown>)) {
    if (!NAME.test(name)) throw new Error(`${where}: invalid reporter name \`${name}\``);
    if (typeof fn !== "function") throw new Error(`${where}: reporter \`${name}\` must be a function`);
  }
  return value as PluginDefinition;
}

export interface LoadRequest {
  specifier: string;
  alias?: string | null;
  cwd: string;
}

/** What Rust gets back: the fully qualified names it may configure. */
export interface LoadedMetadata {
  name: string;
  rules: string[];
  reporters: string[];
}

export async function loadPlugin(request: LoadRequest, registry: PluginRegistry): Promise<LoadedMetadata> {
  const source = resolveSpecifier(request.specifier, request.cwd);
  let mod: { default?: unknown };
  try {
    mod = (await import(pathToFileURL(source).href)) as { default?: unknown };
  } catch (e) {
    const hint = /\.[cm]?ts$/.test(source) ? " (TypeScript plugins need Node >= 22.18)" : "";
    throw new Error(`failed to import ${source}: ${e instanceof Error ? e.message : String(e)}${hint}`);
  }
  const def = validatePlugin(mod.default, source);
  const name = request.alias ?? def.meta.name;
  const existing = registry.plugins.get(name);
  if (existing && existing.source !== source) {
    throw new Error(`two plugins are named \`${name}\` (${existing.source} and ${source}); give one an alias in \`jsPlugins\``);
  }
  registry.plugins.set(name, { name, def, source });
  return { name, rules: Object.keys(def.rules ?? {}), reporters: Object.keys(def.reporters ?? {}) };
}
