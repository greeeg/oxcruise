import { defineConfig } from "vocs/config";
import { rulesSidebar } from "./src/rules-sidebar.generated.ts";

const repo = "https://github.com/greeeg/oxcruise";

export default defineConfig({
  title: "Oxcruise",
  description:
    "Validate and visualise your JS/TS module graph. Fast, with a small config and plugins in TypeScript.",
  logoUrl: { light: "/logo-light.png", dark: "/logo-dark.png" },
  iconUrl: "/icon.png",
  head: { link: [{ rel: "apple-touch-icon", href: "/apple-touch-icon.png" }] },
  accentColor: "light-dark(#19478e, #5897e2)",
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
        { text: "Migrate from dependency-cruiser", link: "/guide/migrate-from-dependency-cruiser" },
      ],
    },
    {
      text: "Features",
      items: [
        { text: "Rules and categories", link: "/features/rules-and-categories" },
        { text: "Automatic fixes", link: "/features/automatic-fixes" },
        { text: "Ignore code", link: "/features/ignore" },
        { text: "Output formats", link: "/features/output-formats" },
        { text: "Visualize the graph", link: "/features/visualize" },
        { text: "Languages", link: "/features/languages" },
        { text: "JS plugins", link: "/features/js-plugins" },
        { text: "Programmatic API", link: "/features/programmatic-api" },
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
