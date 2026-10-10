/**
 * Ported from the pinned upstream `src/lib/promptAnswer.test.ts`
 * (`devswha/herdr-web-ui` @ `54e5a1f6…`, MIT). Authored, NOT EXECUTED — execution override.
 */
import { describe, expect, it } from "vitest";
import {
  REFERENCE_PROMPT_CHANGED_CODE,
  REFERENCE_PROMPT_STALE_CODE,
  REFERENCE_PROMPT_STALE_MESSAGE,
  referenceAnswerFromText,
  referenceAnswerHint,
  referenceAnswerRefusal,
  referenceFocusFollowsAnswer,
  referenceNeedsConfirmation,
  referencePressOrigin,
  referencePromptErrorIsStale,
  type ReferencePressOrigin,
} from "./referencePromptAnswer";
import type { ReferencePrompt } from "./referenceTypes";

function prompt(
  options: readonly string[],
  custom: number | null,
  multi = false,
  kind: ReferencePrompt["kind"] = "question",
): ReferencePrompt {
  return {
    id: "p",
    agent: "claude",
    kind,
    title: "Question",
    question: "?",
    body: null,
    options: options.map((label) => ({ label, description: null })),
    multiSelect: multi,
    customOptionIndex: custom,
    queued: null,
    steps: [],
    fallback: null,
  };
}

describe("answering a prompt from the chat", () => {
  it("reads the agent's option numbers, labels and bound letters, else the typed reply", () => {
    const question = prompt(["LM-O (Recommended)", "YCB-V"], 2);
    expect(referenceAnswerFromText(question, "2")).toEqual({ optionIndex: 1 });
    expect(referenceAnswerFromText(question, " lm-o ")).toEqual({ optionIndex: 0 });
    expect(referenceAnswerFromText(question, "LM-O (Recommended)")).toEqual({ optionIndex: 0 });
    // the "type something" row is answered with text, not picked by its number
    expect(referenceAnswerFromText(question, "3")).toEqual({ customText: "3" });
    expect(referenceAnswerFromText(question, "use T-LESS instead")).toEqual({
      customText: "use T-LESS instead",
    });
    expect(referenceAnswerHint(question)).toBe("Type 1–2 or your own reply…");
  });

  it("answers a free-form question (Codex's queue) with the text itself, numbers included", () => {
    const freeForm = prompt([], 0);
    expect(referenceAnswerFromText(freeForm, "1")).toEqual({ customText: "1" });
    expect(referenceAnswerFromText(freeForm, "keep the logs")).toEqual({ customText: "keep the logs" });
    expect(referenceAnswerHint(freeForm)).toBe("Type your reply…");
    expect(referenceAnswerHint(prompt(["A", "B", "C"], null, true))).toBe(
      "Type the numbers you choose, e.g. 1 3",
    );
    // one option beside the custom answer: its number, not a range
    expect(referenceAnswerHint(prompt(["LM-O"], 1))).toBe("Type 1 or your own reply…");
  });

  it("takes only an option for an approval", () => {
    const approval = prompt(
      [
        "Yes, proceed (y)",
        "Yes, and don't ask again (p)",
        "No, and tell Codex what to do differently (esc)",
      ],
      null,
      false,
      "approval",
    );
    expect(referenceAnswerFromText(approval, "y")).toEqual({ optionIndex: 0 });
    expect(referenceAnswerFromText(approval, "P")).toEqual({ optionIndex: 1 });
    expect(
      referenceAnswerFromText(approval, "no, and tell codex what to do differently"),
    ).toEqual({ optionIndex: 2 });
    expect(referenceAnswerFromText(approval, "4")).toBeNull();
    expect(referenceAnswerFromText(approval, "maybe later")).toBeNull();
    expect(referenceAnswerHint(approval)).toBe("Type 1–3 to choose…");
    expect(referenceAnswerRefusal(approval)).toBe("Choose one of the options above: type 1–3.");
    // typed, an approval's pick waits for Confirm; a question's does not
    expect(referenceNeedsConfirmation(approval, { optionIndex: 0 })).toBe(true);
    expect(referenceNeedsConfirmation(prompt(["LM-O"], 1), { optionIndex: 0 })).toBe(false);
    expect(
      referenceNeedsConfirmation(prompt(["Yes", "No", "Tell Claude what to change"], 2, false, "plan"), {
        customText: "shorter",
      }),
    ).toBe(false);
    // Claude's review submits every answer, a Codex menu continues or stops: a typed pick waits too
    expect(
      referenceNeedsConfirmation(prompt(["Submit answers", "Cancel"], null, false, "menu"), {
        optionIndex: 0,
      }),
    ).toBe(true);
  });

  it("skips a plan's custom row inside the options and reads several numbers for a multiple choice", () => {
    const plan = prompt(
      ["Yes, auto-accept edits", "Yes, manually approve edits", "No", "Tell Claude what to change"],
      3,
    );
    expect(referenceAnswerFromText(plan, "4")).toEqual({ customText: "4" });
    expect(referenceAnswerFromText(plan, "keep the intro")).toEqual({ customText: "keep the intro" });
    const multi = prompt(["LM-O", "YCB-V", "T-LESS"], null, true);
    expect(referenceAnswerFromText(multi, "1, 3")).toEqual({ optionIndices: [0, 2] });
    expect(referenceAnswerFromText(multi, "1 1 2")).toEqual({ optionIndices: [0, 1] });
    expect(referenceAnswerFromText(multi, "1 and 3")).toBeNull();
    expect(referenceAnswerRefusal(multi)).toBe("Choose with the option numbers above, e.g. 1 3.");
  });

  it("treats a missing custom row as absent, where the pinned module tested against null alone", () => {
    const noCustom: ReferencePrompt = { ...prompt(["A", "B"], null), customOptionIndex: undefined };
    expect(referenceAnswerFromText(noCustom, "unlisted text")).toBeNull();
    expect(referenceAnswerHint(noCustom)).toBe("Type 1–2 to choose…");
  });
});

describe("referencePressOrigin", () => {
  it("takes the pointer a click names", () => {
    for (const pointerType of ["mouse", "touch", "pen"] as const) {
      expect(referencePressOrigin({ pointerType, detail: 1, keyed: false })).toBe(pointerType);
      // a keydown left over on the button does not make a pointer's click a key
      expect(referencePressOrigin({ pointerType, detail: 1, keyed: true })).toBe(pointerType);
    }
  });

  it("takes the pointer that went down on the button, and a finger or a pen over a mouse", () => {
    // a browser whose click names no pointer
    for (const downType of ["mouse", "touch", "pen"] as const) {
      expect(referencePressOrigin({ downType, detail: 1, keyed: false })).toBe(downType);
    }
    // iOS Safari has called a tap's click a mouse's
    expect(referencePressOrigin({ downType: "touch", pointerType: "mouse", detail: 1, keyed: false })).toBe("touch");
    expect(referencePressOrigin({ downType: "pen", pointerType: "mouse", detail: 1, keyed: false })).toBe("pen");
    expect(referencePressOrigin({ downType: "mouse", pointerType: "touch", detail: 1, keyed: false })).toBe("touch");
    expect(referencePressOrigin({ downType: "mouse", pointerType: "mouse", detail: 1, keyed: false })).toBe("mouse");
  });

  it("calls a click a key only with no click count and its keydown on the button", () => {
    // Chromium and Firefox: a PointerEvent with an empty pointer type; Safari: a MouseEvent with none
    expect(referencePressOrigin({ pointerType: "", detail: 0, keyed: true })).toBe("keyboard");
    expect(referencePressOrigin({ detail: 0, keyed: true })).toBe("keyboard");
    // a script's click() or an assistive technology's activation: no key went down
    expect(referencePressOrigin({ pointerType: "", detail: 0, keyed: false })).toBe("unknown");
    expect(referencePressOrigin({ detail: 0, keyed: false })).toBe("unknown");
  });

  it("does not guess at a counted click that names no pointer", () => {
    expect(referencePressOrigin({ detail: 1, keyed: false })).toBe("unknown");
    expect(referencePressOrigin({ pointerType: "", detail: 1, keyed: true })).toBe("unknown");
    expect(referencePressOrigin({ keyed: true })).toBe("unknown");
  });

  it("reads Enter in the card's own field, which has no click, as a key", () => {
    expect(referencePressOrigin(undefined)).toBe("keyboard");
  });
});

describe("referenceFocusFollowsAnswer", () => {
  const still = {
    fromCard: true,
    origin: "mouse" as ReferencePressOrigin,
    coarse: false,
    cardMounted: true,
    inCard: false,
    onPage: true,
  };

  it("hands the focus on when the pressed button lost it to the page, or still has it", () => {
    expect(referenceFocusFollowsAnswer(still)).toBe(true);
    expect(referenceFocusFollowsAnswer({ ...still, inCard: true, onPage: false })).toBe(true);
  });

  it("leaves the focus where the user put it while the answer was on its way", () => {
    expect(referenceFocusFollowsAnswer({ ...still, onPage: false })).toBe(false);
    expect(referenceFocusFollowsAnswer({ ...still, origin: "keyboard", coarse: true, onPage: false })).toBe(false);
  });

  it("does nothing for a card that is gone, or an answer that did not start in the card", () => {
    expect(referenceFocusFollowsAnswer({ ...still, cardMounted: false })).toBe(false);
    expect(referenceFocusFollowsAnswer({ ...still, fromCard: false })).toBe(false);
    expect(referenceFocusFollowsAnswer({ ...still, origin: "keyboard", coarse: true, cardMounted: false })).toBe(false);
    expect(referenceFocusFollowsAnswer({ ...still, origin: "keyboard", coarse: true, fromCard: false })).toBe(false);
  });

  // fine: a desktop, or a touch-screen laptop; coarse: a phone, or a tablet with a keyboard or a mouse
  const follows: Record<ReferencePressOrigin, { fine: boolean; coarse: boolean }> = {
    keyboard: { fine: true, coarse: true },
    mouse: { fine: true, coarse: true },
    touch: { fine: false, coarse: false },
    pen: { fine: false, coarse: false },
    unknown: { fine: true, coarse: false },
  };
  for (const [origin, expected] of Object.entries(follows) as [
    ReferencePressOrigin,
    { fine: boolean; coarse: boolean },
  ][]) {
    it(`${expected.fine ? "hands the focus on" : "stays out of the message box"} after a ${origin} press on a fine pointer`, () => {
      expect(referenceFocusFollowsAnswer({ ...still, origin, coarse: false })).toBe(expected.fine);
    });
    it(`${expected.coarse ? "hands the focus on" : "stays out of the message box"} after a ${origin} press on a coarse pointer`, () => {
      expect(referenceFocusFollowsAnswer({ ...still, origin, coarse: true })).toBe(expected.coarse);
    });
  }
});

describe("a stale screen", () => {
  it("is recognised by its structured code, never by a message", () => {
    expect(referencePromptErrorIsStale({ code: REFERENCE_PROMPT_STALE_CODE })).toBe(true);
    expect(referencePromptErrorIsStale({ code: REFERENCE_PROMPT_CHANGED_CODE })).toBe(true);
    expect(referencePromptErrorIsStale(new Error("REQUEST_CONFLICT"))).toBe(false);
    expect(referencePromptErrorIsStale(null)).toBe(false);
    expect(referencePromptErrorIsStale("REQUEST_CONFLICT")).toBe(false);
  });

  it("says what the card tells the user", () => {
    expect(REFERENCE_PROMPT_STALE_MESSAGE).toBe("the prompt changed — re-read");
  });
});
