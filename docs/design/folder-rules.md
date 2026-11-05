# Folder rules

Status: in progress. This document is the design for the rules that check the shape of the folder tree. It also
covers two changes that the work needs: new rule names and a new form of the rule entry.

The text uses the terms of [website/STYLE.md](../../website/STYLE.md): a *module* is a source file in the graph, a
*dependency* is an import, a *violation* is one finding of a rule, and a *level* is `off`, `warn` or `error`.

## Problem

oxcruise checks dependencies (`forbid`, `layers`, `internal-modules`, `no-circular`) and some facts about modules
(unused exports and modules). It does not check the shape of the tree. Teams want rules such as:

- every page has a `codeowners.ts`
- a module can contain `hooks/` and `components/`, but not `utils/`
- every component has a test and a story
- a feature exposes only its `index.ts`

Today teams write these checks as Jest or Vitest specs, with `ls-lint` or `eslint-plugin-project-structure`, or as
scripts. oxcruise already walks the tree and compiles `{name}` captures in globs, so it can run these checks in the
same pass.

## Goals and non-goals

Goals:

- Check the file tree (required files, allowed children, name case, companion files).
- Make the visibility of a folder explicit (`public` and `private`).
- Detect dependency cycles between folders.
- Keep the speed: a run that does not use these rules does not walk or build anything more.

Non-goals (possible follow-ups):

- export names by place (`exports`)
- ownership (`CODEOWNERS`)
- directives (`"use client"`) and `server-only`
- changes to `forbid.from`

## Prior art

| Tool | What it shows |
|---|---|
| ArchUnit (Java), Konsist (Kotlin) | Architecture rules written as unit tests, in code. |
| ArchUnitTS, ts-arch | The same idea for TypeScript, inside Jest or Vitest. |
| import-linter (Python) | Contracts in a config file: layers, independence, forbidden dependencies. |
| Packwerk (Ruby) | Package boundaries with public folders. |
| Sheriff | Tags on folders, and rules between tags. |
| Steiger, Feature-Sliced Design | A fixed folder architecture and a linter for it. |
| eslint-plugin-boundaries | Element types by glob, and allowed dependencies between them. |
| eslint-plugin-project-structure | Folder structure and file names as a config. |
| folderslint, ls-lint | Name and structure rules for the tree, without dependencies. |
| Go `internal/` | A folder name that makes a package private to its parent. |
| Bazel `visibility` | An explicit list of who can see a target. |

The common result: tools for dependencies and tools for the tree are separate. oxcruise has the graph and the tree
in one pass, so one tool can do both.

## Rule names

Principles:

- A rule that finds a kind of problem is named after what it reports, as a plural noun phrase
  (`circular-dependencies`, `unused-exports`).
- A rule that checks the architecture that the config describes is named after that architecture, as a noun
  (`layers`, `visibility`, `folder-structure`).
- No `no-` prefix, and no abbreviations (`deps` becomes `dependencies`, `prod` becomes `production`).
- Option names keep their names. The option `prod` of `dev-dependencies-in-production` stays `prod`, because
  `production` is already a boolean option in the `unused-*` rules, and one name must not have two shapes.
- A rule about `package.json` declarations says `packages`, so that `unused-dependencies` cannot be read as "unused
  imports".

| Current | New | Reports |
|---|---|---|
| `no-circular` | `circular-dependencies` | import cycles (and, with `folders`, cycles between folders) |
| `no-orphans` | `orphan-modules` | modules with no dependents and no dependencies |
| `no-deprecated` | `deprecated-dependencies` | dependencies on deprecated built-ins and packages |
| `no-undeclared-deps` | `undeclared-packages` | imported packages that no `package.json` declares |
| `no-unresolvable` | `unresolved-dependencies` | dependencies that do not resolve |
| `no-duplicate-deps` | `duplicate-packages` | packages declared in more than one `package.json` section |
| `no-optional-deps` | `optional-dependencies` | dependencies on `optionalDependencies` |
| `no-peer-deps` | `peer-dependencies` | dependencies on `peerDependencies` |
| `no-test-imports` | `dependencies-on-tests` | dependencies of non-test modules on test files and test folders |
| `no-dev-deps-in-prod` | `dev-dependencies-in-production` | dependencies of production code on a dev-only package |
| `no-cross-package-relative` | `cross-package-relative-dependencies` | relative dependencies into another workspace package |
| `no-unused-exports` | `unused-exports` | unused exports and members |
| `no-unused-deps` | `unused-packages` | declared packages that nothing imports |
| `no-unused-files` | `unused-modules` | modules that no entry reaches |
| `internal-modules` | `visibility` | dependencies on modules that a folder does not expose |
| `layers`, `folder-structure` | unchanged | |

To confirm in review: `dependencies-on-tests`, `cross-package-relative-dependencies` and `unused-modules` (Knip says
"files", but the glossary does not allow "file" for a module).

Old names stay known: a config that uses one exits with code 2 and the message "renamed to `<new name>`".

## Consistency review

| Codebase fact | Consequence |
|---|---|
| The glossary has *module*, *entry*, *dependency*, *rule*, *violation* and *level*. | Write "file" for non-module files and "folder" (as `Relation::SameFolder` does). Do not call the public modules of a folder `entries`. |
| `PathPatterns`: `{name}` captures one segment, a leading `!` negates, and a list of only negations means "everything except". Negated globs can use captures. | One option, `allow`, covers allow-lists and deny-lists (`allow: "!utils"`). There is no `deny`. "Siblings are independent" is the `feature-isolation` recipe of `forbid`. |
| `{ident}` is a capture and `{a,b}` is alternation. The `regex` crate has no backreferences. | "The file name is the folder name" is `path: "src/components/{c}"` with `require: "{c}.tsx"`. |
| Each rule has one concern and one set of hooks and requirements. | Tree checks, dependency checks and cycle checks are separate rules. Only `folder-structure` asks for the file tree. |
| `internal-modules` uses an implicit `marker` folder. | It becomes `visibility` with an explicit `folders` list. No new rule, no implicit default. |
| `layers` without `layers` does nothing. | `visibility` without `folders` does nothing. It is not an error. |
| `no-circular` owns cycles and their report format. | Cycles between folders are the option `folders` of `circular-dependencies`. |
| Option names are camelCase, and `deny_unknown_fields` is on. | Reuse `allow` and `message`. New names: `folders`, `path`, `require`, `case`, `public`, `private`. Each has one meaning in every rule. |
| Globs are `Paths` (one string or a list). | Every glob option takes `Paths`. |
| One concept has one name and one shape. | `folders` is always a list of objects with a `path`. |
| A `forbid` rule has `message`. | Every `folders` entry that reports its own violation takes `message`. A folder cycle comes from several entries, so entries of `circular-dependencies` have no `message`. |
| The level of a rule is a `Level`, but `forbid`, the core type, the JSON result, the visualizer and the plugin API say `severity`. | One name: `level`. The core `Severity` becomes `ViolationLevel`. The JSON shape changes, so `RESULT_VERSION` goes to 3. |
| Enum values in the config are kebab-case. | `case` takes `kebab-case`, `camel-case`, `pascal-case` or `snake-case`. |
| JSON `kind: "file"` covers a violation about a path that is not a module. | A folder violation uses it too. The shape stays. |
| The rule name is the kebab-case struct name. | A rename is a struct rename. The old names are in one table. |
| New rules start in `nursery`. | `folder-structure` is `nursery`. |

## Why several rules

One rule for everything would tie a tree check, a dependency check and a symbol check to one level, one override and
one ignore name. Separate rules can be switched off one by one:
`{ "files": "src/pages/legacy/**", "rules": { "folder-structure": "off" } }` does not touch `visibility`.

The rules share one entry shape, `folders: [{ path, … }]`, so that a user learns it once:

- `folder-structure`: `require`, `allow`, `case`, `message`. It checks only the file tree.
- `visibility`: `public`, `private`, `message`. It checks dependencies.
- `circular-dependencies`: `path` only. It checks cycles between folders.

## The rule entry

The ESLint and Oxlint form `["error", { …options }]` is replaced by a flat object:
`{ "level": "error", …options }`. The string shorthand `"error"` stays. The options sit next to the level, and each
rule's schema types them, so `defineConfig` can complete them.

- An entry is a level string, or an object with an optional `level` and the options of the rule.
- In `rules`, an object without `level` means `error`: to list options enables the rule, as for `forbid`.
- In `overrides[].rules`, an object without `level` keeps the level that the rule has for those modules and changes
  only the options.
- `level` is reserved. No built-in options struct has a `level` field. The plugin host rejects a `meta.schema` that
  declares `level`, and plugins get `ctx.options` without it.
- The array form is a config error (exit code 2). The message shows the same entry in the object form. The project
  is not published, so there is no period of deprecation.

## Target config

```jsonc
"rules": {
  "orphan-modules": "warn",
  "folder-structure": { "level": "error", "folders": [
    // shape: required files, and the only direct children permitted
    { "path": "src/pages/{page}", "require": ["codeowners.ts", "index.{ts,tsx}"] },
    { "path": "src/modules/{m}", "allow": "!{utils,helpers}", "message": "Put shared helpers in src/lib." },
    // naming: the case of the names of direct children
    { "path": "src/components", "case": "pascal-case" },
    { "path": "src/components/{c}", "require": "{c}.tsx" },
    // companion files: a file match; the options apply to the folder of the file
    { "path": "src/components/**/{c}.tsx", "require": ["{c}.test.tsx", "{c}.stories.tsx"] }
  ]},
  "visibility": { "level": "error", "folders": [
    { "path": "src/features/{f}", "public": "index.ts",
      "message": "Import a feature through its index.ts." },
    { "path": "**", "private": "internal" },
    { "path": "packages/{p}", "public": ["src/index.ts", "src/testing/**"], "private": "src/testing/fixtures" }
  ]},
  "circular-dependencies": { "level": "error", "includeTypeOnly": true, "folders": [{ "path": "src/features/{f}" }] }
},
"forbid": [
  { "name": "feature-isolation", "level": "error",
    "from": "src/features/{feature}/**", "to": ["src/features/**", "!src/features/{feature}/**"] }
]
```

### Semantics of `folders`

- `path` matches a folder. In `folder-structure` it can also match a file, and then the folder is the folder of the
  file.
- `allow` matches the names of the direct children (files and folders). Deeper levels get their own entries.
- `require` globs are relative to the folder and can contain `/`. `{a,b}` means "one of".
- `case` applies to the name before the first `.` (`Button.test.tsx` gives `Button`).
- `message` replaces the generic message of the violations of the entry, as in `forbid`.
- `visibility`:
  - `public` and `private` globs are relative to the matched folder.
  - A module inside the matched folder can use any module in it.
  - For a module outside, a module inside is visible when it matches `public` (or `public` is not set) and does not
    match `private`. `private` wins.
  - An entry with neither key is a config error.
  - `path: "**"` matches every folder. The rule checks only the folders that contain `to` and not `from`.
- `circular-dependencies`: the `path` of each entry groups the modules below a matched folder. A cycle between two or
  more matched folders is a violation.
- When several entries match one folder, all of them apply.
- Option errors are collected and reported together (exit code 2).

## Ignoring violations

No rule gets an `ignore` option. The existing mechanisms apply:

1. Narrow `path` for permanent exceptions: `"path": ["src/pages/{page}", "!src/pages/legacy"]`.
2. `overrides` to switch off a rule, or to change its options, for an area. Violations about a path are scoped by
   their own path, not by the module that anchors them.
3. Inline comments, only where a module exists:
   - `visibility`: `// oxcruise-ignore-next-line visibility -- reason` above the dependency.
   - `circular-dependencies` with `folders`: the same comment on one dependency of the cycle.
   - `folder-structure` `allow` and `case`, when the child is a module: `// oxcruise-ignore-file folder-structure`.

A missing file (`require`), a folder that is not allowed and a file that is not a module have no place for a comment.
Only 1 and 2 apply. This is on purpose: an exception to the structure is a decision about a folder, so it belongs in
the config file.

## Depcruise migration

`oxcruise --migrate` today drops `required` rules and folder-scope rules. After this work:

- `required` rules whose condition is "a module has a file next to it" can become `require` of `folder-structure`.
  The converter does not do this yet: it keeps the warning, and the warning names the new rule.
- Folder-scope `forbidden` rules (`scope: "folder"`) with cycles can become `circular-dependencies` with `folders`.
  This also stays a warning for now.
- A dependency-cruiser `allowed` list of folder relations is the job of `layers` and `forbid`.

## Worked examples

### Pages that need `codeowners.ts`

```jsonc
"folder-structure": { "folders": [
  { "path": "src/pages/{page}", "require": ["codeowners.ts", "index.{ts,tsx}"] }
]}
```

### Modules without `utils/`

```jsonc
"folder-structure": { "folders": [
  { "path": "src/modules/{m}", "allow": "!{utils,helpers}", "message": "Put shared helpers in src/lib." }
]}
```

### Components with tests and stories

```jsonc
"folder-structure": { "folders": [
  { "path": "src/components", "case": "pascal-case" },
  { "path": "src/components/{c}", "require": "{c}.tsx" },
  { "path": "src/components/**/{c}.tsx", "require": ["{c}.test.tsx", "{c}.stories.tsx"] }
]}
```

### A Feature-Sliced Design setup

```jsonc
"rules": {
  "layers": { "layers": ["src/app", "src/pages", "src/features", "src/entities", "src/shared"] },
  "visibility": { "folders": [{ "path": "src/{layer}/{slice}", "public": "index.ts" }] },
  "circular-dependencies": { "folders": [{ "path": "src/features/{f}" }] }
},
"forbid": [
  { "name": "feature-isolation", "from": "src/features/{f}/**", "to": ["src/features/**", "!src/features/{f}/**"] }
]
```

### Monorepo packages that need `package.json` and `README.md`

```jsonc
"folder-structure": { "folders": [
  { "path": "packages/{p}", "require": ["package.json", "README.md"] }
]}
```

## Phases

Each phase is one change and ends green (`just ready`).

0. This document.
1. Rename every rule.
2. Object rule entries, and one name `level` for `level` and `severity`.
3. The file tree and `folder-structure`.
4. `visibility` with `folders`, and `circular-dependencies` with `folders`.

## Architecture specs in Vitest

The programmatic API (`features/programmatic-api.mdx`) returns the result of a run. A team that wants the checks
inside its test suite can call it and assert that `summary.error` is 0. The folder rules need no new API for this.

## Open questions

- Names: `dependencies-on-tests`, `cross-package-relative-dependencies` and `unused-modules`.
- Whether `folder-structure` should get an `ignore` option if the config-only way is too heavy for large repositories.
- Whether `case` should also accept a list of cases.
- Whether `visibility` should check `export` from (re-exports of) a private module.
