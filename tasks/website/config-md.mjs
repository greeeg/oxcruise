// Writes the config reference of the website from the JSON schema of the config.
// Usage: node tasks/website/config-md.mjs website/src
// Reads npm/oxcruise/configuration_schema.json. Output: <out>/pages/reference/config.mdx
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { GENERATED, cell, escapeMdx, write } from "./lib.mjs";

const out = process.argv[2];
if (!out) throw new Error("usage: config-md.mjs <website/src>");
const root = join(dirname(fileURLToPath(import.meta.url)), "../..");
const schema = JSON.parse(
  readFileSync(join(root, "npm/oxcruise/configuration_schema.json"), "utf8"),
);
const defs = schema.$defs;
const code = (s) => `\`${s}\``;
const slug = (name) => name.toLowerCase();
const refName = (s) => s.$ref.replace("#/$defs/", "");

// `X` or a list of `X`: schemars names the generic `OneOrMany`, `OneOrMany2`, ...
function oneOrMany(def) {
  const [a, b] = def?.anyOf ?? [];
  if (a && b?.type === "array" && JSON.stringify(b.items) === JSON.stringify(a)) return a;
  return null;
}

const isNull = (s) => s.type === "null";
const withoutNull = (variants) => variants.filter((s) => !isNull(s));

// The type of a schema, as inline Markdown.
function typeOf(s) {
  if (s.$ref) {
    const name = refName(s);
    const inner = oneOrMany(defs[name]);
    if (inner) return `${typeOf(inner)} or a list`;
    return `[${code(name)}](#${slug(name)})`;
  }
  if (s.anyOf) return withoutNull(s.anyOf).map(typeOf).join(" or ");
  if (s.oneOf) return s.oneOf.map(typeOf).join(" or ");
  if (s.const !== undefined) return code(JSON.stringify(s.const));
  if (s.enum) return s.enum.map((v) => code(JSON.stringify(v))).join(" or ");
  if (Array.isArray(s.type))
    return s.type
      .filter((t) => t !== "null")
      .map(code)
      .join(" or ");
  if (s.type === "array") return s.items === true ? code("array") : `list of ${typeOf(s.items)}`;
  if (s.type === "object" && s.additionalProperties)
    return `object of ${typeOf(s.additionalProperties)}, by name`;
  return code(s.type ?? "any");
}

const describe = (s) => escapeMdx((s.description ?? "").replace(/\s*\n\s*/g, " "));
// A default of `[]` or `{}` is the empty value, not worth a line.
const defaultOf = (s) => {
  const d = s.default;
  if (d === undefined || d === null) return "";
  if (Array.isArray(d) && d.length === 0) return "";
  if (typeof d === "object" && (s.$ref || Object.keys(d).length === 0)) return "";
  return code(JSON.stringify(d));
};

// Resolve a property that is only a $ref, to give it the description of the referenced type.
const withRef = (s) => (s.$ref ? { ...defs[refName(s)], ...s } : s);

function propertyTable(def) {
  const required = new Set(def.required ?? []);
  const rows = Object.entries(def.properties).map(([key, prop]) => {
    const text = describe(withRef(prop));
    return `| ${code(key)}${required.has(key) ? " (required)" : ""} | ${cell(typeOf(prop))} | ${cell(defaultOf(prop))} | ${cell(text)} |`;
  });
  return `| Key | Type | Default | Description |\n|---|---|---|---|\n${rows.join("\n")}\n`;
}

function valueTable(variants) {
  const flat = variants.flatMap((v) =>
    v.enum
      ? v.enum.map((value) => ({ value, description: v.description }))
      : [{ value: v.const, description: v.description }],
  );
  const rows = flat.map(
    (v) => `| ${code(JSON.stringify(v.value))} | ${cell(escapeMdx(v.description ?? ""))} |`,
  );
  return `| Value | Description |\n|---|---|\n${rows.join("\n")}\n`;
}

function definition(name, def) {
  let text = `### ${name}\n\n`;
  if (def.description) text += `${describe(def)}\n\n`;
  if (def.properties) text += propertyTable(def);
  else if (def.oneOf) text += valueTable(def.oneOf);
  else if (def.enum) text += `Allowed values: ${typeOf(def)}.\n`;
  else if (def.anyOf) {
    text += "One of:\n\n";
    for (const v of def.anyOf) {
      const note = v.description ? `: ${describe(v)}` : "";
      text += `* ${typeOf(v)}${note}\n`;
    }
  }
  return `${text}\n`;
}

let page = `---
title: "Configuration reference"
description: "All keys of the oxcruise config file."
---

${GENERATED("config-docs", "npm/oxcruise/configuration_schema.json")}
# Configuration reference

${describe(schema)}
Oxcruise reads \`oxcruise.config.ts\` or \`.oxcruiserc.json\`. See [Configuration](/guide/configuration) for a guide.
The config rejects keys that this page does not list.
A key of type \`string\` or a list takes globs. A leading \`!\` negates a glob: \`["src/**", "!src/**/*.test.ts"]\`.

`;
for (const [key, prop] of Object.entries(schema.properties)) {
  const full = withRef(prop);
  page += `## ${key}\n\nType: ${typeOf(prop)}.`;
  const d = defaultOf(prop);
  if (d) page += ` Default: ${d}.`;
  page += `\n\n${describe(full)}\n\n`;
}

const shown = Object.entries(defs).filter(([, def]) => !oneOrMany(def));
page += "## Types\n\nThe keys above refer to these types.\n\n";
for (const [name, def] of shown) page += definition(name, def);
write(join(out, "pages/reference/config.mdx"), page);
