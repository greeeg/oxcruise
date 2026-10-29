import { run } from "./bindings.js";
import { createHost } from "./host.ts";

// In `--lsp` mode stdout carries the protocol: whatever a config or plugin prints goes to stderr.
if (process.argv.includes("--lsp")) {
  console.log = console.info = console.debug = (...args) => console.error(...args);
}

const host = createHost();
process.exitCode = await run(
  process.argv.slice(2),
  (request) => host.loadPlugin(request),
  (request) => host.runRules(request),
  (request) => host.renderReport(request),
  (path) => host.loadJsConfig(path),
);
