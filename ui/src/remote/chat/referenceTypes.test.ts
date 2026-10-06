import { describe, expect, it } from "vitest";
import type { AttachmentReceipt, DeliveryStage, TargetRef } from "../../lib/scopedContracts";
import {
  REFERENCE_CHAT_ROUTE_PREFIX,
  REFERENCE_OUTCOME_UNKNOWN_CODE,
  REFERENCE_SCROLLBACK_DISCLOSURE,
  REFERENCE_SUBMIT_MAX_CHARS,
  referenceAnswerIsSingleChoice,
  referenceAnswerVariantCount,
  referenceCanReachProviderRead,
  referenceChatRoute,
  referenceCursorKey,
  referenceCursorMatches,
  referenceDraftKey,
  referenceHistoryDisclosure,
  referenceHistoryIsNative,
  referenceIsOutcomeUnknown,
  referenceKeyStepIsEffective,
  referenceMentionFor,
  referenceNativeKindFromRegistryId,
  referenceNativeKindIsNative,
  referenceNativeKindOfSource,
  referencePartIsInlineText,
  referencePromptNeedsConfirmation,
  referenceSelectableIndices,
  referenceStageAtLeast,
  referenceStageRank,
  referenceStopIsRefusal,
  referenceTargetHasProviderSession,
  referenceTargetKey,
  sameReferenceTarget,
  type ReferenceFileReceipt,
  type ReferenceHistoryPage,
  type ReferencePart,
  type ReferencePartKind,
  type ReferencePrompt,
  type ReferencePromptAnswer,
  type ReferenceTargetRef,
} from "./referenceTypes";

const target: TargetRef = {
  hostId: "host-a",
  ownerId: "owner-a",
  epoch: "18446744073709551615",
  backendSessionId: "sess-1",
};

function boundTarget(providerSessionId?: string | null): ReferenceTargetRef {
  return { target, providerSessionId };
}

function page(overrides: Partial<ReferenceHistoryPage>): ReferenceHistoryPage {
  return {
    source: "claude-transcript",
    availability: "native",
    turns: [],
    cursor: null,
    hasMore: false,
    generation: "gen-1",
    unavailableReason: null,
    ...overrides,
  };
}

describe("reference history page", () => {
  it("discloses nothing for a native page and allows providerRead only with an identified session", () => {
    const native = page({});
    expect(referenceHistoryIsNative(native)).toBe(true);
    expect(referenceHistoryDisclosure(native)).toBeNull();
    expect(referenceCanReachProviderRead(native, boundTarget("provider-7"))).toBe(true);
    expect(referenceCanReachProviderRead(native, boundTarget(null))).toBe(false);
    expect(referenceCanReachProviderRead(native, boundTarget("   "))).toBe(false);
  });

  it("labels a scrollback page with the reference disclosure and never claims providerRead", () => {
    const scrollback = page({
      source: "scrollback",
      availability: "scrollback",
      cursor: null,
      hasMore: false,
      unavailableReason: "no native reader for this pane",
    });
    expect(referenceHistoryIsNative(scrollback)).toBe(false);
    expect(referenceHistoryDisclosure(scrollback)).toBe(REFERENCE_SCROLLBACK_DISCLOSURE);
    expect(referenceCanReachProviderRead(scrollback, boundTarget("provider-7"))).toBe(false);
  });

  it("labels a not-started session as an empty conversation rather than a missing one", () => {
    const notStarted = page({
      source: "omo-transcript",
      availability: "notStarted",
      unavailableReason: "session_not_written",
    });
    expect(referenceHistoryDisclosure(notStarted)).not.toBeNull();
    expect(referenceHistoryDisclosure(notStarted)).not.toBe(REFERENCE_SCROLLBACK_DISCLOSURE);
    expect(notStarted.turns).toHaveLength(0);
  });

  it("round-trips a serialized page fixture without losing the source or generation", () => {
    const fixture = page({
      source: "pi-transcript",
      generation: "gen-9",
      cursor: { streamId: "dev:ino", offset: 4096 },
      hasMore: true,
      turns: [
        { role: "user", parts: [{ kind: "text", text: "hi" }] },
        {
          role: "assistant",
          parts: [
            { kind: "thinking", text: "hmm" },
            { kind: "tool", name: "read", summary: "s", input: "i", output: "cut", outputRef: "r1", outputSize: 900 },
            { kind: "compact", text: "summary" },
          ],
          abandoned: { count: 2, branches: 1, summary: "walked away" },
        },
      ],
    });
    const decoded = JSON.parse(JSON.stringify(fixture)) as ReferenceHistoryPage;
    expect(decoded).toEqual(fixture);
    expect(decoded.source).toBe("pi-transcript");
    expect(decoded.generation).toBe("gen-9");
    expect(decoded.cursor?.offset).toBe(4096);
    expect(decoded.turns[1]?.abandoned?.count).toBe(2);
  });
});

describe("reference part union", () => {
  const kinds: readonly ReferencePartKind[] = [
    "text",
    "thinking",
    "skill",
    "tool",
    "image",
    "compact",
    "notice",
    "taskResult",
  ];

  const parts: readonly ReferencePart[] = [
    { kind: "text", text: "hi", phase: "finalAnswer" },
    { kind: "thinking", text: "hmm" },
    {
      kind: "skill",
      skill: { name: "debugging", evidence: "invocation", status: "requested" },
    },
    { kind: "tool", name: "read", summary: "s", input: "i", output: "o" },
    { kind: "image", mediaType: "image/png", ref: "img-1" },
    { kind: "compact", text: "summary" },
    { kind: "notice", text: "job done", source: "async-result" },
    {
      kind: "taskResult",
      tasks: [{ id: "t1", title: "task one", status: "completed", result: "ok" }],
    },
  ];

  it("covers every kind in the union and round-trips each part", () => {
    expect(parts.map((part) => part.kind)).toEqual([...kinds]);
    for (const part of parts) {
      expect(JSON.parse(JSON.stringify(part))).toEqual(part);
    }
  });

  it("classifies only prose, reasoning and compaction as inline text", () => {
    const inline = parts.filter(referencePartIsInlineText).map((part) => part.kind);
    expect(inline).toEqual(["text", "thinking", "compact"]);
  });
});

describe("reference target identity", () => {
  it("keys on host, owner, epoch and backend session, not on the provider session", () => {
    const bare = boundTarget(null);
    const bound = boundTarget("provider-7");
    expect(referenceTargetKey(bare)).toBe(referenceTargetKey(bound));
    expect(referenceDraftKey(bare)).toBe(referenceDraftKey(bound));
    expect(sameReferenceTarget(bare, bound)).toBe(true);
    expect(referenceTargetHasProviderSession(bare)).toBe(false);
    expect(referenceTargetHasProviderSession(bound)).toBe(true);
  });

  it("rejects a wrong target: another owner, another epoch or another session is a different pane", () => {
    const base = boundTarget("provider-7");
    const otherOwner = { target: { ...target, ownerId: "owner-b" }, providerSessionId: "provider-7" };
    const otherEpoch = { target: { ...target, epoch: "7" }, providerSessionId: "provider-7" };
    const otherSession = {
      target: { ...target, backendSessionId: "sess-2" },
      providerSessionId: "provider-7",
    };
    for (const wrong of [otherOwner, otherEpoch, otherSession]) {
      expect(sameReferenceTarget(base, wrong)).toBe(false);
      expect(referenceDraftKey(base)).not.toBe(referenceDraftKey(wrong));
    }
  });
});

describe("reference history cursor", () => {
  it("matches only its own stream and serializes to a stable key", () => {
    const cursor = { streamId: "dev:ino", offset: 4096 };
    expect(referenceCursorMatches(cursor, "dev:ino")).toBe(true);
    expect(referenceCursorMatches(cursor, "dev:other")).toBe(false);
    expect(referenceCursorKey(cursor)).toBe("dev:ino:4096");
  });
});

describe("delivery ladder", () => {
  it("ranks the three stages in order", () => {
    expect(referenceStageRank("staged")).toBe(0);
    expect(referenceStageRank("accepted")).toBe(1);
    expect(referenceStageRank("providerRead")).toBe(2);
    expect(referenceStageAtLeast("accepted", "staged")).toBe(true);
    expect(referenceStageAtLeast("providerRead", "accepted")).toBe(true);
    expect(referenceStageAtLeast("staged", "staged")).toBe(true);
    expect(referenceStageAtLeast("staged", "accepted")).toBe(false);
    expect(referenceStageAtLeast("accepted", "providerRead")).toBe(false);
  });

  it("rejects an unknown receipt stage instead of letting it pass as accepted", () => {
    const unknown = "providerConsumed" as unknown as DeliveryStage;
    expect(referenceStageAtLeast(unknown, "staged")).toBe(false);
    expect(referenceStageAtLeast(unknown, "accepted")).toBe(false);
  });

  it("treats only an explicit refusal as a refusal", () => {
    expect(referenceStopIsRefusal("refused")).toBe(true);
    expect(referenceStopIsRefusal("providerInterrupt")).toBe(false);
    expect(referenceStopIsRefusal("shellSignal")).toBe(false);
  });

  it("recognises the accept-then-unknown code", () => {
    expect(referenceIsOutcomeUnknown(REFERENCE_OUTCOME_UNKNOWN_CODE)).toBe(true);
    expect(referenceIsOutcomeUnknown("TIMEOUT")).toBe(false);
  });
});

describe("reference prompt answers", () => {
  const prompt: ReferencePrompt = {
    id: "p1",
    agent: "claude",
    kind: "approval",
    title: "Ready?",
    question: "Ready?",
    body: null,
    options: [{ label: "Yes" }, { label: "No" }, { label: "Other" }],
    multiSelect: false,
    customOptionIndex: 2,
    queued: null,
    steps: [],
    fallback: null,
  };

  it("accepts exactly one answer shape and refuses empty or ambiguous answers", () => {
    expect(referenceAnswerIsSingleChoice({ optionIndex: 0 })).toBe(true);
    expect(referenceAnswerIsSingleChoice({ optionIndices: [0, 2] })).toBe(true);
    expect(referenceAnswerIsSingleChoice({ customText: "mine" })).toBe(true);
    expect(referenceAnswerIsSingleChoice({})).toBe(false);
    expect(referenceAnswerVariantCount({})).toBe(0);
    const ambiguous: ReferencePromptAnswer = { optionIndex: 0, customText: "mine" };
    expect(referenceAnswerVariantCount(ambiguous)).toBe(2);
    expect(referenceAnswerIsSingleChoice(ambiguous)).toBe(false);
  });

  it("requires confirmation for option picks on approval, plan and menu prompts only", () => {
    const pick: ReferencePromptAnswer = { optionIndex: 0 };
    expect(referencePromptNeedsConfirmation(prompt, pick)).toBe(true);
    expect(referencePromptNeedsConfirmation({ ...prompt, kind: "menu" }, pick)).toBe(true);
    expect(referencePromptNeedsConfirmation({ ...prompt, kind: "plan" }, pick)).toBe(true);
    expect(referencePromptNeedsConfirmation({ ...prompt, kind: "question" }, pick)).toBe(false);
    expect(referencePromptNeedsConfirmation(prompt, { customText: "sure" })).toBe(false);
  });

  it("excludes the custom option from the numberable choices", () => {
    expect(referenceSelectableIndices(prompt)).toEqual([0, 1]);
    expect(referenceSelectableIndices({ ...prompt, customOptionIndex: null })).toEqual([0, 1, 2]);
  });
});

describe("reference registry mapping", () => {
  it("maps the six native registry ids and nothing else", () => {
    expect(referenceNativeKindFromRegistryId("claude")).toBe("claude");
    expect(referenceNativeKindFromRegistryId("  Codex ")).toBe("codex");
    expect(referenceNativeKindFromRegistryId("omp")).toBe("omp");
    expect(referenceNativeKindFromRegistryId("omo")).toBe("omo");
    expect(referenceNativeKindFromRegistryId("gjc")).toBe("gjc");
    expect(referenceNativeKindFromRegistryId("pi")).toBe("pi");
    for (const unknown of ["mimo-code", "cursor-agent", "prime-agent", "openclaw", ""]) {
      expect(referenceNativeKindFromRegistryId(unknown)).toBe("unavailable");
      expect(referenceNativeKindIsNative(referenceNativeKindFromRegistryId(unknown))).toBe(false);
    }
  });

  it("derives the reader from a history source, and unavailable from scrollback", () => {
    expect(referenceNativeKindOfSource("claude-transcript")).toBe("claude");
    expect(referenceNativeKindOfSource("codex-transcript")).toBe("codex");
    expect(referenceNativeKindOfSource("omp-transcript")).toBe("omp");
    expect(referenceNativeKindOfSource("omo-transcript")).toBe("omo");
    expect(referenceNativeKindOfSource("gjc-transcript")).toBe("gjc");
    expect(referenceNativeKindOfSource("pi-transcript")).toBe("pi");
    expect(referenceNativeKindOfSource("scrollback")).toBe("unavailable");
  });
});

describe("reference files, routes and key steps", () => {
  it("reuses the scoped attachment receipt and renders an editable mention", () => {
    const receipt: AttachmentReceipt = {
      hostId: "h",
      attachmentId: "opaque-1",
      sha256: "hash",
      sizeBytes: 12,
      mediaType: "image/png",
    };
    const file: ReferenceFileReceipt = {
      receipt,
      displayName: "shot.png",
      mentionText: referenceMentionFor("/tmp/opaque-1"),
    };
    expect(file.receipt.attachmentId).toBe("opaque-1");
    expect(file.mentionText).toBe("@/tmp/opaque-1 ");
    expect(JSON.parse(JSON.stringify(file))).toEqual(file);
  });

  it("builds the frozen route shape", () => {
    expect(referenceChatRoute("s1", "history")).toBe(`${REFERENCE_CHAT_ROUTE_PREFIX}/s1/history`);
    expect(referenceChatRoute("s1", "/files/f1")).toBe(`${REFERENCE_CHAT_ROUTE_PREFIX}/s1/files/f1`);
    expect(referenceChatRoute("s1")).toBe(`${REFERENCE_CHAT_ROUTE_PREFIX}/s1`);
    expect(REFERENCE_CHAT_ROUTE_PREFIX).toBe("/api/v1/reference-chat");
    expect(REFERENCE_SUBMIT_MAX_CHARS).toBe(20_000);
  });

  it("rejects an empty key step", () => {
    expect(referenceKeyStepIsEffective({ keys: ["Enter"] })).toBe(true);
    expect(referenceKeyStepIsEffective({ text: "2" })).toBe(true);
    expect(referenceKeyStepIsEffective({ text: "" })).toBe(false);
    expect(referenceKeyStepIsEffective({ keys: [] })).toBe(false);
    expect(referenceKeyStepIsEffective({})).toBe(false);
  });
});
