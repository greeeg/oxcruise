import { copyFileSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";

const here = dirname(fileURLToPath(import.meta.url));

/**
 * The build writes fixed names into the Rust crate: `app.js` (a classic script, because a module
 * script does not load from `file://`), `app.css`, and the hand-written `index.html`.
 * `--mode dev` writes into `dev-site/` instead, to look at with a `data.js` of your own.
 */
export default defineConfig(({ mode }) => {
  const out =
    mode === "dev"
      ? resolve(here, "dev-site")
      : resolve(here, "../../crates/oxcruise_visualize/assets");
  return {
    build: {
      outDir: out,
      emptyOutDir: false,
      lib: {
        entry: resolve(here, "src/main.ts"),
        formats: ["iife" as const],
        name: "OxcruiseViewer",
        fileName: () => "app.js",
        cssFileName: "app",
      },
      minify: mode !== "dev",
      sourcemap: false,
      target: "es2022",
    },
    plugins: [
      {
        name: "copy-index-html",
        closeBundle() {
          mkdirSync(out, { recursive: true });
          copyFileSync(resolve(here, "index.html"), resolve(out, "index.html"));
        },
      },
    ],
  };
});
