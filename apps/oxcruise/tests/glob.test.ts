import { describe, expect, test } from "vitest";
import { globToRegExp, matchesGlob } from "../src-js/glob.ts";

describe("globs mean the same as in the Rust pattern module", () => {
  test.each([
    ["src/*.ts", "src/a.ts", true],
    ["src/*.ts", "src/deep/a.ts", false],
    ["lib", "lib", true],
    ["lib", "lib/deep/a.ts", true],
    ["lib", "library/a.ts", false],
    ["**/*.test.ts", "a/b/c.test.ts", true],
    ["**/*.test.ts", "c.test.ts", true],
    ["a/**/z.ts", "a/z.ts", true],
    ["a/**/z.ts", "a/b/c/z.ts", true],
    ["x/**", "x", true],
    ["x/**", "x/y/z", true],
    ["x/**", "xx/y", false],
    ["src/*.{ts,tsx}", "src/a.tsx", true],
    ["src/*.{ts,tsx}", "src/a.js", false],
    ["./src/a.ts", "src/a.ts", true],
    ["src/a?.ts", "src/ab.ts", true],
    ["src/a.b", "src/aXb", false],
  ])("%s vs %s -> %s", (glob, path, want) => {
    expect(matchesGlob(glob, path)).toBe(want);
  });

  test("regex metacharacters in a glob are literal", () => {
    expect(globToRegExp("a+b(c).ts").test("a+b(c).ts")).toBe(true);
    expect(globToRegExp("a+b(c).ts").test("aab(c).ts")).toBe(false);
  });
});
