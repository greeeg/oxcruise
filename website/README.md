# Oxcruise website

The documentation site of Oxcruise, built with [Vocs](https://vocs.dev). The text follows the
Simplified Technical English rules in [STYLE.md](STYLE.md).

## Layout

```
vocs.config.ts            title, sidebar (the four groups: overview, guides, features, reference), top navigation
src/pages/                the pages (MDX): index, guide/, features/, reference/
src/rules-sidebar.generated.ts   the sidebar entries of the rule pages (generated)
public/                   logo and icon
```

The pages under `src/pages/reference/` (`cli.mdx`, `config.mdx`, `rules/`) and `src/rules-sidebar.generated.ts` are
**generated**. Do not edit them. Change their sources and run `just website-gen` from the repository root:

| Page                   | Source                                                                                                         | Recipe             |
| ---------------------- | -------------------------------------------------------------------------------------------------------------- | ------------------ |
| `reference/rules/*`    | the `declare_rule!` doc comments in `crates/oxcruise_rules` and `conventions.json`                             | `just rule-docs`   |
| `reference/config.mdx` | `npm/oxcruise/configuration_schema.json` (from the doc comments in `crates/oxcruise_config`; `just config-ts`) | `just config-docs` |
| `reference/cli.mdx`    | the clap doc comments in `apps/oxcruise/src/args.rs` (`oxcruise --help`)                                       | `just cli-docs`    |

## Commands

Run these from the repository root (or use `pnpm -C website run <script>`):

```sh
just website-dev     # dev server
just website-build   # static build into website/dist
just website-lint    # Simplified Technical English checker (tasks/website/ste-check.mjs)
just website-gen     # regenerate the reference pages
```

`pnpm install` once, at the repository root. `renderStrategy` is `full-static`, so the build is a static site.
