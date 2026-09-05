import { describe, expect, expectTypeOf, it } from "vitest";
import type { TargetRef, MutationEnvelope, ScopeResult, EventReplay, RunTarget,
  ScopeCapability, ConversationClaimKey, ConversationOwner, ChatDraft, TransitionSource } from "./scopedContracts";
import { ATTACHMENT_MAX_FILE_BYTES, ATTACHMENT_MAX_FILES_PER_TURN,
  ATTACHMENT_MAX_TURN_BYTES, ATTACHMENT_UNREFERENCED_TTL_MS } from "./scopedContracts";

describe("scoped contract wire", () => {
  it("keeps producer DTOs compatible with machine JSON", () => {
    // Given declarations checked at compile time, no service implementations.
    const mutation: MutationEnvelope<string> = { requestId: "create-1", params: "fixture" };
    const result: ScopeResult<string> = { ok: false, requestId: "r1", error: {
      code: "TARGET_EXPIRED", message: "fixture", retryable: false, details: null } };
    const replay: EventReplay<string, string> = { kind: "gap", afterSequence: 12,
      snapshot: { revision: 4, items: [], completeness: "partial", unavailableHosts: ["host-b"] } };
    const run: RunTarget = { kind: "ssh", hostId: "host-b" };
    const capabilities: readonly ScopeCapability[] = ["scopeControlV1", "sshHelperV1", "managedCodexV1", "captureV1"];
    const claim: ConversationClaimKey = { hostId: "h", provider: "codex", conversationId: "provider-id" };
    const owner: ConversationOwner = { kind: "provisional", requestId: "thread-start-1" };
    const draft: ChatDraft = { text: "unsent fixture", attachments: [{ hostId: "h",
      attachmentId: "opaque-1", sha256: "fixture-hash", sizeBytes: 17, mediaType: "image/png" }] };
    const source: TransitionSource = { kind: "provider", provider: "codex", requestId: "callback-1" };
    // When crossing JSON, then each declaration preserves the supplied fields.
    for (const dto of [mutation, result, replay, run, capabilities, claim, owner, draft, source]) {
      const wire: unknown = JSON.parse(JSON.stringify(dto));
      expect(wire).toEqual(dto);
    }
    expect([ATTACHMENT_MAX_FILE_BYTES, ATTACHMENT_MAX_FILES_PER_TURN,
      ATTACHMENT_MAX_TURN_BYTES, ATTACHMENT_UNREFERENCED_TTL_MS]).toEqual([10_485_760, 4, 20_971_520, 86_400_000]);
  });
  it("preserves all routing fields and full u64 epoch through JSON", () => {
    // Given a typed identity beyond JS integer precision.
    const target: TargetRef = { hostId: "host-a", ownerId: "owner-a",
      epoch: "18446744073709551615", backendSessionId: "pty-1" };
    // When the DTO crosses JSON (no transport parsing is implemented here).
    const wire: unknown = JSON.parse(JSON.stringify(target));
    // Then the machine fields and string representation remain exact.
    expect(wire).toEqual({ hostId: "host-a", ownerId: "owner-a",
      epoch: "18446744073709551615", backendSessionId: "pty-1" });
    expectTypeOf<TargetRef["epoch"]>().toEqualTypeOf<string>();
  });
});
