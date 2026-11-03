// Writes the CLI reference of the website from the `--help` text of oxcruise.
// Usage: oxcruise --help | node tasks/website/cli-md.mjs website/src
// Output: <out>/pages/reference/cli.mdx, one table per help heading.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { GENERATED, cell, escapeMdx, write } from "./lib.mjs";

const out = process.argv[2];
if (!out) throw new Error("usage: cli-md.mjs <website/src>");
const lines = readFileSync(0, "utf8").split("\n");

// Split the help text into blocks. A block starts at a line with no indent that ends in ":".
const blocks = [];
for (const line of lines) {
  const heading = line.match(/^(\S.*):$/);
  if (heading) blocks.push({ name: heading[1], items: [] });
  else if (blocks.length) blocks.at(-1).items.push(line);
}
const usage = lines.find((l) => l.startsWith("Usage:"));
const about = lines[0];

// An item starts with a flag or an argument, at an indent of 2 to 6 spaces. Its description
// follows after 2 or more spaces, or on the continuation lines.
function parseItems(items) {
  const parsed = [];
  for (const line of items) {
    if (!line.trim()) continue;
    const start = line.match(/^ {2,6}(-\S.*?|\[\S.*?)(?: {2,}(\S.*))?$/);
    if (start) {
      parsed.push({ flags: start[1], text: start[2] ?? "" });
    } else if (parsed.length) {
      parsed.at(-1).text += ` ${line.trim()}`;
    }
  }
  return parsed;
}

const sections = blocks
  .filter((b) => b.name !== "Usage")
  .map((b) => ({ name: b.name, items: parseItems(b.items) }))
  .filter((b) => b.items.length);

let page = `---
title: "CLI reference"
description: "All flags of the oxcruise command."
---

${GENERATED("cli-docs", "the output of `oxcruise --help`")}
# CLI reference

${escapeMdx(about)}.

\`\`\`sh
${usage.replace("Usage: ", "")}
\`\`\`

A mode flag replaces the check. Use one mode flag at a time.
The mode flags refuse the flags that only make sense for a check, such as \`--fix\`, \`--quiet\` and \`--threads\`.
The \`--rules\` flag also accepts \`-f\`.

The exit code is:

| Code | Meaning |
|---|---|
| \`0\` | The run has no errors. Warnings do not change the code. |
| \`1\` | The run has errors. The code is also \`1\` when \`--deny-warnings\` finds a warning, or when \`--max-warnings\` finds too many. |
| \`2\` | The invocation or the config is not valid. |

`;
for (const section of sections) {
  const name = section.name === "Options" ? "General" : section.name.replace(/ Options$/, "");
  page += `## ${name}\n\n| Flag | Description |\n|---|---|\n`;
  for (const item of section.items) {
    const text = item.text.replace(/\[default: (.*?)\]/, "(default: `$1`)");
    page += `| ${cell(`\`${item.flags.replace(/\s{2,}/g, " ")}\``)} | ${cell(escapeMdx(text))} |\n`;
  }
  page += "\n";
}
write(join(out, "pages/reference/cli.mdx"), page);
