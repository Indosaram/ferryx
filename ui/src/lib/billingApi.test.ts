import { afterEach, describe, expect, it, vi } from "vitest";

import {
  BillingApiError,
  fetchBillingEntitlement,
  inviteOrgMember,
  listOrgMembers,
  planLimitDetails,
  removeOrgMember,
  startBillingCheckout,
  suspensionDetails,
  toBillingApiError,
  updateBillingQuantity,
} from "./billingApi";

const ORIGIN = "https://account.example.test";
const TOKEN = "session-token-1";

interface RecordedRequest {
  readonly url: string;
  readonly method: string;
  readonly authorization: string | null;
  readonly contentType: string | null;
  readonly body: string | null;
  readonly redirect: RequestRedirect | null;
  readonly hasSignal: boolean;
}

function jsonResponse(payload: unknown, status = 200): Response {
  if (status === 204 || status === 205 || status === 304) {
    return new Response(null, { status });
  }
  return new Response(JSON.stringify(payload), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

function requestUrl(input: RequestInfo | URL): string {
  if (typeof input === "string") return input;
  if (input instanceof URL) return input.toString();
  return input.url;
}

function stubFetch(
  requests: RecordedRequest[],
  response: Response | ((url: string, init?: RequestInit) => Response | Promise<Response>),
): void {
  vi.stubGlobal("fetch", async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = requestUrl(input);
    const headers = new Headers(init?.headers);
    requests.push({
      url,
      method: init?.method ?? "GET",
      authorization: headers.get("Authorization"),
      contentType: headers.get("Content-Type"),
      body: typeof init?.body === "string" ? init.body : null,
      redirect: init?.redirect ?? null,
      hasSignal: typeof init?.signal?.aborted === "boolean",
    });
    if (typeof response === "function") {
      return await response(url, init);
    }
    return response.clone();
  });
}

async function captureBillingError(run: () => Promise<unknown>): Promise<BillingApiError> {
  try {
    await run();
  } catch (error) {
    if (error instanceof BillingApiError) return error;
    throw error;
  }
  throw new Error("expected the billing call to fail");
}

const CALL = { accountOrigin: ORIGIN, accountSessionToken: TOKEN };

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("billingApi entitlement", () => {
  it("sends the account session bearer to the canonical entitlement route", async () => {
    const requests: RecordedRequest[] = [];
    stubFetch(
      requests,
      jsonResponse({
        plan: "pro_monthly",
        status: "ok",
        machineLimit: 15,
        machinesUsed: 2,
        seats: null,
        hostPacks: 1,
        graceEndsAt: null,
        orgId: null,
        role: null,
        manageUrl: "https://store.example.test/subscriptions/42",
      }),
    );

    const entitlement = await fetchBillingEntitlement(CALL);

    expect(requests).toHaveLength(1);
    expect(requests[0].url).toBe(`${ORIGIN}/api/account/v1/billing/entitlement`);
    expect(requests[0].method).toBe("GET");
    expect(requests[0].authorization).toBe(`Bearer ${TOKEN}`);
    expect(requests[0].body).toBeNull();
    expect(entitlement).toEqual({
      plan: "pro_monthly",
      status: "ok",
      machineLimit: 15,
      machinesUsed: 2,
      seats: null,
      hostPacks: 1,
      graceEndsAt: null,
      orgId: null,
      role: null,
      manageUrl: "https://store.example.test/subscriptions/42",
    });
  });

  it("parses team fields including seats, org role and grace deadline", async () => {
    stubFetch(
      [],
      jsonResponse({
        plan: "team_annual",
        status: "past_due",
        machineLimit: 25,
        machinesUsed: 27,
        seats: 2,
        hostPacks: 1,
        graceEndsAt: 1_800_259_140,
        orgId: "org-1",
        role: "admin",
        manageUrl: null,
      }),
    );

    const entitlement = await fetchBillingEntitlement(CALL);

    expect(entitlement.plan).toBe("team_annual");
    expect(entitlement.status).toBe("past_due");
    expect(entitlement.seats).toBe(2);
    expect(entitlement.role).toBe("admin");
    expect(entitlement.orgId).toBe("org-1");
    expect(entitlement.graceEndsAt).toBe(1_800_259_140);
  });

  it("rejects an entitlement payload that is missing contract fields", async () => {
    stubFetch([], jsonResponse({ plan: "pro_monthly", status: "ok" }));

    const error = await captureBillingError(() => fetchBillingEntitlement(CALL));

    expect(error.code).toBe("INVALID_RESPONSE");
  });

  it("keeps the server code and status on a 401", async () => {
    stubFetch([], jsonResponse({ code: "UNAUTHORIZED", message: "session expired" }, 401));

    const error = await captureBillingError(() => fetchBillingEntitlement(CALL));

    expect(error.code).toBe("UNAUTHORIZED");
    expect(error.status).toBe(401);
  });

  it("refuses to guess a credential when no account session is stored", async () => {
    const requests: RecordedRequest[] = [];
    stubFetch(requests, jsonResponse({}));

    const error = await captureBillingError(() =>
      fetchBillingEntitlement({ accountOrigin: ORIGIN, accountSessionToken: null }),
    );

    expect(error.code).toBe("ACCOUNT_SESSION_REQUIRED");
    expect(requests).toHaveLength(0);
  });

  it("translates a transport failure into a typed billing error", async () => {
    vi.stubGlobal("fetch", async () => {
      throw new TypeError("Load failed");
    });

    const error = await captureBillingError(() => fetchBillingEntitlement(CALL));

    expect(error.code).toBe("NETWORK_FAILED");
    expect(error.status).toBeNull();
    expect(toBillingApiError(error)).toBe(error);
  });

  it("sends the request without following redirects and with an abort signal", async () => {
    const requests: RecordedRequest[] = [];
    stubFetch(requests, jsonResponse({
      plan: "free",
      status: "ok",
      machineLimit: 1,
      machinesUsed: 0,
      seats: null,
      hostPacks: 0,
      graceEndsAt: null,
      orgId: null,
      role: null,
      manageUrl: null,
    }));

    await fetchBillingEntitlement(CALL);

    expect(requests[0].redirect).toBe("error");
    expect(requests[0].hasSignal).toBe(true);
  });

  it("maps an elapsed request timeout to a typed error", async () => {
    vi.stubGlobal("fetch", (_input: RequestInfo | URL, init?: RequestInit) =>
      new Promise<Response>((_resolve, reject) => {
        const signal = init?.signal;
        const fail = () => reject(new DOMException("Aborted", "AbortError"));
        if (signal?.aborted) fail();
        else signal?.addEventListener("abort", fail, { once: true });
      }),
    );

    const error = await captureBillingError(() =>
      fetchBillingEntitlement({ ...CALL, timeoutMs: 1 }),
    );

    expect(error.code).toBe("REQUEST_TIMEOUT");
    expect(error.status).toBeNull();
  });

  it("refuses to start a request whose caller signal is already aborted", async () => {
    const requests: RecordedRequest[] = [];
    stubFetch(requests, jsonResponse({}));
    const controller = new AbortController();
    controller.abort();

    const error = await captureBillingError(() =>
      fetchBillingEntitlement({ ...CALL, signal: controller.signal }),
    );

    expect(error.code).toBe("REQUEST_ABORTED");
    expect(requests).toHaveLength(0);
  });
});

describe("billingApi plan-limit details", () => {
  it("carries plan, limit and used from a 402 PLAN_LIMIT_REACHED envelope", async () => {
    stubFetch(
      [],
      jsonResponse(
        {
          code: "PLAN_LIMIT_REACHED",
          message: "Plan limit reached",
          details: { plan: "free", limit: 1, used: 2 },
        },
        402,
      ),
    );

    const error = await captureBillingError(() => fetchBillingEntitlement(CALL));

    expect(error.code).toBe("PLAN_LIMIT_REACHED");
    expect(error.status).toBe(402);
    expect(planLimitDetails(error.details)).toEqual({ plan: "free", limit: 1, used: 2 });
  });

  it("carries suspension timestamps from a 402 REMOTE_SUSPENDED envelope", async () => {
    stubFetch(
      [],
      jsonResponse(
        {
          code: "REMOTE_SUSPENDED",
          message: "Remote access suspended",
          details: { plan: "pro_monthly", status: "stopped", graceEndsAt: 1_800_604_800, stoppedAt: 1_800_604_900 },
        },
        402,
      ),
    );

    const error = await captureBillingError(() => fetchBillingEntitlement(CALL));

    expect(error.code).toBe("REMOTE_SUSPENDED");
    expect(suspensionDetails(error.details)).toEqual({
      plan: "pro_monthly",
      status: "stopped",
      graceEndsAt: 1_800_604_800,
      stoppedAt: 1_800_604_900,
    });
  });

  it("returns null details for envelopes without a details object", async () => {
    stubFetch([], jsonResponse({ code: "ORG_ROLE_REQUIRED", message: "owner only" }, 403));

    const error = await captureBillingError(() => fetchBillingEntitlement(CALL));

    expect(error.code).toBe("ORG_ROLE_REQUIRED");
    expect(error.details).toBeNull();
    expect(planLimitDetails(error.details)).toBeNull();
    expect(suspensionDetails(error.details)).toBeNull();
  });
});

describe("billingApi checkout and quantity", () => {
  it("posts the plan and returns the checkout url", async () => {
    const requests: RecordedRequest[] = [];
    stubFetch(requests, jsonResponse({ url: "https://store.example.test/checkout/abc" }));

    const url = await startBillingCheckout("pro_annual", CALL);

    expect(url).toBe("https://store.example.test/checkout/abc");
    expect(requests[0].url).toBe(`${ORIGIN}/api/account/v1/billing/checkout`);
    expect(requests[0].method).toBe("POST");
    expect(requests[0].authorization).toBe(`Bearer ${TOKEN}`);
    expect(requests[0].contentType).toBe("application/json");
    expect(JSON.parse(requests[0].body ?? "null")).toEqual({ plan: "pro_annual" });
  });

  it("includes seats and host packs in the checkout body when asked", async () => {
    const requests: RecordedRequest[] = [];
    stubFetch(requests, jsonResponse({ url: "https://store.example.test/checkout/team" }));

    await startBillingCheckout("team_annual", { ...CALL, seats: 4 });
    await startBillingCheckout("team_hostpack_annual", { ...CALL, hostPacks: 3 });

    expect(JSON.parse(requests[0].body ?? "null")).toEqual({ plan: "team_annual", seats: 4 });
    expect(JSON.parse(requests[1].body ?? "null")).toEqual({
      plan: "team_hostpack_annual",
      hostPacks: 3,
    });
  });

  it("fails without a checkout url instead of opening an empty target", async () => {
    stubFetch([], jsonResponse({}));

    const error = await captureBillingError(() => startBillingCheckout("pro_monthly", CALL));

    expect(error.code).toBe("INVALID_RESPONSE");
  });

  it("posts a quantity change and parses the returned entitlement", async () => {
    const requests: RecordedRequest[] = [];
    stubFetch(
      requests,
      jsonResponse({
        plan: "pro_monthly",
        status: "ok",
        machineLimit: 25,
        machinesUsed: 2,
        seats: null,
        hostPacks: 3,
        graceEndsAt: null,
        orgId: null,
        role: null,
        manageUrl: null,
      }),
    );

    const entitlement = await updateBillingQuantity({ hostPacks: 3 }, CALL);

    expect(requests[0].url).toBe(`${ORIGIN}/api/account/v1/billing/quantity`);
    expect(requests[0].method).toBe("POST");
    expect(JSON.parse(requests[0].body ?? "null")).toEqual({ hostPacks: 3 });
    expect(entitlement.machineLimit).toBe(25);
    expect(entitlement.hostPacks).toBe(3);
  });

  it("posts a seat change without touching host packs", async () => {
    const requests: RecordedRequest[] = [];
    stubFetch(
      requests,
      jsonResponse({
        plan: "team_monthly",
        status: "ok",
        machineLimit: 30,
        machinesUsed: 4,
        seats: 3,
        hostPacks: 0,
        graceEndsAt: null,
        orgId: "org-1",
        role: "owner",
        manageUrl: null,
      }),
    );

    const entitlement = await updateBillingQuantity({ seats: 3 }, CALL);

    expect(JSON.parse(requests[0].body ?? "null")).toEqual({ seats: 3 });
    expect(entitlement.seats).toBe(3);
    expect(entitlement.machineLimit).toBe(30);
  });
});

describe("billingApi org members", () => {
  it("lists members from the canonical route with the bearer", async () => {
    const requests: RecordedRequest[] = [];
    stubFetch(
      requests,
      jsonResponse([
        { userId: "u-owner", email: "owner@example.test", role: "owner" },
        { userId: "u-admin", email: "admin@example.test", role: "admin" },
      ]),
    );

    const members = await listOrgMembers(CALL);

    expect(requests[0].url).toBe(`${ORIGIN}/api/account/v1/org/members`);
    expect(requests[0].authorization).toBe(`Bearer ${TOKEN}`);
    expect(members).toEqual([
      { userId: "u-owner", email: "owner@example.test", role: "owner" },
      { userId: "u-admin", email: "admin@example.test", role: "admin" },
    ]);
  });

  it("accepts a members envelope and skips malformed rows", async () => {
    stubFetch(
      [],
      jsonResponse({
        members: [
          { userId: "u-1", email: "one@example.test", role: "member" },
          { userId: "u-2", email: "two@example.test", role: "chairperson" },
          { email: "no-id@example.test", role: "member" },
        ],
      }),
    );

    const members = await listOrgMembers(CALL);

    expect(members).toEqual([{ userId: "u-1", email: "one@example.test", role: "member" }]);
  });

  it("rejects a members payload that is neither a list nor an envelope", async () => {
    stubFetch([], jsonResponse({ count: 2 }));

    const error = await captureBillingError(() => listOrgMembers(CALL));

    expect(error.code).toBe("INVALID_RESPONSE");
  });

  it("posts the invite email and returns the server invite expiry", async () => {
    const requests: RecordedRequest[] = [];
    const expiresAt = 1_800_604_800;
    stubFetch(requests, () => jsonResponse({ email: "teammate@example.test", expiresAt }));

    const invite = await inviteOrgMember(" teammate@example.test ", CALL);

    expect(requests[0].url).toBe(`${ORIGIN}/api/account/v1/org/invite`);
    expect(requests[0].method).toBe("POST");
    expect(JSON.parse(requests[0].body ?? "null")).toEqual({ email: "teammate@example.test" });
    expect(invite).toEqual({ email: "teammate@example.test", expiresAt });
  });

  it("rejects an invite response without the contract fields", async () => {
    stubFetch([], () => jsonResponse({ status: "invited" }));

    const error = await captureBillingError(() => inviteOrgMember("x@example.test", CALL));

    expect(error.code).toBe("INVALID_RESPONSE");
  });

  it("removes a member through an encoded remove route", async () => {
    const requests: RecordedRequest[] = [];
    stubFetch(requests, jsonResponse(null, 204));

    await removeOrgMember("user/with space", CALL);

    expect(requests[0].url).toBe(
      `${ORIGIN}/api/account/v1/org/members/user%2Fwith%20space/remove`,
    );
    expect(requests[0].method).toBe("POST");
    expect(requests[0].authorization).toBe(`Bearer ${TOKEN}`);
  });

  it("surfaces the owner-only refusal as a typed code", async () => {
    stubFetch([], jsonResponse({ code: "ORG_ROLE_REQUIRED", message: "owner only" }, 403));

    const error = await captureBillingError(() => inviteOrgMember("x@example.test", CALL));

    expect(error.code).toBe("ORG_ROLE_REQUIRED");
    expect(error.status).toBe(403);
  });
});
