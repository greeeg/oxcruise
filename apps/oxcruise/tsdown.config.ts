import { copyFileSync, mkdirSync } from "node:fs";
import { defineConfig } from "tsdown";

const out = "../../npm/oxcruise/dist";

export default defineConfig({
  entry: { cli: "src-js/cli.ts", index: "src-js/index.ts", plugin: "src-js/plugin.ts" },
  format: "esm",
  platform: "node",
  target: "node22",
  outDir: out,
  clean: true,
  dts: true,
  // The generated loader picks the right native binary at runtime; it is copied, not bundled.
  deps: { neverBundle: [/bindings\.js$/] },
  fixedExtension: false,
  hooks: {
    "build:done": () => {
      mkdirSync(out, { recursive: true });
      copyFileSync("src-js/bindings.js", `${out}/bindings.js`);
      copyFileSync("src-js/bindings.d.ts", `${out}/bindings.d.ts`);
    },
  },
});
