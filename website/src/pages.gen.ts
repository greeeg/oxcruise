// deno-fmt-ignore-file
// biome-ignore format: generated types do not need formatting
// prettier-ignore
import type { PathsForPages } from 'waku/router';

// prettier-ignore
type Page =
  | { path: '/features/automatic-fixes'; render: 'static' }
  | { path: '/features/forbid-rules'; render: 'static' }
  | { path: '/features/ignore-files'; render: 'static' }
  | { path: '/features/inline-ignore-comments'; render: 'static' }
  | { path: '/features/js-plugins'; render: 'static' }
  | { path: '/features/languages'; render: 'static' }
  | { path: '/features/layers-and-internal-modules'; render: 'static' }
  | { path: '/features/output-formats'; render: 'static' }
  | { path: '/features/programmatic-api'; render: 'static' }
  | { path: '/features/rules-and-categories'; render: 'static' }
  | { path: '/features/unused-code'; render: 'static' }
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
  | { path: '/reference/rules'; render: 'static' }
  | { path: '/reference/rules/internal-modules'; render: 'static' }
  | { path: '/reference/rules/layers'; render: 'static' }
  | { path: '/reference/rules/no-circular'; render: 'static' }
  | { path: '/reference/rules/no-cross-package-relative'; render: 'static' }
  | { path: '/reference/rules/no-deprecated'; render: 'static' }
  | { path: '/reference/rules/no-dev-deps-in-prod'; render: 'static' }
  | { path: '/reference/rules/no-duplicate-deps'; render: 'static' }
  | { path: '/reference/rules/no-optional-deps'; render: 'static' }
  | { path: '/reference/rules/no-orphans'; render: 'static' }
  | { path: '/reference/rules/no-peer-deps'; render: 'static' }
  | { path: '/reference/rules/no-test-imports'; render: 'static' }
  | { path: '/reference/rules/no-undeclared-deps'; render: 'static' }
  | { path: '/reference/rules/no-unresolvable'; render: 'static' }
  | { path: '/reference/rules/no-unused-deps'; render: 'static' }
  | { path: '/reference/rules/no-unused-exports'; render: 'static' }
  | { path: '/reference/rules/no-unused-files'; render: 'static' };

// prettier-ignore
declare module 'waku/router' {
  interface RouteConfig {
    paths: PathsForPages<Page>
  }
  interface CreatePagesConfig {
    pages: Page
  }
}
