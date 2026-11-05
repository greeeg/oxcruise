// Writes the rules reference of the website from `oxcruise --rules -f json` and conventions.json.
// Usage: oxcruise --rules -f json | node tasks/website/rules-md.mjs website/src
// Output: <out>/pages/reference/rules/index.mdx, one <rule>.mdx per rule, and
// <out>/rules-sidebar.generated.ts (the sidebar entries of the rule pages).
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { GENERATED, capitalize, cell, escapeMdx, write, yaml } from "./lib.mjs";

const out = process.argv[2];
if (!out) throw new Error("usage: rules-md.mjs <website/src>");
const rules = JSON.parse(readFileSync(0, "utf8"));
const byCategory = Map.groupBy(rules, (r) => r.category);
const order = ["correctness", "suspicious", "style", "restriction", "nursery"];
const categories = order.filter((c) => byCategory.has(c));
// what a category does when the config does not mention it
const defaults = { correctness: "error", suspicious: "warn" };
const level = (category) => defaults[category] ?? "off";
const source = "the declare_rule! doc comments";
const rulesDir = join(out, "pages/reference/rules");

// Demote the `###` headings of the doc comments to `##`: the page title is the `#` heading.
const body = (docs) => escapeMdx(docs.replace(/^### /gm, "## "));
// The first sentence of the "What it does" paragraph: the same text as `oxcruise --rules`.
const firstSentence = (docs) => {
  const paragraph = docs
    .split(/^### /m)
    .find((section) => section.startsWith("What it does"))
    ?.split("\n")
    .slice(1)
    .join("\n")
    .trim()
    .split(/\n\s*\n/)[0]
    .replace(/\s+/g, " ");
  if (!paragraph) throw new Error("a rule has no 'What it does' paragraph");
  const end = paragraph.indexOf(". ");
  return end === -1 ? paragraph : paragraph.slice(0, end + 1);
};

// The example of the rule page. The first `json` block of the "Example" section of the docs is the
// options object of the rule. A rule that has no such block is shown with its level only.
const optionsExample = (docs) => {
  const section = docs.split(/^### /m).find((s) => s.startsWith("Example"));
  const block = section?.match(/```json\n([\s\S]*?)```/)?.[1];
  return block ? JSON.parse(block) : undefined;
};
// A rule that is `error` by default needs another level to show a change.
const exampleLevel = (category) => (level(category) === "error" ? "warn" : "error");
const configExample = (r) => {
  const options = optionsExample(r.docs);
  const setting = options
    ? `["${exampleLevel(r.category)}", ${JSON.stringify(options, null, 2).replace(/\n/g, "\n    ")}]`
    : `"${exampleLevel(r.category)}"`;
  return `{\n  "rules": {\n    "${r.name}": ${setting}\n  }\n}`;
};
const optionsText = (docs) =>
  optionsExample(docs)
    ? "The array has the level first, then the options of the rule."
    : /^### Options$/m.test(docs)
      ? "To set options, use an array: the level first, then an object with the options."
      : "";
const isFixable = (docs) => /^### Fix$/m.test(docs);

let index = `---
title: "Rules"
description: "All built-in oxcruise rules, by category."
---

${GENERATED("rule-docs", source)}
# Rules

Oxcruise has ${rules.length} built-in rules. Each rule is in one category.
A category switches all its rules on or off together. See [Rules and categories](/features/rules-and-categories).

| Rule | Category | Default |
|---|---|---|
${rules.map((r) => `| [\`${r.name}\`](/reference/rules/${r.name}) | ${r.category} | ${level(r.category)} |`).join("\n")}

`;
for (const cat of categories) {
  index += `## ${capitalize(cat)}\n\nDefault level: \`${level(cat)}\`.\n\n`;
  for (const r of byCategory.get(cat))
    index += `* [\`${r.name}\`](/reference/rules/${r.name}): ${escapeMdx(firstSentence(r.docs))}\n`;
  index += "\n";
}

// The tool conventions behind `unused-exports`, `unused-modules` and `unused-packages`, from the
// same JSON the rules embed, so the table and the code cannot drift apart.
const conventions = JSON.parse(
  readFileSync(
    join(
      dirname(fileURLToPath(import.meta.url)),
      "../../crates/oxcruise_rules/src/conventions.json",
    ),
    "utf8",
  ),
);
const code = (list) => list.map((g) => `\`${g}\``).join("<br />") || "–";
index += `## Conventions

A convention tells the \`no-unused-*\` rules what a tool loads by itself.
A convention is active in a \`package.json\` that declares one of its packages.
When a convention lists no packages, it is always active.

* The entry points are globs relative to that \`package.json\`. They are files that the tool loads.
* The \`unused-packages\` rule counts the packages of a convention as used.
* Use the \`conventions\` option of the rules to switch conventions off or to select some of them.

| Convention | Active when it declares | Entry points | Packages used |
|---|---|---|---|
${conventions.map((c) => `| ${c.name} | ${cell(code(c.when))} | ${cell(code(c.entries))} | ${cell(code(c.uses))} |`).join("\n")}
`;
write(join(rulesDir, "index.mdx"), index);

for (const r of rules) {
  const default_ = level(r.category);
  write(
    join(rulesDir, `${r.name}.mdx`),
    `---
title: ${yaml(r.name)}
description: ${yaml(firstSentence(r.docs))}
---

${GENERATED("rule-docs", source)}
# ${r.name}

Category: [${r.category}](/reference/rules#${r.category}). Default: ${default_ === "off" ? "off" : `\`${default_}\``}. Fixable: ${isFixable(r.docs) ? "yes" : "no"}. Since: ${r.version}.

${body(r.docs)}

## Configure the rule

\`\`\`json
${configExample(r)}
\`\`\`

${optionsText(r.docs)}
`,
  );
}

const items = categories.map((cat) => ({
  text: capitalize(cat),
  collapsed: true,
  items: byCategory.get(cat).map((r) => ({ text: r.name, link: `/reference/rules/${r.name}` })),
}));
write(
  join(out, "rules-sidebar.generated.ts"),
  `// generated by \`just rule-docs\` from ${source}; do not edit
export const rulesSidebar = ${JSON.stringify(items, null, 2)};
`,
);
