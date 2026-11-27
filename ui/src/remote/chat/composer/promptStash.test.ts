import { afterEach, describe, expect, it } from "vitest";
import { cleanup } from "@testing-library/react";
import {
  COMPOSER_STASH_LIMIT,
  COMPOSER_STASH_STORAGE_KEY,
  readStashes,
  removeStash,
  saveStash,
  stashableDraft,
  type StashedPrompt,
} from "./promptStash";

afterEach(cleanup);

function memoryStorage(): Storage & { data: Map<string, string> } {
  const data = new Map<string, string>();
  return {
    data,
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => void data.set(key, value),
    removeItem: (key: string) => void data.delete(key),
    clear: () => data.clear(),
    key: () => null,
    get length() {
      return data.size;
    },
  } as Storage & { data: Map<string, string> };
}

function entry(text: string, savedAt = 0): StashedPrompt {
  return { id: text, text, attachmentNames: [], savedAt };
}

describe("prompt stash", () => {
  it("persists stashes under the ferryx.* key, newest first", () => {
    const storage = memoryStorage();
    saveStash(storage, entry("one", 1));
    saveStash(storage, entry("two", 2));
    expect(storage.data.has(COMPOSER_STASH_STORAGE_KEY)).toBe(true);
    expect(readStashes(storage).map((row) => row.text)).toEqual(["two", "one"]);
  });

  it("cannot stash an empty draft and keeps the stash list bounded", () => {
    const storage = memoryStorage();
    expect(stashableDraft("", [])).toBeNull();
    expect(stashableDraft("   ", [])).toBeNull();
    expect(stashableDraft("", ["shot.png"])).not.toBeNull();
    for (let i = 0; i < COMPOSER_STASH_LIMIT + 3; i += 1) {
      saveStash(storage, entry(`p${i}`, i));
    }
    expect(readStashes(storage)).toHaveLength(COMPOSER_STASH_LIMIT);
    expect(readStashes(storage)[0]?.text).toBe(`p${COMPOSER_STASH_LIMIT + 2}`);
  });

  it("removes one stash and keeps the rest", () => {
    const storage = memoryStorage();
    saveStash(storage, entry("a"));
    saveStash(storage, entry("b"));
    const rest = removeStash(storage, "a");
    expect(rest.map((row) => row.text)).toEqual(["b"]);
    expect(readStashes(storage).map((row) => row.text)).toEqual(["b"]);
  });

  it("tolerates corrupt storage and a missing storage", () => {
    const storage = memoryStorage();
    storage.setItem(COMPOSER_STASH_STORAGE_KEY, "not json");
    expect(readStashes(storage)).toEqual([]);
    expect(readStashes(null)).toEqual([]);
    expect(saveStash(null, entry("x"))).toHaveLength(1);
  });
});
