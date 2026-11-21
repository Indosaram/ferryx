import { describe, expect, it } from "vitest";

import {
  CONTROL_CONFLICT_BODY,
  CONTROL_CONFLICT_HANDOFF_HINT,
  CONTROL_CONFLICT_TAKEOVER_HINT,
  CONTROL_CONFLICT_UNKNOWN_HOLDER,
  controlConflictCopy,
  holderDisplayName,
  terminalOutcomeCopy,
} from "./RemoteTerminal";

const conflict = (blockedDetail: string | null = null) => ({
  state: "blocked",
  blockedReason: "CONTROL_CONFLICT",
  blockedDetail,
  exitCode: null,
});

const loneSurrogate = /[\ud800-\udbff](?![\udc00-\udfff])|(?<![\ud800-\udbff])[\udc00-\udfff]/;

describe("terminal conflict copy", () => {
  it("changes only the recovery action when takeover becomes available", () => {
    const displaceable = controlConflictCopy(null, true);
    const fixed = controlConflictCopy(null, false);
    expect(displaceable).toContain(CONTROL_CONFLICT_BODY);
    expect(displaceable).toContain(CONTROL_CONFLICT_TAKEOVER_HINT);
    expect(displaceable).not.toContain(CONTROL_CONFLICT_HANDOFF_HINT);
    expect(fixed).toContain(CONTROL_CONFLICT_BODY);
    expect(fixed).toContain(CONTROL_CONFLICT_HANDOFF_HINT);
    expect(fixed).not.toContain(CONTROL_CONFLICT_TAKEOVER_HINT);
  });

  it("defaults a caller that passes no takeover flag to the no-takeover recovery", () => {
    expect(terminalOutcomeCopy(conflict("Phone"))).toBe(controlConflictCopy("Phone", false));
  });

  it("names the server fallback as the holder when the server sends none", () => {
    for (const detail of [null, "", "   "]) {
      for (const takeoverAvailable of [true, false]) {
        expect(controlConflictCopy(detail, takeoverAvailable))
          .toContain(`Held by ${CONTROL_CONFLICT_UNKNOWN_HOLDER}.`);
      }
    }
  });

  it("renders a holder whose real name is the server fallback like any other name", () => {
    expect(controlConflictCopy(CONTROL_CONFLICT_UNKNOWN_HOLDER, true))
      .toBe(controlConflictCopy(null, true));
    expect(controlConflictCopy(CONTROL_CONFLICT_UNKNOWN_HOLDER, false))
      .toBe(controlConflictCopy(null, false));
  });

  it("prints a real holder name", () => {
    expect(controlConflictCopy("Pixel 9 Pro", false)).toContain("Pixel 9 Pro");
    expect(controlConflictCopy("Pixel 9 Pro", false))
      .not.toContain(CONTROL_CONFLICT_UNKNOWN_HOLDER);
  });

  it("treats a name made only of invisible joiners as an unnamed holder", () => {
    for (const detail of ["\u200d", "\u200d\u200d", "\ufeff\u200d\ufe0f", "\u200c", "\ufe0f\u200d\u0301"]) {
      expect(holderDisplayName(detail)).toBe("");
      expect(controlConflictCopy(detail, true)).toBe(controlConflictCopy(null, true));
    }
  });

  it("never leaves a dangling joiner or variation selector at the cut", () => {
    const family = "\ud83d\udc68\u200d\ud83d\udc69\u200d\ud83d\udc67";
    for (const detail of [
      family.repeat(40),
      `a${family.repeat(40)}`,
      `${"x".repeat(77)}${family}`,
      `${"x".repeat(79)}\ufe0f`,
      `${"x".repeat(79)}\u200d`,
    ]) {
      const holder = holderDisplayName(detail);
      expect(holder.length).toBeLessThanOrEqual(80);
      expect(holder).not.toMatch(/[\u200d\ufe0e\ufe0f]$/);
      expect(holder).not.toMatch(loneSurrogate);
      expect(holder).toBe(holder.trim());
    }
  });

  it("keeps a bounded, grapheme-safe name without Intl.Segmenter", () => {
    const realSegmenter = (Intl as unknown as { Segmenter?: unknown }).Segmenter;
    try {
      delete (Intl as unknown as { Segmenter?: unknown }).Segmenter;
      expect(holderDisplayName("Pixel 9 Pro")).toBe("Pixel 9 Pro");
      expect(holderDisplayName("\ud83d\ude80".repeat(120))).toHaveLength(80);
      expect(holderDisplayName("\u200d\ufe0f")).toBe("");
      const cutoff = holderDisplayName(`${"x".repeat(79)}\ud83d\ude80`);
      expect(cutoff).toBe("x".repeat(79));
      expect(cutoff).not.toMatch(loneSurrogate);
    } finally {
      (Intl as unknown as { Segmenter?: unknown }).Segmenter = realSegmenter;
    }
  });

  it("reduces server holder text to one bounded display line without splitting a grapheme", () => {
    expect(holderDisplayName(null)).toBe("");
    expect(holderDisplayName(undefined)).toBe("");
    expect(holderDisplayName("  Ana's  laptop ")).toBe("Ana's laptop");
    const onlyControlAndBidi = ["\u202e\u0000\n", "\u0000\u202e\u001b"];
    for (const detail of onlyControlAndBidi) {
      expect(holderDisplayName(detail)).toBe("");
    }
    for (const detail of ["Phone\n\u202e\u001b[31m", "Tab\tName", "\ud83d\ude80".repeat(120),
      "a\ud83d\ude80b\ud83d\udc68\u200d\ud83d\udc69\u200d\ud83d\udc67".repeat(40), "x".repeat(300)]) {
      const holder = holderDisplayName(detail);
      expect(holder.length).toBeGreaterThan(0);
      expect(holder.length).toBeLessThanOrEqual(80);
      expect(holder).not.toMatch(/[\u0000-\u001f\u007f-\u009f\u202a-\u202e]/);
      expect(holder).not.toMatch(loneSurrogate);
      expect(holder).toBe(holder.trim());
    }
    expect(holderDisplayName("\ud83d\ude80".repeat(80))).toHaveLength(80);
    expect(holderDisplayName(`a${"\ud83d\ude80".repeat(60)}`)).toBe(`a${"\ud83d\ude80".repeat(39)}`);
  });

  it("keeps every blocked banner on exactly one line", () => {
    expect(controlConflictCopy("Phone\nsecond line", false))
      .toBe(controlConflictCopy("Phone second line", false));
    expect(controlConflictCopy("Phone\u202esecond", false))
      .toBe(controlConflictCopy("Phone second", false));
  });

  it("keeps the takeover hint out of every non-conflict banner", () => {
    for (const blockedReason of ["STALE_EPOCH", "SESSION_EXPIRED", "permission_denied", null]) {
      const copy = terminalOutcomeCopy(
        { state: "blocked", blockedReason, blockedDetail: "Phone", exitCode: null },
        true,
      );
      expect(copy).toBe(terminalOutcomeCopy(
        { state: "blocked", blockedReason, blockedDetail: null, exitCode: null },
        true,
      ));
      expect(copy).not.toContain("Phone");
      expect(copy).not.toContain(CONTROL_CONFLICT_UNKNOWN_HOLDER);
      expect(copy).not.toBe(controlConflictCopy("Phone", true));
    }
  });
});
