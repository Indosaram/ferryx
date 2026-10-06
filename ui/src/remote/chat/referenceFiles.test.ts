/**
 * Adjacent tests for the reference chat's owning-host file lane (plan task 11).
 *
 * Authored, NOT EXECUTED - the execution override forbids running any check during authoring.
 * Deferred command: bun run --cwd ui test -- src/remote/chat/referenceFiles.test.ts
 *
 * Covered here: name and mention-path traversal, the symlink/containment fence, the frozen
 * bounds, staging, explicit cancel/delete cleanup, a failed send that keeps the draft and the
 * staged file, and the guarantee that a staged file never becomes a managed attachment.
 */
import { describe, expect, it } from "vitest";
import type { TargetRef } from "../../lib/scopedContracts";
import type { ReferenceFileStagePayload, ReferenceTargetRef } from "./referenceTypes";
import {
  REFERENCE_FILE_LIMITS,
  ReferenceFileError,
  cancelReferenceChatFile,
  deleteReferenceChatFile,
  parseReferenceFileReceipt,
  planReferenceFileSend,
  referenceAppendMention,
  referenceBase64,
  referenceFileMutationEnvelope,
  referenceFilePreviewRoute,
  referenceFileSendFailure,
  referenceFilesRoute,
  referenceMentionInsertion,
  referenceMentionPathIsSafe,
  referenceMentionPathOf,
  referenceMentionPathWithinRoot,
  stageReferenceChatFile,
  validateReferenceFileName,
  validateReferenceFileSize,
  validateReferenceTurnBounds,
  type ReferenceFileReceiptExpectation,
  type ReferenceFileSource,
  type ReferenceFileStagingDeps,
  type ReferenceFileStagingTransport,
} from "./referenceFiles";

const FIXED_SHA = "a".repeat(64);
const BACKSLASH = String.fromCharCode(92);

const base: TargetRef = {
  hostId: "host-a",
  ownerId: "owner-a",
  epoch: "18446744073709551615",
  backendSessionId: "sess-1",
};

function target(overrides: Partial<TargetRef> = {}): ReferenceTargetRef {
  return { target: { ...base, ...overrides } };
}

function source(name: string, type: string, bytes: number[]): ReferenceFileSource {
  const data = new Uint8Array(bytes);
  return {
    name,
    type,
    size: data.byteLength,
    arrayBuffer: async () => data.slice().buffer,
  };
}

function oversizedSource(size: number): ReferenceFileSource {
  return {
    name: "shot.png",
    type: "image/png",
    size,
    arrayBuffer: async () => new ArrayBuffer(0),
  };
}

function receiptBody(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    receipt: {
      hostId: "host-a",
      attachmentId: "att-1",
      sha256: FIXED_SHA,
      sizeBytes: 3,
      mediaType: "image/png",
    },
    displayName: "shot.png",
    mentionText: "@shot.png ",
    ...overrides,
  };
}

function expectation(
  overrides: Partial<ReferenceFileReceiptExpectation> = {},
): ReferenceFileReceiptExpectation {
  return {
    target: base,
    displayName: "shot.png",
    mediaType: "image/png",
    sizeBytes: 3,
    sha256: FIXED_SHA,
    ...overrides,
  };
}

function fakeTransport(overrides: Partial<ReferenceFileStagingTransport> = {}) {
  const staged: ReferenceFileStagePayload[] = [];
  const cancelled: string[] = [];
  const removed: string[] = [];
  const transport: ReferenceFileStagingTransport = {
    stage: async (_target, payload) => {
      staged.push(payload);
      return {
        receipt: {
          hostId: "host-a",
          attachmentId: "att-1",
          sha256: FIXED_SHA,
          sizeBytes: payload.sizeBytes,
          mediaType: payload.mediaType,
        },
        displayName: payload.name,
        mentionText: "@" + payload.name + " ",
      };
    },
    cancel: async (_target, attachmentId) => {
      cancelled.push(attachmentId);
      return true;
    },
    remove: async (_target, attachmentId) => {
      removed.push(attachmentId);
      return true;
    },
    ...overrides,
  };
  return { transport, staged, cancelled, removed };
}

function deps(transport: ReferenceFileStagingTransport): ReferenceFileStagingDeps {
  return { transport, digest: async () => FIXED_SHA };
}

function expectCode(run: () => unknown, code: string): void {
  try {
    run();
  } catch (error) {
    expect(error).toBeInstanceOf(ReferenceFileError);
    expect((error as ReferenceFileError).code).toBe(code);
    return;
  }
  throw new Error("expected a ReferenceFileError with code " + code);
}

async function expectCodeAsync(run: () => Promise<unknown>, code: string): Promise<void> {
  try {
    await run();
  } catch (error) {
    expect(error).toBeInstanceOf(ReferenceFileError);
    expect((error as ReferenceFileError).code).toBe(code);
    return;
  }
  throw new Error("expected a ReferenceFileError with code " + code);
}

const signal = new AbortController().signal;

describe("reference file names and mention paths", () => {
  it("refuses a name that is a path, a parent or a control byte", () => {
    for (const bad of [
      "",
      "   ",
      "../evil.png",
      "a/b.png",
      "..",
      ".",
      "a" + BACKSLASH + "b.png",
      "line" + String.fromCharCode(10) + "break.png",
    ]) {
      expectCode(() => validateReferenceFileName(bad), "INVALID_REQUEST");
    }
    expect(validateReferenceFileName("  shot.png  ")).toBe("shot.png");
    expect(validateReferenceFileName("a".repeat(255))).toBe("a".repeat(255));
    expectCode(() => validateReferenceFileName("a".repeat(256)), "INVALID_REQUEST");
  });

  it("refuses a mention path that is absolute, home-relative or has a parent component", () => {
    for (const good of ["shot.png", "sub/dir/shot.png", "a-b_c.1.png"]) {
      expect(referenceMentionPathIsSafe(good)).toBe(true);
    }
    for (const bad of [
      "",
      " ",
      "/etc/passwd",
      "~/secrets",
      "../outside.png",
      "sub/../../outside.png",
      "C:/windows",
      "sub/./shot.png",
      "a" + BACKSLASH + "b.png",
    ]) {
      expect(referenceMentionPathIsSafe(bad)).toBe(false);
    }
  });

  it("reads the path out of an editable mention, and only out of a plain one", () => {
    expect(referenceMentionPathOf("@shot.png ")).toBe("shot.png");
    expect(referenceMentionPathOf("@sub/shot.png")).toBe("sub/shot.png");
    expect(referenceMentionPathOf("shot.png")).toBeNull();
    expect(referenceMentionPathOf("@two words.png ")).toBeNull();
    expect(referenceMentionPathOf("@ ")).toBeNull();
  });
});

describe("reference file containment", () => {
  it("fences a host-resolved path against the owning root, segment-wise", () => {
    expect(referenceMentionPathWithinRoot("/wt/root", "/wt/root/sub/shot.png")).toBe(true);
    expect(referenceMentionPathWithinRoot("/wt/root", "/wt/root")).toBe(true);
    expect(referenceMentionPathWithinRoot("/wt/root", "/wt/root-other/shot.png")).toBe(false);
    expect(referenceMentionPathWithinRoot("/wt/root", "/etc/passwd")).toBe(false);
    expect(referenceMentionPathWithinRoot("/wt/root", "/wt/root/../escape")).toBe(false);
  });

  it("refuses a receipt whose resolved path escaped the owning root", () => {
    expectCode(
      () =>
        parseReferenceFileReceipt(
          receiptBody(),
          expectation({ root: "/wt/root", resolvedPath: "/wt/elsewhere/shot.png" }),
        ),
      "FORBIDDEN",
    );

    const contained = parseReferenceFileReceipt(
      receiptBody(),
      expectation({ root: "/wt/root", resolvedPath: "/wt/root/shot.png" }),
    );
    expect(contained.mentionText).toBe("@shot.png ");
  });

  it("refuses a receipt that carried a raw host path", () => {
    const withPath = {
      ...receiptBody(),
      receipt: { ...(receiptBody().receipt as Record<string, unknown>), path: "/tmp/secret.png" },
    };
    expectCode(() => parseReferenceFileReceipt(withPath, expectation()), "INVALID_REQUEST");

    const envelopePath = { ...receiptBody(), localPath: "/tmp/secret.png" };
    expectCode(() => parseReferenceFileReceipt(envelopePath, expectation()), "INVALID_REQUEST");
  });

  it("refuses a receipt from a foreign host or with a different digest", () => {
    expectCode(
      () =>
        parseReferenceFileReceipt(
          receiptBody({
            receipt: {
              hostId: "host-b",
              attachmentId: "att-1",
              sha256: FIXED_SHA,
              sizeBytes: 3,
              mediaType: "image/png",
            },
          }),
          expectation(),
        ),
      "FORBIDDEN",
    );
    expectCode(
      () => parseReferenceFileReceipt(receiptBody(), expectation({ sha256: "b".repeat(64) })),
      "INVALID_REQUEST",
    );
  });

  it("refuses a mention that names a different file, or names it unsafely", () => {
    expectCode(
      () => parseReferenceFileReceipt(receiptBody({ mentionText: "@other.png " }), expectation()),
      "INVALID_REQUEST",
    );
    expectCode(
      () => parseReferenceFileReceipt(receiptBody({ mentionText: "@/etc/passwd " }), expectation()),
      "INVALID_REQUEST",
    );
    expect(
      parseReferenceFileReceipt(receiptBody({ mentionText: "@sub/shot.png " }), expectation())
        .mentionText,
    ).toBe("@sub/shot.png ");
  });
});

describe("reference file bounds", () => {
  it("refuses a file past the per-file, per-turn count and per-turn byte limits", () => {
    expect(() => validateReferenceFileSize(REFERENCE_FILE_LIMITS.maxFileBytes)).not.toThrow();
    expectCode(
      () => validateReferenceFileSize(REFERENCE_FILE_LIMITS.maxFileBytes + 1),
      "PAYLOAD_TOO_LARGE",
    );
    expect(() => validateReferenceTurnBounds(0, 0, 1)).not.toThrow();
    expectCode(
      () => validateReferenceTurnBounds(REFERENCE_FILE_LIMITS.maxFilesPerTurn, 0, 1),
      "PAYLOAD_TOO_LARGE",
    );
    expectCode(
      () => validateReferenceTurnBounds(0, REFERENCE_FILE_LIMITS.maxTurnBytes, 1),
      "PAYLOAD_TOO_LARGE",
    );
  });

  it("refuses an oversized or over-counted file before the transport is called", async () => {
    const { transport, staged } = fakeTransport();
    await expectCodeAsync(
      () =>
        stageReferenceChatFile(
          deps(transport),
          target(),
          oversizedSource(REFERENCE_FILE_LIMITS.maxFileBytes + 1),
          signal,
        ),
      "PAYLOAD_TOO_LARGE",
    );
    await expectCodeAsync(
      () =>
        stageReferenceChatFile(
          deps(transport),
          target(),
          source("shot.png", "image/png", [1, 2, 3]),
          signal,
          { fileCount: REFERENCE_FILE_LIMITS.maxFilesPerTurn, turnBytes: 0 },
        ),
      "PAYLOAD_TOO_LARGE",
    );
    expect(staged).toHaveLength(0);
  });
});

describe("reference file staging", () => {
  it("stages through the transport and returns the mention the chat refers to", async () => {
    const { transport, staged } = fakeTransport();
    const receipt = await stageReferenceChatFile(
      deps(transport),
      target(),
      source("shot.png", "image/png", [1, 2, 3]),
      signal,
    );

    expect(receipt.displayName).toBe("shot.png");
    expect(receipt.mentionText).toBe("@shot.png ");
    expect(receipt.receipt).toEqual({
      hostId: "host-a",
      attachmentId: "att-1",
      sha256: FIXED_SHA,
      sizeBytes: 3,
      mediaType: "image/png",
    });
    expect(staged).toHaveLength(1);
    expect(staged[0].sizeBytes).toBe(3);
    expect(staged[0].contentBase64).toBe(referenceBase64(new Uint8Array([1, 2, 3])));
  });

  it("refuses a traversing name before anything is staged", async () => {
    const { transport, staged } = fakeTransport();
    await expectCodeAsync(
      () =>
        stageReferenceChatFile(
          deps(transport),
          target(),
          source("../evil.png", "image/png", [1]),
          signal,
        ),
      "INVALID_REQUEST",
    );
    expect(staged).toHaveLength(0);
  });

  it("refuses a file whose read byte count does not match its reported size", async () => {
    const { transport, staged } = fakeTransport();
    const lying: ReferenceFileSource = {
      name: "shot.png",
      type: "image/png",
      size: 9,
      arrayBuffer: async () => new Uint8Array([1, 2, 3]).slice().buffer,
    };
    await expectCodeAsync(
      () => stageReferenceChatFile(deps(transport), target(), lying, signal),
      "INVALID_REQUEST",
    );
    expect(staged).toHaveLength(0);
  });
});

describe("reference file cleanup", () => {
  it("cancels and deletes explicitly, and reports an unacknowledged cleanup", async () => {
    const { transport, cancelled, removed } = fakeTransport();
    const receipt = await stageReferenceChatFile(
      deps(transport),
      target(),
      source("shot.png", "image/png", [1, 2, 3]),
      signal,
    );

    expect(await cancelReferenceChatFile(deps(transport), target(), receipt)).toEqual({
      attachmentId: "att-1",
      cleaned: true,
    });
    expect(cancelled).toEqual(["att-1"]);

    expect(await deleteReferenceChatFile(deps(transport), target(), receipt)).toEqual({
      attachmentId: "att-1",
      cleaned: true,
    });
    expect(removed).toEqual(["att-1"]);

    const unacknowledged = fakeTransport({ cancel: async () => false });
    expect(
      (await cancelReferenceChatFile(deps(unacknowledged.transport), target(), receipt)).cleaned,
    ).toBe(false);
    expect(unacknowledged.removed).toHaveLength(0);
  });
});

describe("reference file send", () => {
  it("sends the mention as editable text and never as a managed attachment", async () => {
    const { transport } = fakeTransport();
    const receipt = await stageReferenceChatFile(
      deps(transport),
      target(),
      source("shot.png", "image/png", [1, 2, 3]),
      signal,
    );

    const plan = planReferenceFileSend("look at this", [receipt]);
    expect(plan.text).toBe("look at this @shot.png ");
    expect(plan.attachmentIds).toEqual([]);
    expect(plan.origin).toBe("chat");

    const terminalPlan = planReferenceFileSend("", [receipt], "terminal");
    expect(terminalPlan.text).toBe("@shot.png ");
    expect(terminalPlan.origin).toBe("terminal");
  });

  it("keeps the draft and the staged file when a send fails", async () => {
    const { transport, staged, removed } = fakeTransport();
    const receipt = await stageReferenceChatFile(
      deps(transport),
      target(),
      source("shot.png", "image/png", [1, 2, 3]),
      signal,
    );

    const failure = referenceFileSendFailure(
      "still typing",
      [receipt],
      new ReferenceFileError("TIMEOUT", "the pane did not acknowledge the submit"),
    );

    expect(failure.text).toBe("still typing");
    expect(failure.deleted).toBe(false);
    expect(failure.attachmentIds).toEqual([]);
    expect(failure.receipts).toEqual([receipt]);
    expect(removed).toHaveLength(0);
    expect(staged).toHaveLength(1);
  });

  it("refuses to send a receipt whose mention is not a safe relative path", () => {
    const hostile = {
      receipt: {
        hostId: "host-a",
        attachmentId: "att-1",
        sha256: FIXED_SHA,
        sizeBytes: 3,
        mediaType: "image/png" as const,
      },
      displayName: "shot.png",
      mentionText: "@/etc/passwd ",
    };
    expectCode(() => planReferenceFileSend("hello", [hostile]), "INVALID_REQUEST");
  });
});

describe("reference file mention insertion", () => {
  it("appends a mention, adding the separating space only when one is needed", () => {
    expect(referenceAppendMention("", "shot.png")).toBe("@shot.png ");
    expect(referenceAppendMention("hello", "shot.png")).toBe("hello @shot.png ");
    expect(referenceAppendMention("hello ", "shot.png")).toBe("hello @shot.png ");
    expect(
      referenceAppendMention("hello" + String.fromCharCode(10), "shot.png"),
    ).toBe("hello" + String.fromCharCode(10) + "@shot.png ");
  });

  it("inserts a mention at the caret, leaving the caret after it", () => {
    expect(referenceMentionInsertion("", "shot.png", 0)).toEqual({
      text: "@shot.png ",
      caret: 10,
    });
    expect(referenceMentionInsertion("look here", "shot.png", 9)).toEqual({
      text: "look here @shot.png ",
      caret: 20,
    });
    expect(referenceMentionInsertion("look ", "shot.png", 5)).toEqual({
      text: "look @shot.png ",
      caret: 15,
    });
    expect(referenceMentionInsertion("a b", "shot.png", 1)).toEqual({
      text: "a @shot.png  b",
      caret: 12,
    });
    expect(referenceMentionInsertion("abc", "shot.png", 99)).toEqual({
      text: "abc @shot.png ",
      caret: 14,
    });
  });
});

describe("reference file routes", () => {
  it("builds the frozen file routes and the target-bound mutation envelope", () => {
    expect(referenceFilesRoute("sess-1")).toBe("/api/v1/reference-chat/sess-1/files");
    expect(referenceFilePreviewRoute("sess-1", "att-9")).toBe(
      "/api/v1/reference-chat/sess-1/files/att-9",
    );
    expect(referenceFileMutationEnvelope("req-1", target(), { attachmentId: "att-9" })).toEqual({
      requestId: "req-1",
      target: base,
      params: { attachmentId: "att-9" },
    });
  });
});
