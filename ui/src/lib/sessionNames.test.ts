import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { SESSION_NAME_ADJECTIVES, SESSION_NAME_NOUNS } from "./sessionNameWords";
import { baseSessionName, fnv1a, uniqueSessionName } from "./sessionNames";

const namesDir = resolve(import.meta.dirname, "../../../src-tauri/resources/names");
const wordFile = (file: string) =>
  readFileSync(resolve(namesDir, file), "utf8").split("\n").map((line) => line.trim()).filter(Boolean);

describe("session names", () => {
  it("mirrors the daemon's embedded word lists exactly", () => {
    expect(SESSION_NAME_ADJECTIVES).toEqual(wordFile("adjectives.txt"));
    expect(SESSION_NAME_NOUNS).toEqual(wordFile("nouns.txt"));
  });

  it("hashes like the daemon (FNV-1a 32-bit)", () => {
    expect(fnv1a("")).toBe(0x811c9dc5);
    expect(fnv1a("a")).toBe(0xe40c292c);
  });

  it("derives the same two-word name the daemon reports for a session id", () => {
    // Values computed from the .txt lists with the Rust derivation (hash % adjectives, hash / adjectives % nouns).
    expect(baseSessionName("session-1")).toBe("sharp-canyon");
    expect(baseSessionName("4f1c2e9a-7b3d-4c11-9e2a-0d5b6c7e8f90")).toBe("dusky-hill");
    expect(baseSessionName("anything")).toMatch(/^[a-z]+-[a-z]+$/);
  });

  it("keeps 300 names in one workspace unique with numbered suffixes", () => {
    const taken: string[] = [];
    for (let i = 0; i < 300; i += 1) taken.push(uniqueSessionName(`session-${i}`, taken));
    expect(new Set(taken).size).toBe(300);
    for (const name of taken) expect(name).toMatch(/^[a-z]+-[a-z]+(?:-\d+)?$/);
  });

  it("suffixes a collision with -2, then -3", () => {
    const base = baseSessionName("x");
    expect(uniqueSessionName("x", [base])).toBe(`${base}-2`);
    expect(uniqueSessionName("x", [base, `${base}-2`])).toBe(`${base}-3`);
  });
});
