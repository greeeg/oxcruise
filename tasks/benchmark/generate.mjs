// Deterministic synthetic repositories for benchmarking.
//
//   node generate.mjs <dir> <modules> [--ts]
//
// Modules are spread over folders; each imports ~6 others (mostly lower-numbered, so the graph is
// nearly a DAG like real code), a few import type-only, a few lazily, and a handful form cycles. `.js` ESM is readable by
// dependency-cruiser without extra parsers; `--ts` writes TypeScript for oxcruise only.

import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";

const [dir, countArg, ...flags] = process.argv.slice(2);
if (!dir || !countArg) {
  console.error("usage: node generate.mjs <dir> <modules> [--ts]");
  process.exit(2);
}
const count = Number(countArg);
const ext = flags.includes("--ts") ? "ts" : "js";

// mulberry32: small, fast, seedable
let seed = 0x9e3779b9;
const rand = () => {
  seed |= 0;
  seed = (seed + 0x6d2b79f5) | 0;
  let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
  t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
  return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
};

const perFolder = 25;
const folders = Math.ceil(count / perFolder);
const path = (i) =>
  `pkg${Math.floor(i / (perFolder * 20))}/feature${Math.floor(i / perFolder) % 20}/m${i}.${ext}`;

rmSync(dir, { recursive: true, force: true });
mkdirSync(dir, { recursive: true });
writeFileSync(
  join(dir, "package.json"),
  JSON.stringify({ name: "synthetic", private: true, dependencies: { lodash: "4" } }),
);

for (let i = 0; i < count; i++) {
  const lines = [];
  const imports = 4 + Math.floor(rand() * 5);
  const seen = new Set();
  for (let k = 0; k < imports; k++) {
    // Like real code: imports mostly point "down" to lower-numbered modules (80% nearby, 20% anywhere
    // below), and about 1 in 200 points back "up", which closes a cycle.
    const down = rand() < 0.8 ? i - 1 - Math.floor(rand() * 40) : Math.floor(rand() * i);
    const up = i + 1 + Math.floor(rand() * 40);
    const target = Math.max(0, Math.min(count - 1, rand() < 0.005 ? up : down));
    if (target === i || seen.has(target)) continue;
    seen.add(target);
    let rel = relative(dirname(path(i)), path(target)).replace(/\.ts$/, "");
    if (!rel.startsWith(".")) rel = `./${rel}`;
    const r = rand();
    if (r < 0.1 && ext === "ts") lines.push(`import type { v${target} } from "${rel}";`);
    else if (r < 0.15) lines.push(`const lazy${target} = () => import("${rel}");`);
    else lines.push(`import { v${target} } from "${rel}";`);
  }
  if (i % 7 === 0) lines.push('import fs from "node:fs";');
  if (i % 11 === 0) lines.push('import _ from "lodash";');
  lines.push(`export const v${i} = ${i};`);
  const file = join(dir, path(i));
  mkdirSync(dirname(file), { recursive: true });
  writeFileSync(file, lines.join("\n") + "\n");
}
console.log(`${count} modules in ${folders} folders -> ${dir}`);

function relative(from, to) {
  const a = from.split("/");
  const b = to.split("/");
  let i = 0;
  while (i < a.length && i < b.length - 1 && a[i] === b[i]) i++;
  return [...a.slice(i).map(() => ".."), ...b.slice(i)].join("/");
}
