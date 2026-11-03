# Style guide for the website text

The website text follows a practical subset of Simplified Technical English (ASD-STE100).
The goal is text that a reader who is not a native speaker can follow without help.
This rule set also applies to the doc comments from which the reference pages are generated:

* the `declare_rule!` doc comments in `crates/oxcruise_rules/src/rules/*.rs`
* the config doc comments in `crates/oxcruise_config/src/config.rs`
* the clap doc comments in `apps/oxcruise/src/args.rs`

Run `just website-lint` to find the problems that a program can find.
The checker (`tasks/website/ste-check.mjs`) is a heuristic.
It does not replace a manual review of each page against this guide.

## Sentences

* An instruction has at most 20 words. A description has at most 25 words.
* A paragraph has at most 6 sentences.
* A sentence has one topic. Split a sentence that has two.
* Use the imperative for instructions: "Run `npx oxcruise --init`."
* Use the active voice and the simple present tense. Write "The rule reports a cycle", not "A cycle is reported".
* Put a warning or a caution before the step that it applies to. Use a `:::warning` callout.
* Write one instruction in each sentence. Put the steps of a procedure in a numbered list.

## Words

* Use one word for one meaning, and use the same word for the same thing. See the glossary below.
* Use "must" for a mandatory action or condition. Do not use "should".
* Do not use phrasal verbs, such as "set up", "carry over" or "turn on". Use "configure", "keep" or "enable".
* Do not use "-ing" forms as nouns or adjectives when another word is possible.
* Do not use abbreviations such as "e.g." or "i.e.". Write "for example" or "that is".
* Do not use these words: *simply, just, easily, utilize, in order to, leverage, very*.
* Technical names and code identifiers are allowed: flags, keys, rule names, file names. Put them in `code`.

## Layout

* Use a list or a table when the text has three or more parallel items.
* Start each page with a short paragraph that says what the page is for.
* Use sentence case in headings.
* Give each code sample a language. Show only the lines that the reader needs.
* Show real output. Copy it from a run of the current build.

## Glossary

Use these terms, and no others, for these things.

| Term | Meaning | Do not use |
|---|---|---|
| module | A source file in the graph. | file (for a module), node |
| dependency | An import from one module to another module, package or built-in. | edge, link, import (as a noun) |
| rule | A check that reports violations. | check, validator |
| category | A group of built-in rules that has one default level. | preset, group |
| level | `off`, `warn` or `error`: what a rule does when it finds a violation. | severity (in prose), mode |
| entry | A path or glob where the graph starts. | root, start point |
| violation | One finding that a rule reports. | problem, issue, error (for a finding) |
| config file | `oxcruise.config.ts` or `.oxcruiserc.json`. | configuration (for the file), rc file |
| plugin | A JS or TS module that adds rules or reporters. | extension, add-on |
| reporter | Code that formats the result of a run. | formatter, output plugin |

The key `severity` in `forbid` is a code identifier. Write it in `code`.
