import { describe, expect, it, vi } from "vitest";
import type { TargetRef } from "../../lib/scopedContracts";
import { createRemoteManagedChatService } from "./remoteManagedChatService";

const validTarget: TargetRef = {
  hostId: "machine-alpha",
  ownerId: "device-owner-1",
  epoch: "42",
  backendSessionId: "session-xyz",
};

const baseUrl = "https://relay.test.lan:8899";

/**
 * Mimics the native `Window.fetch` brand check: the real method throws
 * `Failed to execute 'fetch' on 'Window': Illegal invocation` whenever it is
 * invoked with a receiver other than the global object. A plain function stub
 * cannot observe that defect — only a receiver-sensitive one can.
 */
function makeReceiverSensitiveFetch() {
  const calls: { input: unknown; init?: unknown }[] = [];
  const impl = function (this: unknown, input: unknown, init?: unknown): Promise<unknown> {
    if (this !== globalThis) {
      throw new TypeError("Failed to execute 'fetch' on 'Window': Illegal invocation");
    }
    calls.push({ input, init });
    return Promise.resolve({
      ok: true,
      status: 200,
      json: async () => ({
        ok: true,
        data: { target: validTarget, provider: "codex", threadId: "thread-default-receiver" },
      }),
    });
  };
  return { impl, calls };
}

describe("RemoteManagedChatService default fetch receiver", () => {
  it("calls the default global fetch with a legal receiver, never the service instance", async () => {
    const globalFetch = makeReceiverSensitiveFetch();
    vi.stubGlobal("fetch", globalFetch.impl);
    try {
      const service = createRemoteManagedChatService({
        baseUrl,
        token: () => "mock-token",
      });

      await expect(service.start(validTarget)).resolves.toMatchObject({
        threadId: "thread-default-receiver",
      });
      expect(globalFetch.calls).toHaveLength(1);
      expect(globalFetch.calls[0].input).toBe(`${baseUrl}/api/v1/chat/start`);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("passes a caller-supplied fetchFn through unchanged and never touches the global fetch", async () => {
    const globalFetch = makeReceiverSensitiveFetch();
    vi.stubGlobal("fetch", globalFetch.impl);
    try {
      const fetchMock = vi.fn().mockResolvedValue({
        ok: true,
        status: 200,
        json: async () => ({
          ok: true,
          data: { target: validTarget, provider: "codex", threadId: "thread-injected" },
        }),
      });

      const service = createRemoteManagedChatService({
        baseUrl,
        token: () => "mock-token",
        fetchFn: fetchMock,
      });

      expect((service as unknown as { fetchImpl: typeof fetch }).fetchImpl).toBe(fetchMock);

      await expect(service.start(validTarget)).resolves.toMatchObject({
        threadId: "thread-injected",
      });
      expect(fetchMock).toHaveBeenCalledTimes(1);
      expect(fetchMock.mock.calls[0][0]).toBe(`${baseUrl}/api/v1/chat/start`);
      expect(globalFetch.calls).toHaveLength(0);
    } finally {
      vi.unstubAllGlobals();
    }
  });
});
