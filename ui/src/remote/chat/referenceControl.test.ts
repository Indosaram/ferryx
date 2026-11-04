/**
 * Control apply client (plan T09). One POST, then applied, rejected, or unknown.
 * Unknown is not retried by this function. agent-picker does not POST.
 */
import { describe, expect, it, vi } from "vitest";
import {
  applyReferenceControl,
  classifyReferenceControlResponse,
  referenceControlPath,
  type ReferenceControlRequestBody,
} from "./referenceControl";
import { REFERENCE_OUTCOME_UNKNOWN_CODE } from "./referenceTypes";

const body: ReferenceControlRequestBody = {
  requestId: "req-1",
  target: { hostId: "local", ownerId: "owner-1", epoch: "7", backendSessionId: "sess-1" },
  registryId: "claude",
  controlId: "model",
  valueId: "opus",
};

function poster(status: number, payload: unknown, calls: string[]) {
  return vi.fn(async (path: string, init: { body: string }) => {
    calls.push(`${path} ${init.body}`);
    return { status, json: async () => payload };
  });
}

describe("referenceControl", () => {
  it("posts the control body once and reports applied", async () => {
    const calls: string[] = [];
    const post = poster(200, { ok: true, data: { outcome: "accepted" } }, calls);
    const result = await applyReferenceControl({
      sessionId: "sess-1",
      body,
      apply: "command",
      post,
    });
    expect(result).toEqual({ status: "applied", requestId: "req-1" });
    expect(post).toHaveBeenCalledTimes(1);
    expect(calls[0]).toContain(referenceControlPath("sess-1"));
    expect(JSON.parse(calls[0].slice(calls[0].indexOf("{")))).toEqual(body);
  });

  it("treats 404 and unsupported as rejected and does not call post again", async () => {
    const calls: string[] = [];
    const post = poster(404, { ok: false, error: { code: "NOT_FOUND", message: "no route" } }, calls);
    const rejected = await applyReferenceControl({ sessionId: "sess-1", body, apply: "command", post });
    expect(rejected.status).toBe("rejected");
    expect(rejected.code).toBe("NOT_FOUND");
    expect(classifyReferenceControlResponse(422, { error: { code: "UNSUPPORTED", message: "no" } }, "req-1").status).toBe("rejected");
    expect(post).toHaveBeenCalledTimes(1);
  });

  it("treats a timeout and a thrown post as unknown without a second post", async () => {
    const calls: string[] = [];
    const post = poster(504, { ok: false, error: { code: "TIMEOUT", message: "timed out" } }, calls);
    const timedOut = await applyReferenceControl({ sessionId: "sess-1", body, apply: "command", post });
    expect(timedOut).toMatchObject({ status: "unknown", code: "TIMEOUT" });
    const thrown = await applyReferenceControl({
      sessionId: "sess-1",
      body,
      apply: "command",
      post: async () => {
        throw new Error("network down");
      },
    });
    expect(thrown).toMatchObject({ status: "unknown", code: REFERENCE_OUTCOME_UNKNOWN_CODE });
    expect(post).toHaveBeenCalledTimes(1);
    expect(classifyReferenceControlResponse(200, { ok: true, data: {} }, "req-1").status).toBe("unknown");
  });

  it("does not post for an agent picker or a start-only option", async () => {
    const post = vi.fn();
    await expect(applyReferenceControl({ sessionId: "sess-1", body, apply: "agent-picker", post })).resolves.toEqual({
      status: "picker",
      requestId: "req-1",
    });
    await expect(applyReferenceControl({ sessionId: "sess-1", body, apply: "start-only", post })).resolves.toMatchObject({
      status: "rejected",
      code: "START_ONLY",
    });
    expect(post).not.toHaveBeenCalled();
  });
});
