import { afterEach, describe, expect, it } from "vitest";
import { cleanup } from "@testing-library/react";
import {
  INITIAL_RECALL,
  isOnFirstVisualLine,
  isOnLastVisualLine,
  stepRecall,
} from "./recallHistory";

afterEach(cleanup);

const prompts = ["first prompt", "second prompt", "third prompt"];

describe("prompt recall", () => {
  it("recalls the newest prompt from an empty composer and walks backwards", () => {
    let state = stepRecall({ state: INITIAL_RECALL, prompts, direction: "up", composerEmpty: true, text: "", caret: 0 });
    expect(state).toEqual({ recalled: "third prompt", index: 2 });
    state = stepRecall({ state: state!, prompts, direction: "up", composerEmpty: false, text: "third prompt", caret: 0 });
    expect(state).toEqual({ recalled: "second prompt", index: 1 });
    state = stepRecall({ state: state!, prompts, direction: "up", composerEmpty: false, text: "second prompt", caret: 0 });
    expect(state).toEqual({ recalled: "first prompt", index: 0 });
    // the oldest prompt holds
    expect(stepRecall({ state: state!, prompts, direction: "up", composerEmpty: false, text: "first prompt", caret: 0 })).toBeNull();
  });

  it("moves forward with ArrowDown and clears past the newest", () => {
    const state = { recalled: "second prompt", index: 1 };
    const forward = stepRecall({ state, prompts, direction: "down", composerEmpty: false, text: "second prompt", caret: 13 });
    expect(forward).toEqual({ recalled: "third prompt", index: 2 });
    const cleared = stepRecall({ state: forward!, prompts, direction: "down", composerEmpty: false, text: "third prompt", caret: 12 });
    expect(cleared).toEqual(INITIAL_RECALL);
  });

  it("recall takes over only while the caret sits on the boundary visual line", () => {
    const multiline = prompts[2] + "\nsecond line";
    const state = { recalled: multiline, index: 2 };
    // caret on the first line: ArrowUp walks back, ArrowDown moves the caret as usual
    expect(stepRecall({ state, prompts, direction: "up", composerEmpty: false, text: multiline, caret: 3 })).toEqual({ recalled: prompts[1], index: 1 });
    expect(stepRecall({ state, prompts, direction: "down", composerEmpty: false, text: multiline, caret: 3 })).toBeNull();
    // caret on the last line: the reverse
    const caret = multiline.length;
    // The newest prompt is already on screen, so ArrowDown clears the composer.
    expect(stepRecall({ state, prompts, direction: "down", composerEmpty: false, text: multiline, caret })).toEqual(INITIAL_RECALL);
    expect(stepRecall({ state, prompts, direction: "up", composerEmpty: false, text: multiline, caret })).toBeNull();
  });

  it("editing a recalled prompt turns it into a normal draft", () => {
    const state = { recalled: "second prompt", index: 1 };
    expect(
      stepRecall({ state, prompts, direction: "up", composerEmpty: false, text: "second prompt edited", caret: 0 }),
    ).toBeNull();
  });

  it("ignores ArrowDown in an empty composer and a thread with no prompts", () => {
    expect(stepRecall({ state: INITIAL_RECALL, prompts: [], direction: "up", composerEmpty: true, text: "", caret: 0 })).toBeNull();
    expect(stepRecall({ state: INITIAL_RECALL, prompts, direction: "down", composerEmpty: true, text: "", caret: 0 })).toBeNull();
  });

  it("counts visual lines by source newlines", () => {
    expect(isOnFirstVisualLine("abc\ndef", 2)).toBe(true);
    expect(isOnFirstVisualLine("abc\ndef", 5)).toBe(false);
    expect(isOnLastVisualLine("abc\ndef", 5)).toBe(true);
    expect(isOnLastVisualLine("abc\ndef", 2)).toBe(false);
  });
});
