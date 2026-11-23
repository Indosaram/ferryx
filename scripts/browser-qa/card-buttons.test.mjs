import { describe, expect, test } from "bun:test";

import { cardButtonPoint } from "./card-buttons.mjs";

// Word boxes as tesseract reported them on the Windows pair card of run 45024452034
// (screenshots/04-agent-pair-card.png, 1920x1080). The card reads:
//
//   Allow "ferryxqa" to control the browser?      y=163, so the title's "Allow" is at x=547
//   A local program asks to pair as a browser ...  y=196/216 (body, two lines)
//                                    Deny  Allow   y=228..255, Allow pill x=904..958
//
// That run clicked (581,167): the title's "Allow", not the button at (931,241). The card then
// expired unapproved, and the audit still recorded a successful click.
const word = (text, left, top, width = 40, height = 15) => ({ text, left, top, width, height });

const titleLine = [
  word("Allow", 547, 163, 46),
  word('"ferryxqa"', 601, 163, 92),
  word("to", 701, 163, 16),
  word("control", 725, 163, 56),
  word("the", 789, 163, 27),
  word("browser?", 824, 163, 74),
];
const bodyLines = [
  word("A", 547, 196, 12), word("local", 566, 196, 38), word("program", 612, 196, 62), word("asks", 682, 196, 32),
  word("to", 547, 216, 16), word("pair", 571, 216, 30), word("as", 609, 216, 18),
];
// Windows OCR drops the dark "Deny" button (the audit's own comment) — that omission is the bug's
// trigger, so the primary-only variants below are the ones that must not fall back to the title.
const buttonRowWithDeny = [word("Deny", 854, 241, 44), word("Allow", 904, 241, 54)];
const buttonRowAllowOnly = [word("Allow", 904, 241, 54)];

const card = (buttons) => [...titleLine, ...bodyLines, ...buttons];

describe("the pair card's Allow button, not the title's Allow", () => {
  const titleTop = titleLine[0].top;

  test("Deny beside Allow pins the button even when the title is unreadable", () => {
    const located = cardButtonPoint(card(buttonRowWithDeny), "Allow");
    expect(located.at).toEqual([931, 249]);
    expect(located.via).toBe("ocr (right of Deny)");
  });

  test("without Deny the title's Allow is rejected and the button below it kept", () => {
    const located = cardButtonPoint(card(buttonRowAllowOnly), "Allow", { titleTop });
    // (931, 249), not (570,170) where run 45024452034 clicked.
    expect(located.at).toEqual([931, 249]);
    expect(located.via).toBe("ocr (label alone on its row)");
    expect(located.rejected).toEqual([{ at: [570, 171], why: "on the card title line" }]);
  });

  test("H1: a lone label word with no sibling and no title line proves nothing", () => {
    // The archived frame's real OCR, as the evidence file reports it: tesseract returned exactly
    // one "Allow" - the title's own word - and dropped both the rest of the title and dark
    // "Deny". That word is not provably a button, so no click is offered at all; the caller's
    // primaryButtonBelow pixel fallback must run instead of clicking the title at (583,167).
    const lone = [{ text: "Allow", left: 564, top: 152, width: 37, height: 29 }];
    const located = cardButtonPoint(lone, "Allow", { titleTop: null });
    expect(located.at).toBeNull();
    expect(located.via).toBe("none");
    expect(located.rejected).toEqual([{ at: [583, 167], why: "alone on its row with no title line read" }]);
    // Same verdict for the shipped default (titleTop omitted) and for a sibling-less title line.
    expect(cardButtonPoint(lone, "Allow").at).toBeNull();
    expect(cardButtonPoint(card(buttonRowAllowOnly), "Allow").at).toBeNull();
  });

  test("a title-only card yields no click rather than the title", () => {
    const located = cardButtonPoint([...titleLine, ...bodyLines], "Allow", { titleTop });
    expect(located.at).toBeNull();
    expect(located.via).toBe("none");
    expect(located.rejected.map((entry) => entry.why)).toEqual(["on the card title line"]);
  });

  test("the old topmost-match rule would have clicked the title", () => {
    // Guards the regression: the first Allow by top is the title's.
    const topmost = card(buttonRowAllowOnly).filter((entry) => entry.text === "Allow").sort((a, b) => a.top - b.top)[0];
    expect(topmost.top).toBe(163);
    expect(cardButtonPoint(card(buttonRowAllowOnly), "Allow", { titleTop }).at).not.toEqual([
      topmost.left + Math.round(topmost.width / 2),
      topmost.top + Math.round(topmost.height / 2),
    ]);
  });
});

describe("the grant card's Approve button", () => {
  const grantTitle = [word("Approve", 547, 163, 66), word("access", 621, 163, 48), word("to", 677, 163, 16), word("example.test", 701, 163, 90)];

  test("Cancel beside Approve pins it", () => {
    const located = cardButtonPoint([...grantTitle, word("Cancel", 830, 241, 52), word("Approve", 890, 241, 66)], "Approve");
    expect(located.at).toEqual([923, 249]);
    expect(located.via).toBe("ocr (right of Cancel)");
  });

  test("Keep left of Approve pins it, matching the tickKeep row", () => {
    const located = cardButtonPoint([...grantTitle, word("Keep", 780, 241, 34), word("Approve", 890, 241, 66)], "Approve");
    expect(located.at).toEqual([923, 249]);
    expect(located.via).toBe("ocr (right of Keep)");
  });

  test("H2: a wrapped title word above the button row never outranks the real button", () => {
    // The title wraps, so its "Approve" ends the first line miles away from "access" on the
    // second; the button row below is Cancel + Approve. Ranking by top alone chose (580,171) -
    // the title - and *rejected* the real button as "right of Cancel".
    const wrapped = [word("Approve", 547, 163, 66), word("access", 621, 196, 48), word("to", 677, 196, 16), word("Cancel", 830, 241, 52), word("Approve", 890, 241, 66)];
    const located = cardButtonPoint(wrapped, "Approve");
    expect(located.at).toEqual([923, 249]);
    expect(located.via).toBe("ocr (right of Cancel)");
    // The title word is reported as rejected, never silently dropped.
    expect(located.rejected).toEqual([{ at: [580, 171], why: "alone on its row with no title line read" }]);
  });

  test("H2: with the title line read, a lone word above it still loses to the row below", () => {
    // Same card, but the caller read the wrapped title's top line: the word on it is card text.
    const wrapped = [word("Approve", 547, 163, 66), word("access", 621, 196, 48), word("Cancel", 830, 241, 52), word("Approve", 890, 241, 66)];
    const located = cardButtonPoint(wrapped, "Approve", { titleTop: 163 });
    expect(located.at).toEqual([923, 249]);
    expect(located.via).toBe("ocr (right of Cancel)");
    expect(located.rejected).toEqual([{ at: [580, 171], why: "on the card title line" }]);
  });

  test("H2: a lone label below the title band is accepted, one above it is not", () => {
    // No sibling at all, so only the title line decides. OCR drops both "Deny" and "Cancel".
    const lone = [word("Approve", 547, 163, 66), word("access", 621, 163, 48), word("Approve", 890, 241, 66)];
    expect(cardButtonPoint(lone, "Approve", { titleTop: 163 }).at).toEqual([923, 249]);
    expect(cardButtonPoint(lone, "Approve", { titleTop: 241 }).at).toBeNull();
  });

  test("two stacked button rows pick the bottom row, where the action buttons are", () => {
    const rows = [word("Deny", 854, 241, 44), word("Allow", 904, 241, 54), word("Deny", 854, 300, 44), word("Allow", 904, 300, 54)];
    expect(cardButtonPoint(card(rows), "Allow").at).toEqual([931, 308]);
  });

  test("a sibling-anchored button beats a lone label word even when it sits lower down", () => {
    // Tier outranks position: a title-ish lone word on its own row above must not win over the
    // row that is positively identified by its sibling.
    const mixed = [word("Approve", 547, 163, 66), word("access", 621, 196, 48), word("Approve", 890, 196, 66), word("Cancel", 830, 300, 52), word("Approve", 890, 300, 66)];
    const located = cardButtonPoint(mixed, "Approve", { titleTop: 163 });
    expect(located.at).toEqual([923, 308]);
    expect(located.via).toBe("ocr (right of Cancel)");
  });
});

describe("word boxes", () => {
  test("a label word from another row is not a sibling", () => {
    // "Deny" on the body line must not promote the title's "Allow".
    const located = cardButtonPoint([...titleLine, word("Deny", 547, 196, 44), ...buttonRowAllowOnly], "Allow", { titleTop: 163 });
    expect(located.at).toEqual([931, 249]);
  });

  test("boxes without geometry are ignored, not clicked at NaN", () => {
    const located = cardButtonPoint([{ text: "Allow" }, null, ...card(buttonRowAllowOnly)], "Allow", { titleTop: 163 });
    expect(located.at).toEqual([931, 249]);
    expect(located.at.every((value) => Number.isFinite(value))).toBe(true);
  });

  test("no words at all yields no click", () => {
    expect(cardButtonPoint([], "Allow").at).toBeNull();
    expect(cardButtonPoint(null, "Allow").at).toBeNull();
  });
});
