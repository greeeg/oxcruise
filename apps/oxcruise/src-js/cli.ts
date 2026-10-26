import { run } from "./bindings.js";
import { createHost } from "./host.ts";

const host = createHost();
process.exitCode = await run(
  process.argv.slice(2),
  (request) => host.loadPlugin(request),
  (request) => host.runRules(request),
  (request) => host.renderReport(request),
  (path) => host.loadJsConfig(path),
);
