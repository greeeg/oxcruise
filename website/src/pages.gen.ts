// deno-fmt-ignore-file
// biome-ignore format: generated types do not need formatting
// prettier-ignore
import type { PathsForPages } from 'waku/router'

// prettier-ignore
type Page =
  | { path: '/features/automatic-fixes'; render: 'static' }
  | { path: '/features/folder-structure'; render: 'static' }
  | { path: '/features/forbid-rules'; render: 'static' }
  | { path: '/features/ignore-files'; render: 'static' }
  | { path: '/features/inline-ignore-comments'; render: 'static' }
  | { path: '/features/js-plugins'; render: 'static' }
  | { path: '/features/languages'; render: 'static' }
  | { path: '/features/layers-and-visibility'; render: 'static' }
  | { path: '/features/output-formats'; render: 'static' }
  | { path: '/features/programmatic-api'; render: 'static' }
  | { path: '/features/rules-and-categories'; render: 'static' }
  | { path: '/features/unused-code'; render: 'static' }
  | { path: '/features/visualize'; render: 'static' }
  | { path: '/features/writing-js-plugins'; render: 'static' }
  | { path: '/guide/ci'; render: 'static' }
  | { path: '/guide/coding-agents'; render: 'static' }
  | { path: '/guide/configuration'; render: 'static' }
  | { path: '/guide/monorepos'; render: 'static' }
  | { path: '/guide/quickstart'; render: 'static' }
  | { path: '/'; render: 'static' }
  | { path: '/migration/dependency-cruiser'; render: 'static' }
  | { path: '/migration/differences'; render: 'static' }
  | { path: '/reference/cli'; render: 'static' }
  | { path: '/reference/config'; render: 'static' }
  | { path: '/reference/rules/circular-dependencies'; render: 'static' }
  | { path: '/reference/rules/cross-package-relative-dependencies'; render: 'static' }
  | { path: '/reference/rules/dependencies-on-tests'; render: 'static' }
  | { path: '/reference/rules/deprecated-dependencies'; render: 'static' }
  | { path: '/reference/rules/dev-dependencies-in-production'; render: 'static' }
  | { path: '/reference/rules/duplicate-packages'; render: 'static' }
  | { path: '/reference/rules/folder-structure'; render: 'static' }
  | { path: '/reference/rules'; render: 'static' }
  | { path: '/reference/rules/layers'; render: 'static' }
  | { path: '/reference/rules/optional-dependencies'; render: 'static' }
  | { path: '/reference/rules/orphan-modules'; render: 'static' }
  | { path: '/reference/rules/peer-dependencies'; render: 'static' }
  | { path: '/reference/rules/undeclared-packages'; render: 'static' }
  | { path: '/reference/rules/unresolved-dependencies'; render: 'static' }
  | { path: '/reference/rules/unused-exports'; render: 'static' }
  | { path: '/reference/rules/unused-modules'; render: 'static' }
  | { path: '/reference/rules/unused-packages'; render: 'static' }
  | { path: '/reference/rules/visibility'; render: 'static' }

// prettier-ignore
declare module 'waku/router' {
  interface RouteConfig {
    paths: PathsForPages<Page>
  }
  interface CreatePagesConfig {
    pages: Page
  }
}
