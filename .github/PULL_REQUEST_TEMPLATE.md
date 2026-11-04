## What and why

<!-- What does this change, and why? Link the issue it fixes: "Fixes #123". -->

## How to try it

<!-- For a user-visible change: the command, config or code that shows it. For example `npx oxcruise src`. -->

## Checklist

- [ ] `just ready` passes
- [ ] Tests added or updated
- [ ] Docs updated if behaviour changed (`website/src`, `README.md`)
- [ ] Generated files regenerated if their sources changed (`just config-ts`, `just website-gen`, `just visualizer-build`)
- [ ] Output shape or plugin API changed? `RESULT_VERSION` or the graph buffer `VERSION` bumped

## Performance

<!-- Skip if the change is not on the hot path. Otherwise: before and after numbers from `cargo bench` or
`node tasks/benchmark/run.mjs`, and a note that the output is identical. -->
