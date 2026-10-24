import { afterEach, describe, expect, it, vi } from "vitest";
import { AccountSessionError, deleteAccount } from "./accountSession";

const origin = "https://relay.example.com";

function respond(status: number, body: unknown) {
  const calls: Array<{ url: string; init?: RequestInit }> = [];
  globalThis.fetch = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
    calls.push({ url: String(input), init });
    return Promise.resolve(new Response(JSON.stringify(body), { status }));
  });
  return calls;
}

async function failure(promise: Promise<unknown>): Promise<AccountSessionError> {
  const err = await promise.then(() => null, (e: unknown) => e);
  if (!(err instanceof AccountSessionError)) throw new Error("expected AccountSessionError");
  return err;
}

describe("deleteAccount", () => {
  const realFetch = globalThis.fetch;
  afterEach(() => {
    globalThis.fetch = realFetch;
  });

  it("posts the confirmation email with the account bearer", async () => {
    const calls = respond(200, { deleted: true });
    await deleteAccount(origin, "tok", "me@example.com");
    expect(calls[0]?.url).toBe(`${origin}/api/account/v1/account/delete`);
    expect(calls[0]?.init?.method).toBe("POST");
    expect(new Headers(calls[0]?.init?.headers).get("Authorization")).toBe("Bearer tok");
    expect(JSON.parse(String(calls[0]?.init?.body))).toEqual({ confirmEmail: "me@example.com" });
  });

  it("surfaces CONFIRMATION_MISMATCH from the server envelope", async () => {
    respond(400, { code: "CONFIRMATION_MISMATCH", message: "Email does not match" });
    const err = await failure(deleteAccount(origin, "tok", "x@example.com"));
    expect(err.code).toBe("CONFIRMATION_MISMATCH");
    expect(err.status).toBe(400);
  });

  it("surfaces SUBSCRIPTION_ACTIVE", async () => {
    respond(409, { code: "SUBSCRIPTION_ACTIVE", message: "Cancel first" });
    expect((await failure(deleteAccount(origin, "tok", "me@example.com"))).code).toBe("SUBSCRIPTION_ACTIVE");
  });

  it("maps a 401 to UNAUTHORIZED", async () => {
    respond(401, { code: "UNAUTHORIZED", message: "no" });
    const err = await failure(deleteAccount(origin, "tok", "me@example.com"));
    expect(err.code).toBe("UNAUTHORIZED");
    expect(err.status).toBe(401);
  });

  it("falls back to DELETE_ACCOUNT_FAILED without an envelope", async () => {
    respond(500, "boom");
    expect((await failure(deleteAccount(origin, "tok", "me@example.com"))).code).toBe("DELETE_ACCOUNT_FAILED");
  });

  it("rejects a 200 that does not confirm deletion", async () => {
    respond(200, { deleted: false });
    expect((await failure(deleteAccount(origin, "tok", "me@example.com"))).code).toBe("INVALID_RESPONSE");
  });
});
