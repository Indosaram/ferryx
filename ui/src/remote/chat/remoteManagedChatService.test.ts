import { describe, expect, it, vi } from "vitest";
import type { ChatDraft, TargetRef } from "../../lib/scopedContracts";
import type { Callback } from "../../features/ferryx/chat/ManagedChat";
import {
  createRemoteManagedChatService,
  parseLiveCallback,
  validateTargetRef,
  ManagedChatRemoteError,
  StaleManagedChatCallbackError,
} from "./remoteManagedChatService";

const validTarget: TargetRef = {
  hostId: "machine-alpha",
  ownerId: "device-owner-1",
  epoch: "42",
  backendSessionId: "session-xyz",
};

describe("TargetRef validation", () => {
  it("freezes and accepts a valid canonical TargetRef", () => {
    const validated = validateTargetRef(validTarget);
    expect(validated).toEqual(validTarget);
    expect(Object.isFrozen(validated)).toBe(true);
  });

  it("rejects non-decimal or invalid epoch formats", () => {
    expect(() => validateTargetRef({ ...validTarget, epoch: "01" })).toThrowError(
      ManagedChatRemoteError
    );
    expect(() => validateTargetRef({ ...validTarget, epoch: "abc" })).toThrowError(
      ManagedChatRemoteError
    );
    expect(() => validateTargetRef({ ...validTarget, epoch: "-1" })).toThrowError(
      ManagedChatRemoteError
    );
  });

  it("rejects empty hostId, ownerId, or backendSessionId", () => {
    expect(() => validateTargetRef({ ...validTarget, hostId: "" })).toThrowError(
      ManagedChatRemoteError
    );
    expect(() => validateTargetRef({ ...validTarget, ownerId: "   " })).toThrowError(
      ManagedChatRemoteError
    );
    expect(() =>
      validateTargetRef({ ...validTarget, backendSessionId: "" })
    ).toThrowError(ManagedChatRemoteError);
  });
});

describe("RemoteManagedChatService.send with canonical ScopeResult", () => {
  it("delivers chat draft and receives validated DeliveryReceipt from canonical ScopeResult", async () => {
    const draft: ChatDraft = {
      text: "Investigate test failures",
      attachments: [],
    };
    const requestId = "req-fixed-uuid-101";

    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      json: async () => ({
        ok: true,
        data: {
          requestId,
          target: validTarget,
          stage: "accepted",
        },
        requestId,
      }),
    });

    const service = createRemoteManagedChatService({
      baseUrl: "https://relay.test.lan:8899",
      token: () => "mock-token-xyz",
      fetchFn: fetchMock,
    });

    const receipt = await service.send(validTarget, draft, requestId);

    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [calledUrl, calledInit] = fetchMock.mock.calls[0];
    expect(calledUrl).toBe("https://relay.test.lan:8899/api/v1/chat/send");
    expect(calledInit.method).toBe("POST");
    expect(calledInit.headers).toEqual({
      Authorization: "Bearer mock-token-xyz",
      "Content-Type": "application/json",
    });

    const sentPayload = JSON.parse(calledInit.body);
    expect(sentPayload).toEqual({
      requestId,
      target: validTarget,
      draft,
    });

    expect(receipt.stage).toBe("accepted");
    expect(receipt.requestId).toBe(requestId);
    expect(receipt.target).toEqual(validTarget);
    expect(Object.isFrozen(receipt)).toBe(true);
  });

  it("rejects delivery receipts when the request ID does not match", async () => {
    const draft: ChatDraft = { text: "hello", attachments: [] };
    const requestId = "req-client-id";

    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      json: async () => ({
        ok: true,
        data: {
          requestId: "req-spoofed-other-id",
          target: validTarget,
          stage: "accepted",
        },
        requestId: "req-spoofed-other-id",
      }),
    });

    const service = createRemoteManagedChatService({
      baseUrl: "https://relay.test.lan:8899",
      token: () => "mock-token",
      fetchFn: fetchMock,
    });

    await expect(service.send(validTarget, draft, requestId)).rejects.toThrowError(
      /Receipt requestId mismatch/
    );
  });

  it("rejects delivery receipts when the target identity does not match", async () => {
    const draft: ChatDraft = { text: "hello", attachments: [] };
    const requestId = "req-client-id";
    const mismatchedTargets: TargetRef[] = [
      { ...validTarget, epoch: "999" },
      { ...validTarget, hostId: "machine-beta" },
      { ...validTarget, ownerId: "device-owner-2" },
      { ...validTarget, backendSessionId: "session-other" },
    ];

    for (const receiptTarget of mismatchedTargets) {
      const fetchMock = vi.fn().mockResolvedValue({
        ok: true,
        status: 200,
        json: async () => ({
          ok: true,
          data: {
            requestId,
            target: receiptTarget,
            stage: "accepted",
          },
          requestId,
        }),
      });

      const service = createRemoteManagedChatService({
        baseUrl: "https://relay.test.lan:8899",
        token: () => "mock-token",
        fetchFn: fetchMock,
      });

      await expect(service.send(validTarget, draft, requestId)).rejects.toThrowError(
        /Receipt target does not match request target/
      );
    }
  });

  it("surfaces typed backend error codes from canonical ScopeResult.error", async () => {
    const draft: ChatDraft = { text: "hello", attachments: [] };

    const fetchMock = vi.fn().mockResolvedValue({
      ok: false,
      status: 403,
      json: async () => ({
        ok: false,
        error: {
          code: "FORBIDDEN",
          message: "Target owner_id does not match authenticated device",
          retryable: false,
        },
        requestId: "req-1",
      }),
    });

    const service = createRemoteManagedChatService({
      baseUrl: "https://relay.test.lan:8899",
      token: () => "mock-token",
      fetchFn: fetchMock,
    });

    await expect(
      service.send(validTarget, draft, "req-1")
    ).rejects.toThrowError(
      /Target owner_id does not match authenticated device/
    );
  });
});

describe("RemoteManagedChatService.reply with canonical ScopeResult", () => {
  it("submits callback incarnation as callbackIncarnation in the reply body", async () => {
    const callback: Callback & { callbackIncarnation: number; target: TargetRef } = {
      id: "cb-approval-turn-4",
      threadId: "th-agent-main",
      turnId: "tu-step-12",
      callbackIncarnation: 7,
      target: validTarget,
      kind: "approval",
      text: "Apply git stash before rebase?",
    };

    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      json: async () => ({
        ok: true,
        data: { resolved: true },
        requestId: "req-cb-reply",
      }),
    });

    const service = createRemoteManagedChatService({
      baseUrl: "https://relay.test.lan:8899",
      token: () => "mock-token",
      fetchFn: fetchMock,
    });

    await service.reply(validTarget, callback, { decision: "accept" });

    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [calledUrl, calledInit] = fetchMock.mock.calls[0];
    expect(calledUrl).toBe("https://relay.test.lan:8899/api/v1/chat/reply");
    const body = JSON.parse(calledInit.body);
    expect(body.callbackId).toBe("cb-approval-turn-4");
    expect(body.threadId).toBe("th-agent-main");
    expect(body.turnId).toBe("tu-step-12");
    expect(body.kind).toBe("approval");
    expect(body.result).toEqual({ decision: "accept" });
    expect(body.target).toEqual(validTarget);
    expect(body.callbackIncarnation).toBe(7);
    expect(body).not.toHaveProperty("callback_incarnation");
  });

  it("maps STALE_CALLBACK rejection to a distinct typed error", async () => {
    const callback: Callback & { callbackIncarnation: number } = {
      id: "cb-stale",
      threadId: "th-agent-main",
      turnId: "tu-step-12",
      callbackIncarnation: 7,
      kind: "approval",
      text: "Approve?",
    };
    const fetchMock = vi.fn().mockResolvedValue({
      ok: false,
      status: 409,
      json: async () => ({
        ok: false,
        error: {
          code: "STALE_CALLBACK",
          message: "Callback incarnation is stale",
          retryable: false,
        },
        requestId: "req-stale",
      }),
    });
    const service = createRemoteManagedChatService({
      baseUrl: "https://relay.test.lan:8899",
      token: () => "mock-token",
      fetchFn: fetchMock,
    });

    await expect(service.reply(validTarget, callback, { decision: "accept" }))
      .rejects.toBeInstanceOf(StaleManagedChatCallbackError);
  });

  it("keeps plain TARGET_EXPIRED rejection on the generic error path", async () => {
    const callback: Callback & { callbackIncarnation: number } = {
      id: "cb-expired-target",
      threadId: "th-agent-main",
      turnId: "tu-step-12",
      callbackIncarnation: 7,
      kind: "approval",
      text: "Approve?",
    };
    const fetchMock = vi.fn().mockResolvedValue({
      ok: false,
      status: 409,
      json: async () => ({
        ok: false,
        error: {
          code: "TARGET_EXPIRED",
          message: "Target epoch has expired",
          retryable: false,
        },
        requestId: "req-target-expired",
      }),
    });
    const service = createRemoteManagedChatService({
      baseUrl: "https://relay.test.lan:8899",
      token: () => "mock-token",
      fetchFn: fetchMock,
    });

    await expect(service.reply(validTarget, callback, { decision: "accept" }))
      .rejects.toMatchObject({
        name: "ManagedChatRemoteError",
        code: "TARGET_EXPIRED",
        status: 409,
      });
  });

  it("keeps legacy TARGET_EXPIRED incarnation rejection as a stale callback error", async () => {
    const callback: Callback & { callbackIncarnation: number } = {
      id: "cb-legacy-stale",
      threadId: "th-agent-main",
      turnId: "tu-step-12",
      callbackIncarnation: 7,
      kind: "approval",
      text: "Approve?",
    };
    const fetchMock = vi.fn().mockResolvedValue({
      ok: false,
      status: 409,
      json: async () => ({
        ok: false,
        error: {
          code: "TARGET_EXPIRED",
          message: "Callback 'cb-legacy-stale' incarnation is stale",
          retryable: false,
        },
        requestId: "req-legacy-stale",
      }),
    });
    const service = createRemoteManagedChatService({
      baseUrl: "https://relay.test.lan:8899",
      token: () => "mock-token",
      fetchFn: fetchMock,
    });

    await expect(service.reply(validTarget, callback, { decision: "accept" }))
      .rejects.toBeInstanceOf(StaleManagedChatCallbackError);
  });
});

describe("RemoteManagedChatService.stop with canonical ScopeResult", () => {
  it("submits stop execution request bound to target identity and parses ScopeResult", async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      json: async () => ({
        ok: true,
        data: { stopped: true },
        requestId: "req-stop",
      }),
    });

    const service = createRemoteManagedChatService({
      baseUrl: "https://relay.test.lan:8899",
      token: () => "mock-token",
      fetchFn: fetchMock,
    });

    await service.stop(validTarget);

    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [calledUrl, calledInit] = fetchMock.mock.calls[0];
    expect(calledUrl).toBe("https://relay.test.lan:8899/api/v1/chat/stop");
    const body = JSON.parse(calledInit.body);
    expect(body.target).toEqual(validTarget);
  });
});

describe("RemoteManagedChatService.start with canonical ScopeResult", () => {
  it("delivers explicit start request and parses ScopeResult returning ChatStartResult", async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      json: async () => ({
        ok: true,
        data: {
          target: validTarget,
          provider: "codex",
          threadId: "th-codex-1234",
        },
        requestId: "req-start-id",
      }),
    });

    const service = createRemoteManagedChatService({
      baseUrl: "https://relay.test.lan:8899",
      token: () => "mock-token",
      fetchFn: fetchMock,
    });

    const result = await service.start(validTarget, "codex", "req-start-id");

    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [calledUrl, calledInit] = fetchMock.mock.calls[0];
    expect(calledUrl).toBe("https://relay.test.lan:8899/api/v1/chat/start");
    expect(calledInit.method).toBe("POST");
    const body = JSON.parse(calledInit.body);
    expect(body).toEqual({
      requestId: "req-start-id",
      target: validTarget,
      provider: "codex",
    });

    expect(result).toEqual({
      target: validTarget,
      provider: "codex",
      threadId: "th-codex-1234",
    });
    expect(Object.isFrozen(result)).toBe(true);
  });

  it("surfaces typed backend error codes on start rejection", async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: false,
      status: 422,
      json: async () => ({
        ok: false,
        error: {
          code: "Unsupported",
          message: "Only managed Codex is supported",
          retryable: false,
        },
        requestId: "req-start-fail",
      }),
    });

    const service = createRemoteManagedChatService({
      baseUrl: "https://relay.test.lan:8899",
      token: () => "mock-token",
      fetchFn: fetchMock,
    });

    await expect(
      service.start(validTarget, "codex", "req-start-fail")
    ).rejects.toThrowError(/Only managed Codex is supported/);
  });

  it("rejects when provider returns an empty or invalid threadId", async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      json: async () => ({
        ok: true,
        data: {
          target: validTarget,
          provider: "codex",
          threadId: "",
        },
        requestId: "req-start-bad-thread",
      }),
    });

    const service = createRemoteManagedChatService({
      baseUrl: "https://relay.test.lan:8899",
      token: () => "mock-token",
      fetchFn: fetchMock,
    });

    await expect(
      service.start(validTarget, "codex", "req-start-bad-thread")
    ).rejects.toThrowError(/Provider did not return a valid managed threadId/);
  });
});

describe("RemoteManagedChatService.fetchCallbacks", () => {
  it("discovers callback target and incarnation from the authoritative response", async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      json: async () => ({
        ok: true,
        data: [
          {
            callbackId: "cb-100",
            threadId: "th-agent",
            turnId: "tu-4",
            callbackIncarnation: 12,
            target: validTarget,
            kind: "question",
            text: "Which database target to migrate?",
            questions: [
              {
                id: "q-1",
                question: "Select database",
                isSecret: false,
                options: [
                  { label: "PostgreSQL", description: "Production primary" },
                  { label: "SQLite", description: "Embedded fallback" },
                ],
              },
            ],
          },
        ],
        requestId: "chat-callbacks-query",
      }),
    });

    const service = createRemoteManagedChatService({
      baseUrl: "https://relay.test.lan:8899",
      token: () => "mock-token",
      fetchFn: fetchMock,
    });

    const callbacks = await service.fetchCallbacks("session-xyz");

    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [calledUrl, calledInit] = fetchMock.mock.calls[0];
    expect(calledUrl).toBe(
      "https://relay.test.lan:8899/api/v1/chat/callbacks?backendSessionId=session-xyz"
    );
    expect(calledInit.method).toBe("GET");
    expect(calledInit.headers).toEqual({
      Authorization: "Bearer mock-token",
      "Content-Type": "application/json",
    });

    expect(callbacks).toHaveLength(1);
    expect(callbacks[0].id).toBe("cb-100");
    expect(callbacks[0].kind).toBe("question");
    expect(callbacks[0].target).toEqual(validTarget);
    expect(callbacks[0].callbackIncarnation).toBe(12);
    expect(callbacks[0].questions?.[0].options).toHaveLength(2);
  });

  it("continues parsing callback payloads without optional target or incarnation", async () => {
    const callback = parseLiveCallback({
      callbackId: "cb-legacy",
      threadId: "th-agent",
      turnId: "tu-legacy",
      kind: "approval",
    });

    expect(callback).toMatchObject({
      id: "cb-legacy",
      threadId: "th-agent",
      turnId: "tu-legacy",
      kind: "approval",
    });
    expect(callback.target).toBeUndefined();
    expect(callback.callbackIncarnation).toBeUndefined();
  });

  it("preserves question options, required metadata, and secret status", () => {
    const callback = parseLiveCallback({
      callbackId: "cb-question",
      threadId: "th-agent",
      turnId: "tu-question",
      callbackIncarnation: 13,
      target: validTarget,
      kind: "question",
      questions: [{
        id: "q-secret",
        question: "Enter deployment key",
        isSecret: true,
        required: true,
        options: [{ label: "Primary", description: "Production key" }],
      }],
    });

    expect(callback.questions?.[0]).toMatchObject({
      id: "q-secret",
      question: "Enter deployment key",
      isSecret: true,
      required: true,
      options: [{ label: "Primary", description: "Production key" }],
    });
  });
});

describe("RemoteManagedChatService.openResultPreview", () => {
  it("mints capability token and returns scoped URL without arbitrary host path leakage", async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      json: async () => ({
        ok: true,
        token: "tok-preview-uuid-789",
        expiresAt: Date.now() + 60000,
      }),
    });

    const service = createRemoteManagedChatService({
      baseUrl: "https://relay.test.lan:8899",
      token: () => "mock-token",
      fetchFn: fetchMock,
    });

    const previewUrl = await service.openResultPreview(validTarget, "server-issued-id");

    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [calledUrl, calledInit] = fetchMock.mock.calls[0];
    expect(calledUrl).toBe("https://relay.test.lan:8899/api/v1/files/preview/token");
    const body = JSON.parse(calledInit.body);
    expect(body).toEqual({ target: validTarget, fileId: "server-issued-id" });

    expect(previewUrl).toBe(
      "https://relay.test.lan:8899/api/v1/files/preview/tok-preview-uuid-789"
    );
  });

  it("fetches path-free result listing entries", async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      json: async () => ({ ok: true, files: [{ fileId: "file-1", displayName: "step_1.txt" }] }),
    });
    const service = createRemoteManagedChatService({ baseUrl: "https://relay.test.lan:8899", token: () => "mock-token", fetchFn: fetchMock });
    await expect(service.fetchResultFiles(validTarget)).resolves.toEqual([{ fileId: "file-1", displayName: "step_1.txt" }]);
    expect(JSON.parse(fetchMock.mock.calls[0][1].body)).toEqual({ target: validTarget });
  });
});
