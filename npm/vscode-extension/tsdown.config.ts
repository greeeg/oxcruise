import { defineConfig } from "tsdown";

export default defineConfig({
  entry: { extension: "src/extension.ts" },
  // VS Code loads extensions as CommonJS and provides `vscode` itself.
  format: "cjs",
  platform: "node",
  target: "node20",
  outDir: "dist",
  clean: true,
  dts: false,
  fixedExtension: true,
  deps: {
    neverBundle: ["vscode"],
    alwaysBundle: [/^vscode-languageclient/, /^vscode-jsonrpc/, /^vscode-languageserver/],
  },
});
