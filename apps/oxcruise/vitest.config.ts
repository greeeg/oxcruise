import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    include: ["tests/**/*.test.ts"],
    // projects the CLI tests run on, not tests of ours
    exclude: ["tests/fixtures/**", "node_modules/**"],
    // end-to-end tests spawn the native addon and Node; give them room
    testTimeout: 60_000,
    // the native addon is loaded once per worker process
    pool: "forks",
  },
});
