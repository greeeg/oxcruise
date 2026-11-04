import { defineConfig } from "vocs/config";
import { rulesSidebar } from "./src/rules-sidebar.generated.ts";

const repo = "https://github.com/greeeg/oxcruise";

export default defineConfig({
  title: "Oxcruise",
  description:
    "Validate and visualise your JS/TS module graph. Fast, with a small config and plugins in TypeScript.",
  logoUrl: { light: "/logo-light.svg", dark: "/logo-dark.svg" },
  iconUrl: "/icon.svg",
  accentColor: "light-dark(#0f766e, #2dd4bf)",
  baseUrl: "https://oxcruise.dev",
  renderStrategy: "full-static",
  checkDeadlinks: true,
  socials: [{ icon: "github", link: repo }],
  editLink: {
    link: `${repo}/edit/master/website/src/pages/:path`,
    text: "Edit this page on GitHub",
  },
  topNav: [
    { text: "Guide", link: "/guide/quickstart" },
    { text: "Reference", link: "/reference/rules" },
    { text: "GitHub", link: repo, external: true },
  ],
  sidebar: [
    {
      text: "Overview",
      link: "/",
    },
    {
      text: "Configure and integrate",
      items: [
        { text: "Quickstart", link: "/guide/quickstart" },
        { text: "Configuration", link: "/guide/configuration" },
        { text: "Monorepos and nested configs", link: "/guide/monorepos" },
        { text: "CI and other setup", link: "/guide/ci" },
        { text: "Coding agents", link: "/guide/coding-agents" },
      ],
    },
    {
      text: "Features",
      items: [
        { text: "Rules and categories", link: "/features/rules-and-categories" },
        { text: "Forbid rules", link: "/features/forbid-rules" },
        { text: "Layers and internal modules", link: "/features/layers-and-internal-modules" },
        { text: "Unused code", link: "/features/unused-code" },
        { text: "Automatic fixes", link: "/features/automatic-fixes" },
        { text: "Ignore files", link: "/features/ignore-files" },
        { text: "Inline ignore comments", link: "/features/inline-ignore-comments" },
        { text: "Output formats", link: "/features/output-formats" },
        { text: "Visualize the graph", link: "/features/visualize" },
        { text: "Languages", link: "/features/languages" },
        { text: "JS plugins", link: "/features/js-plugins" },
        { text: "Write a JS plugin", link: "/features/writing-js-plugins" },
        { text: "Programmatic API", link: "/features/programmatic-api" },
      ],
    },
    {
      text: "Migration",
      items: [
        { text: "From dependency-cruiser", link: "/migration/dependency-cruiser" },
        { text: "Differences", link: "/migration/differences" },
      ],
    },
    {
      text: "Reference",
      items: [
        { text: "CLI", link: "/reference/cli" },
        { text: "Config", link: "/reference/config" },
        {
          text: "Rules",
          link: "/reference/rules",
          collapsed: true,
          items: rulesSidebar,
        },
      ],
    },
  ],
});
