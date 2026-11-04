/**
 * Client mapping for the reference payload route. Not executed on this machine.
 * Deferred: bun run --cwd ui test -- src/remote/chat/referencePayload.test.ts
 */
import { describe, expect, it, vi } from "vitest";
import type { ReferenceTargetRef } from "./referenceTypes";
import { loadReferencePayload, type ReferencePayloadDeps } from "./referencePayload";

const target: ReferenceTargetRef = {
  target: {
    hostId: "host-a",
    ownerId: "owner-a",
    epoch: "3",
    backendSessionId: "sess-1",
  },
  providerSessionId: "provider-1",
};

function jsonResponse(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json" },
  });
}

function deps(fetchImpl: ReferencePayloadDeps["fetchImpl"], signal?: AbortSignal): ReferencePayloadDeps {
  return {
    baseUrl: "http://gateway.example",
    token: "token-1",
    registryId: "codex",
    signal,
    fetchImpl,
  };
}

describe("loadReferencePayload", () => {
  it("maps a loaded body, a truncated body, expiry, and a missing part", async () => {
    const fetchImpl = vi.fn(async (path: string) => {
      const ref = new URLSearchParams(path.split("?")[1]).get("ref");
      if (ref === "loaded") {
        return jsonResponse(200, {
          status: "loaded",
          kind: "diff",
          body: "*** Begin Patch",
          byteLength: 16,
          digest: "a".repeat(64),
          cap: 2097152,
          truncated: false,
        });
      }
      if (ref === "cut") {
        return jsonResponse(200, {
          status: "truncated",
          kind: "output",
          body: "zz",
          byteLength: 9,
          digest: "b".repeat(64),
          cap: 2,
          truncated: true,
        });
      }
      if (ref === "stale") return jsonResponse(410, { error: { code: "TARGET_EXPIRED" } });
      return jsonResponse(404, { error: { code: "NOT_FOUND" } });
    });
    await expect(loadReferencePayload("sess-1", target, "loaded", deps(fetchImpl))).resolves.toMatchObject({
      status: "loaded",
      body: "*** Begin Patch",
      truncated: false,
    });
    await expect(loadReferencePayload("sess-1", target, "cut", deps(fetchImpl))).resolves.toMatchObject({
      status: "truncated",
      truncated: true,
      cap: 2,
    });
    await expect(loadReferencePayload("sess-1", target, "stale", deps(fetchImpl))).resolves.toEqual({
      status: "expired",
    });
    await expect(loadReferencePayload("sess-1", target, "missing", deps(fetchImpl))).resolves.toEqual({
      status: "notFound",
    });
    expect(fetchImpl).toHaveBeenCalledTimes(4);
  });

  it("does not retry an unknown result and refuses a path ref without a request", async () => {
    const fetchImpl = vi.fn(async () => jsonResponse(500, { error: { code: "REQUEST_FAILED" } }));
    await expect(loadReferencePayload("sess-1", target, "fp1.abc", deps(fetchImpl))).resolves.toEqual({
      status: "unavailable",
      code: "REQUEST_FAILED",
    });
    expect(fetchImpl).toHaveBeenCalledTimes(1);
    await expect(loadReferencePayload("sess-1", target, "/etc/passwd", deps(fetchImpl))).resolves.toEqual({
      status: "unavailable",
      code: "INVALID_REQUEST",
    });
    expect(fetchImpl).toHaveBeenCalledTimes(1);
  });

  it("aborts on target switch and does not map that rejection to a result", async () => {
    const fetchImpl = vi.fn(async (_path: string, init: RequestInit) => {
      if (init.signal?.aborted) throw new DOMException("The operation was aborted", "AbortError");
      return jsonResponse(200, { status: "loaded" });
    });
    const controller = new AbortController();
    controller.abort();
    await expect(loadReferencePayload("sess-1", target, "fp1.abc", deps(fetchImpl, controller.signal))).rejects.toMatchObject({
      name: "AbortError",
    });
    expect(fetchImpl).not.toHaveBeenCalled();

    const later = new AbortController();
    const pending = loadReferencePayload("sess-1", target, "fp1.abc", deps(async (_path, init) => {
      later.abort();
      init.signal?.throwIfAborted();
      return jsonResponse(200, { status: "loaded", kind: "output", body: "x", byteLength: 1, digest: "c", cap: 1, truncated: false });
    }, later.signal));
    await expect(pending).rejects.toThrow();
  });
});
