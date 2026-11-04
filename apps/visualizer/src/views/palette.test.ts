import { describe, expect, it } from "vitest";
import { matchRanges } from "./palette";

describe("matchRanges", () => {
  it("finds each word, case-insensitively", () => {
    expect(matchRanges("src/UI/Button.ts", ["ui", "button"])).toEqual([
      [4, 6],
      [7, 13],
    ]);
  });

  it("merges words that touch or overlap, and skips words that are absent", () => {
    expect(matchRanges("abcdef", ["abc", "cde"])).toEqual([[0, 5]]);
    expect(matchRanges("abcdef", ["ab", "cd"])).toEqual([[0, 4]]);
    expect(matchRanges("abc", ["x", ""])).toEqual([]);
  });
});
