// A heuristic check of the website text against website/STYLE.md (Simplified Technical English).
// Usage: node tasks/website/ste-check.mjs [files...]   (default: website/src/pages/**/*.mdx)
// Ignored: frontmatter, code blocks, tables, JSX lines and comments. Inline code counts as one word.
// Errors (the exit code is 1): too many words in a sentence, too many sentences in a paragraph,
// banned words, passive voice. Warnings: "should", phrasal verbs, abbreviations.
import { readFileSync, readdirSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "../..");
const MAX_INSTRUCTION = 25;
const MAX_DESCRIPTION = 30;
const MAX_SENTENCES = 6;

const BANNED = [
  "simply",
  "just",
  "easily",
  "utilize",
  "utilise",
  "in order to",
  "leverage",
  "very",
];
const PHRASAL = [
  "set up",
  "sets up",
  "carry over",
  "carries over",
  "turn on",
  "turns on",
  "turn off",
  "turns off",
  "look up",
  "find out",
  "point out",
  "pick up",
  "fill in",
  "start up",
  "figure out",
  "opt in",
  "opt out",
  "wrap up",
  "break down",
  "come up",
  "work out",
  "go through",
  "check out",
];
const IMPERATIVE = new Set(
  `add allow apply ask avoid build call change check choose click close commit configure connect copy
  create define delete disable do enable ensure enter extend find fix follow generate get give go
  import include install keep leave let list load look make mark match merge move name note open
  pass pick place point print put read refer remove rename replace report restart return run save
  see select send set show split start stop store switch take test try turn type update use view
  wait write`.split(/\s+/),
);
// Words in "-ed" that are not a passive participle.
const NOT_PARTICIPLE = new Set([
  "need",
  "feed",
  "speed",
  "seed",
  "embed",
  "bed",
  "red",
  "shed",
  "hundred",
  "indeed",
  "proceed",
  "exceed",
  "succeed",
]);
// Participles that are common adjectives of the product vocabulary. The checker reports them
// as passive only when no such adjective is listed here.
const ADJECTIVE = new Set(["type-only"]);

function walk(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) =>
    e.isDirectory() ? walk(join(dir, e.name)) : e.name.endsWith(".mdx") ? [join(dir, e.name)] : [],
  );
}

// Split a file into text blocks: [{ line, text, list }]. Skips what the checker ignores.
function blocks(source) {
  const lines = source.split("\n");
  const out = [];
  let i = 0;
  if (lines[0] === "---") {
    i = lines.indexOf("---", 1) + 1;
  }
  let current = null;
  let fenced = false;
  const flush = () => {
    if (current) out.push(current);
    current = null;
  };
  for (; i < lines.length; i++) {
    const line = lines[i];
    if (/^\s*(```|~~~)/.test(line)) {
      fenced = !fenced;
      flush();
      continue;
    }
    if (fenced) continue;
    if (
      !line.trim() ||
      /^\s*\|/.test(line) ||
      /^\s*</.test(line) ||
      /^\s*\{\/\*/.test(line) ||
      /^:::/.test(line) ||
      /^\s*---+\s*$/.test(line)
    ) {
      flush();
      continue;
    }
    if (/^#{1,6}\s/.test(line)) {
      flush();
      continue;
    }
    const item = line.match(/^\s*(?:[*+-]|\d+\.)\s+(.*)$/);
    if (item) {
      flush();
      current = { line: i + 1, text: item[1], list: true, numbered: /^\s*\d+\./.test(line) };
    } else if (current) {
      current.text += ` ${line.trim()}`;
    } else {
      current = { line: i + 1, text: line.trim(), list: false, numbered: false };
    }
  }
  flush();
  return out;
}

// Plain words: inline code is one word, links keep their text, emphasis marks go.
function plain(text) {
  return text
    .replace(/`[^`]*`/g, "CODE")
    .replace(/!?\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/<[^>]+>/g, " ")
    .replace(/[*_]{1,3}([^*_]+)[*_]{1,3}/g, "$1")
    .replace(/\be\.g\./gi, "for example")
    .replace(/\bi\.e\./gi, "that is")
    .replace(/\betc\./gi, "etc")
    .replace(/\\([{}<])/g, "$1");
}

function sentences(text) {
  return text
    .split(/(?<=[.!?])\s+(?=[A-Z"(])|(?<=:)\s*$/)
    .map((s) => s.trim())
    .filter(Boolean);
}

const words = (s) => s.split(/\s+/).filter(Boolean);

function check(file) {
  const findings = [];
  const add = (level, line, message) => findings.push({ level, line, message });
  for (const block of blocks(readFileSync(file, "utf8"))) {
    const raw = block.text;
    const text = plain(raw);
    const list = sentences(text);
    if (!block.list && list.length > MAX_SENTENCES)
      add("error", block.line, `paragraph has ${list.length} sentences (max ${MAX_SENTENCES})`);
    for (const sentence of list) {
      const w = words(sentence);
      const first = w[0]?.toLowerCase().replace(/[^a-z]/g, "");
      const procedural = IMPERATIVE.has(first);
      const max = procedural ? MAX_INSTRUCTION : MAX_DESCRIPTION;
      if (w.length > max)
        add(
          "error",
          block.line,
          `${procedural ? "instruction" : "sentence"} has ${w.length} words (max ${max}): "${w.slice(0, 8).join(" ")} …"`,
        );
    }
    const lower = ` ${text.toLowerCase()} `;
    for (const word of BANNED)
      if (new RegExp(`\\b${word}\\b`).test(lower))
        add("error", block.line, `banned word "${word}"`);
    for (const phrase of PHRASAL)
      if (new RegExp(`\\b${phrase}\\b`).test(lower))
        add("warning", block.line, `phrasal verb "${phrase}"`);
    if (/\bshould\b/.test(lower)) add("warning", block.line, 'use "must" for a mandatory thing');
    if (/\b(etc|e\.g|i\.e)\b/.test(raw)) add("warning", block.line, "avoid an abbreviation");
    for (const m of text.matchAll(
      /\b(is|are|was|were|been|be)\s+(?:not\s+|also\s+|then\s+|only\s+)?(\w+ed|written|given|shown|made|done|built|found|seen|taken|kept|known|chosen|drawn|sent|held|broken)\b/gi,
    )) {
      const participle = m[2].toLowerCase();
      if (NOT_PARTICIPLE.has(participle) || ADJECTIVE.has(participle)) continue;
      add("error", block.line, `passive voice: "${m[0]}"`);
    }
  }
  return findings;
}

const files =
  process.argv.length > 2 ? process.argv.slice(2) : walk(join(root, "website/src/pages"));
let errors = 0;
let warnings = 0;
for (const file of files.sort()) {
  for (const f of check(file)) {
    if (f.level === "error") errors++;
    else warnings++;
    console.log(`${relative(process.cwd(), file)}:${f.line}: ${f.level}: ${f.message}`);
  }
}
console.log(`\n${errors} errors, ${warnings} warnings in ${files.length} files`);
process.exit(errors ? 1 : 0);
