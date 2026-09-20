import { describe, expect, it } from "vitest";

import { composeJamoRuns, isUncomposedJamoRun } from "./hangulComposition";

describe("composeJamoRuns", () => {
  it.each([
    ["ㅇㅣㄹㅓㅎㄱㅔ", "이렇게"],
    ["ㅎㅏㄴㄱㅡㄹ", "한글"],
    ["ㅇㅏㄴㄴㅕㅇㅎㅏㅅㅔㅇㅛ", "안녕하세요"],
    ["ㄱㅏㅂㅅ", "값"],
    ["ㅇㅣㄹㄱㄷㅏ", "읽다"],
    ["ㅇㅏㄴㅈㄷㅏ", "앉다"],
    ["ㄱㅗㅐㄴㅊㅏㄴㅎㄷㅏ", "괜찮다"],
    ["ㄱㅗㅏㅇ", "광"],
    ["ㄱㅏㅅㅏ", "가사"],
    ["ㄷㅏㄹㄱㅏ", "달가"],
    ["ㅁㅗㅂㅏㅇㅣㄹ", "모바일"],
  ])("composes %s into %s", (jamo, syllable) => {
    expect(composeJamoRuns(jamo)).toBe(syllable);
  });

  it.each([
    ["ㅓㅗㅏ", "standalone vowels keep their ㅇ-free form"],
    ["ㅠㅠ", "repeated vowels stay vowels"],
    ["ㄱㄴ", "consonants that never met a vowel stay lone jamo"],
    ["ㅋㅋ", "laughter stays laughter"],
    ["ㄳ", "a compound final typed alone is not an initial"],
    ["한글 test 123 값", "precomposed syllables and ASCII are left untouched"],
    ["이렇게", "already composed text is unchanged"],
    ["", "empty text stays empty"],
  ])("leaves %s alone (%s)", (text) => {
    expect(composeJamoRuns(text)).toBe(text);
  });

  it("composes only the jamo runs of mixed text", () => {
    expect(composeJamoRuns("ls -la ㅎㅏㄴㄱㅡㄹ ls")).toBe("ls -la 한글 ls");
  });

  it("is idempotent", () => {
    const jamo = "ㅇㅏㄴㄴㅕㅇㅎㅏㅅㅔㅇㅛ ㅁㅗㅂㅏㅇㅣㄹ";
    expect(composeJamoRuns(composeJamoRuns(jamo))).toBe(composeJamoRuns(jamo));
  });

  it("composes conjoining jamo the same as compatibility jamo", () => {
    expect(composeJamoRuns("\u1106\u1161")).toBe("마");
    expect(composeJamoRuns("\u1100\u1161\u11ab")).toBe("간");
    expect(composeJamoRuns("\u1100\u1161\u11ab\u1103\u1161")).toBe("간다");
  });
});

describe("isUncomposedJamoRun", () => {
  it.each([
    ["ㅇㅣ", true],
    ["ㄱㄴㅅ", true],
    ["\u1106\u1161", true],
    ["이", false],
    ["한", false],
    ["ㅇㅣa", false],
    ["ㅇㅣ ", false],
    ["", false],
  ])("classifies %s as %s", (text, expected) => {
    expect(isUncomposedJamoRun(text)).toBe(expected);
  });
});
