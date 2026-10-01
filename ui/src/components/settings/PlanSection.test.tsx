import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { PlanSection } from "./PlanSection";
import {
  deferred,
  observeDom,
  observeRequests,
  pumpRequests,
  settle,
  settledAllTestId,
  settledCount,
  settledTestId,
} from "../../test/domSignals";

const browserNative = vi.hoisted(() => ({
  openExternalUrl: vi.fn(async (_url: string) => undefined),
}));

vi.mock(import("../../lib/browserTauri"), async (importOriginal) => {
  const actual = await importOriginal();
  return {
    ...actual,
    openExternalUrl: browserNative.openExternalUrl,
  };
});

const ORIGIN = "https://account.example.test";
const TOKEN = "session-token-1";
const NOW = 1_800_000_000_000;
const THREE_DAYS_SECONDS = 3 * 86_400;
const GRACE_ENDS_AT = NOW / 1000 + THREE_DAYS_SECONDS - 60;
const CHECKOUT_URL = "https://store.example.test/checkout/abc";

interface RecordedRequest {
  readonly url: string;
  readonly method: string;
  readonly body: string | null;
}

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

function wireEntitlement(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    plan: "free",
    status: "ok",
    machineLimit: 1,
    machinesUsed: 1,
    seats: null,
    hostPacks: 0,
    graceEndsAt: null,
    orgId: null,
    role: null,
    manageUrl: null,
    ...overrides,
  };
}

function requestUrl(input: RequestInfo | URL): string {
  if (typeof input === "string") return input;
  if (input instanceof URL) return input.toString();
  return input.url;
}

type FetchHandler = (url: string, init: RequestInit | undefined) => Response | Promise<Response>;

let activeNormalOps: Set<Promise<unknown>> = new Set();
let registeredNormalResolvers: Array<() => Promise<void>> = [];

function trackResponse(res: Response, trackSet: Set<Promise<unknown>>): Response {
  const originalJson = res.json.bind(res);
  res.json = async () => {
    const jsonPromise = originalJson();
    trackSet.add(jsonPromise);
    void jsonPromise.then(
      () => {
        trackSet.delete(jsonPromise);
      },
      () => {
        trackSet.delete(jsonPromise);
      },
    );
    return await jsonPromise;
  };
  return res;
}

function stubFetch(
  requests: RecordedRequest[],
  handle: FetchHandler,
  isDeferredPredicate?: (url: string) => boolean,
): void {
  activeNormalOps.clear();
  registeredNormalResolvers = [];
  vi.stubGlobal("fetch", async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = requestUrl(input);
    requests.push({
      url,
      method: init?.method ?? "GET",
      body: typeof init?.body === "string" ? init.body : null,
    });
    pumpRequests(requests);

    const isDeferred = isDeferredPredicate ? isDeferredPredicate(url) : false;

    if (isDeferred) {
      // Execute deferred handler immediately without enqueueing into registeredNormalResolvers.
      // This returns the test's held Promise<Response> directly.
      const resPromise = Promise.resolve(handle(url, init));
      return await resPromise;
    }

    // Normal handler: tracked and flushed explicitly via flushFetch() in act()
    const promise = new Promise<Response>((resolve, reject) => {
      registeredNormalResolvers.push(async () => {
        try {
          const res = await handle(url, init);
          resolve(trackResponse(res, activeNormalOps));
        } catch (err) {
          reject(err);
        }
      });
    });

    activeNormalOps.add(promise);
    void promise.then(
      () => {
        activeNormalOps.delete(promise);
      },
      () => {
        activeNormalOps.delete(promise);
      },
    );

    return promise;
  });
}

async function flushFetch(): Promise<void> {
  await act(async () => {
    while (registeredNormalResolvers.length > 0 || activeNormalOps.size > 0) {
      if (registeredNormalResolvers.length > 0) {
        const batch = registeredNormalResolvers;
        registeredNormalResolvers = [];
        for (const resolver of batch) {
          await resolver();
        }
      }
      if (activeNormalOps.size > 0) {
        await Promise.all(Array.from(activeNormalOps));
      }
    }
  });
}

function renderPlan(props: {
  onUnavailable?: () => void;
  now?: number;
  accountSessionToken?: string | null;
} = {}) {
  return render(
    <PlanSection accountOrigin={ORIGIN} accountSessionToken={TOKEN} now={NOW} {...props} />,
  );
}

beforeEach(() => {
  browserNative.openExternalUrl.mockReset();
  browserNative.openExternalUrl.mockImplementation(async () => undefined);
});

afterEach(() => {
  registeredNormalResolvers = [];
  activeNormalOps.clear();
  cleanup();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("PlanSection entitlement states", () => {
  it("shows a loading state until the entitlement resolves", async () => {
    let resolveEntitlement: (response: Response) => void = () => {};
    vi.stubGlobal(
      "fetch",
      () =>
        new Promise<Response>((resolve) => {
          resolveEntitlement = resolve;
        }),
    );

    const summarySignal = observeDom(() => screen.queryByTestId("plan-summary"));

    renderPlan();

    expect(screen.getByTestId("plan-loading")).toBeInTheDocument();

    await act(async () => {
      resolveEntitlement(jsonResponse(wireEntitlement()));
    });

    const summary = await settle(summarySignal);
    expect(summary).toHaveAttribute("data-status", "ok");
    expect(screen.queryByTestId("plan-loading")).toBeNull();
  });

  it("renders an active plan without a deadline banner", async () => {
    stubFetch([], () => jsonResponse(wireEntitlement({ status: "ok", machinesUsed: 0 })));

    const summarySignal = observeDom(() => screen.queryByTestId("plan-summary"));
    renderPlan();
    await flushFetch();

    const summary = await settle(summarySignal);
    expect(summary).toHaveAttribute("data-plan", "free");
    expect(summary).toHaveAttribute("data-status", "ok");
    expect(screen.getByTestId("plan-usage").textContent).toContain("0 of 1");
    expect(screen.queryByTestId("plan-grace-banner")).toBeNull();
    expect(screen.queryByTestId("plan-stopped-banner")).toBeNull();
  });

  it("renders an over-limit plan with the grace deadline in days", async () => {
    stubFetch([], () =>
      jsonResponse(
        wireEntitlement({
          plan: "pro_monthly",
          status: "over_limit",
          machineLimit: 10,
          machinesUsed: 12,
          hostPacks: 0,
          graceEndsAt: GRACE_ENDS_AT,
        }),
      ),
    );

    const bannerSignal = observeDom(() => screen.queryByTestId("plan-grace-banner"));
    renderPlan();
    await flushFetch();

    const banner = await settle(bannerSignal);
    expect(banner).toHaveAttribute("data-status", "over_limit");
    expect(banner).toHaveAttribute("data-days-left", "3");
    expect(banner).toHaveAttribute("data-grace-ends-at", String(GRACE_ENDS_AT));
    expect(within(banner).getByTestId("plan-local-note")).toBeInTheDocument();
    expect(screen.queryByTestId("plan-stopped-banner")).toBeNull();
  });

  it("renders a past-due plan with the same grace deadline surface", async () => {
    stubFetch([], () =>
      jsonResponse(
        wireEntitlement({
          plan: "pro_annual",
          status: "past_due",
          machineLimit: 10,
          machinesUsed: 1,
          graceEndsAt: GRACE_ENDS_AT,
        }),
      ),
    );

    const bannerSignal = observeDom(() => screen.queryByTestId("plan-grace-banner"));
    renderPlan();
    await flushFetch();

    const banner = await settle(bannerSignal);
    expect(banner).toHaveAttribute("data-status", "past_due");
    expect(banner).toHaveAttribute("data-days-left", "3");
  });

  it("renders a stopped plan with the stopped banner and no grace countdown", async () => {
    stubFetch([], () =>
      jsonResponse(
        wireEntitlement({
          plan: "pro_monthly",
          status: "stopped",
          machineLimit: 10,
          machinesUsed: 2,
          graceEndsAt: GRACE_ENDS_AT,
        }),
      ),
    );

    const bannerSignal = observeDom(() => screen.queryByTestId("plan-stopped-banner"));
    renderPlan();
    await flushFetch();

    const banner = await settle(bannerSignal);
    expect(banner).toHaveAttribute("data-status", "stopped");
    expect(within(banner).getByTestId("plan-local-note")).toBeInTheDocument();
    expect(screen.queryByTestId("plan-grace-banner")).toBeNull();
  });

  it("keeps the upgrade path enabled while remote access is stopped", async () => {
    stubFetch([], () =>
      jsonResponse(
        wireEntitlement({
          plan: "free",
          status: "stopped",
          machinesUsed: 1,
          graceEndsAt: GRACE_ENDS_AT,
        }),
      ),
    );

    const summarySignal = observeDom(() => screen.queryByTestId("plan-summary"));
    renderPlan();
    await flushFetch();
    await settle(summarySignal);

    expect(screen.getByRole("button", { name: "Pro monthly" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Pro yearly" })).toBeEnabled();
  });

  it("renders a Pro plan with the usage-bar percentage", async () => {
    stubFetch([], () =>
      jsonResponse(wireEntitlement({ plan: "pro_monthly", machineLimit: 20, machinesUsed: 2 })),
    );

    const summarySignal = observeDom(() => screen.queryByTestId("plan-summary"));
    renderPlan();
    await flushFetch();

    const summary = await settle(summarySignal);
    expect(summary).toHaveAttribute("data-plan", "pro_monthly");
    expect(summary).toHaveAttribute("data-status", "ok");
    const bar = screen.getByRole("progressbar");
    expect(bar).toHaveAttribute("aria-valuenow", "10");
  });

  it.each([
    ["ok", 1, null],
    ["over_limit", 2, GRACE_ENDS_AT],
    ["past_due", 1, GRACE_ENDS_AT],
    ["stopped", 1, GRACE_ENDS_AT],
  ] as const)(
    "keeps the plan actions enabled and renders no local control in the %s state",
    async (status, machinesUsed, graceEndsAt) => {
      stubFetch([], () =>
        jsonResponse(wireEntitlement({ status, machinesUsed, graceEndsAt })),
      );

      const summarySignal = observeDom(() => screen.queryByTestId("plan-summary"));
      renderPlan();
      await flushFetch();

      const summary = await settle(summarySignal);
      expect(summary).toHaveAttribute("data-status", status);
      expect(
        screen.queryByRole("button", { name: /terminal|worktree|pairing|localhost/i }),
      ).toBeNull();
      const buttons = screen.getAllByRole("button");
      expect(buttons.length).toBeGreaterThan(0);
      for (const button of buttons) {
        expect(button).toBeEnabled();
      }
    },
  );
});

describe("PlanSection deployment and failure handling", () => {
  it("hides itself and reports unavailability when the account server answers 404", async () => {
    const requests: RecordedRequest[] = [];
    const unavailability = deferred<null>();
    const onUnavailable = vi.fn(() => unavailability.resolve(null));
    stubFetch(requests, () => jsonResponse({ code: "NOT_FOUND", message: "not found" }, 404));

    const reqSignal = observeRequests(requests, "/billing/entitlement", 1);
    const removalSignal = observeDom(() => (document.querySelector("section") === null ? true : null));
    const { container } = renderPlan({ onUnavailable });
    await flushFetch();

    await settle(reqSignal);
    await settle(unavailability);
    await settle(removalSignal);
    expect(onUnavailable).toHaveBeenCalledTimes(1);
    expect(container.querySelector("section")).toBeNull();
    expect(screen.queryByTestId("plan-summary")).toBeNull();
  });

  it("ignores a late entitlement response from a replaced origin", async () => {
    const signals: Array<AbortSignal | undefined> = [];
    const deferred: Array<(response: Response) => void> = [];
    vi.stubGlobal("fetch", async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = requestUrl(input);
      signals.push(init?.signal ?? undefined);
      if (url.startsWith("https://first.example.test")) {
        return new Promise<Response>((resolve) => {
          deferred.push(resolve);
        });
      }
      return jsonResponse(
        wireEntitlement({
          plan: "pro_annual",
          status: "past_due",
          machineLimit: 10,
          machinesUsed: 12,
          graceEndsAt: GRACE_ENDS_AT,
        }),
      );
    });

    const { rerender } = render(
      <PlanSection
        accountOrigin="https://first.example.test"
        accountSessionToken={TOKEN}
        now={NOW}
      />,
    );
    expect(screen.getByTestId("plan-loading")).toBeInTheDocument();

    const summarySignal = observeDom(() => screen.queryByTestId("plan-summary"));
    rerender(<PlanSection accountOrigin={ORIGIN} accountSessionToken={TOKEN} now={NOW} />);

    const summary = await settle(summarySignal);
    expect(summary).toHaveAttribute("data-plan", "pro_annual");
    expect(summary).toHaveAttribute("data-status", "past_due");
    expect(signals[0]?.aborted).toBe(true);

    await act(async () => {
      for (const resolve of deferred) {
        resolve(
          jsonResponse(wireEntitlement({ plan: "free", status: "ok", machineLimit: 1, machinesUsed: 0 })),
        );
      }
    });

    expect(screen.getByTestId("plan-summary")).toHaveAttribute("data-plan", "pro_annual");
    expect(screen.getByTestId("plan-summary")).toHaveAttribute("data-status", "past_due");
  });

  it("ignores a deferred checkout response after origin replacement and does not open old URL", async () => {
    let resolveCheckout!: (response: Response) => void;
    const checkoutRequests: RecordedRequest[] = [];
    stubFetch(
      checkoutRequests,
      (url) => {
        if (url.includes("/billing/checkout")) {
          return new Promise<Response>((res) => {
            resolveCheckout = res;
          });
        }
        return jsonResponse(wireEntitlement({ plan: "free", machineLimit: 1, machinesUsed: 0 }));
      },
      (url) => url.includes("/billing/checkout"),
    );

    const { rerender } = render(
      <PlanSection accountOrigin={ORIGIN} accountSessionToken={TOKEN} now={NOW} />,
    );
    await flushFetch();
    const summarySignal = observeDom(() => screen.queryByTestId("plan-summary"));
    await settle(summarySignal);

    const checkoutReqSignal = observeRequests(checkoutRequests, "/billing/checkout", 1);
    fireEvent.click(screen.getByRole("button", { name: "Pro monthly" }));
    await settle(checkoutReqSignal);

    const newEntitlementSignal = observeRequests(checkoutRequests, "/billing/entitlement", 2);
    rerender(
      <PlanSection
        accountOrigin="https://new-origin.example.test"
        accountSessionToken="new-token"
        now={NOW}
      />,
    );
    await flushFetch();
    await settle(newEntitlementSignal);

    await act(async () => {
      resolveCheckout(jsonResponse({ url: "https://old-store.example.test/checkout/old" }));
    });

    expect(browserNative.openExternalUrl).not.toHaveBeenCalled();
  });

  it("ignores a deferred quantity response after token replacement and does not overwrite entitlement or renewal notice", async () => {
    let resolveQuantity!: (response: Response) => void;
    const quantityRequests: RecordedRequest[] = [];
    stubFetch(
      quantityRequests,
      (url) => {
        if (url.includes("/billing/quantity")) {
          return new Promise<Response>((res) => {
            resolveQuantity = res;
          });
        }
        return jsonResponse(
          wireEntitlement({ plan: "pro_monthly", machineLimit: 10, machinesUsed: 2, hostPacks: 0 }),
        );
      },
      (url) => url.includes("/billing/quantity"),
    );

    const { rerender } = render(
      <PlanSection accountOrigin={ORIGIN} accountSessionToken={TOKEN} now={NOW} />,
    );
    await flushFetch();
    const summarySignal = observeDom(() => screen.queryByTestId("plan-summary"));
    await settle(summarySignal);

    const quantityReqSignal = observeRequests(quantityRequests, "/billing/quantity", 1);
    fireEvent.change(screen.getByLabelText("Additional machine packs"), {
      target: { value: "3" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Apply packs" }));
    await settle(quantityReqSignal);

    // Token replaced on same origin (account change)
    const newEntitlementSignal = observeRequests(quantityRequests, "/billing/entitlement", 2);
    rerender(
      <PlanSection
        accountOrigin={ORIGIN}
        accountSessionToken="different-token-2"
        now={NOW}
      />,
    );
    await flushFetch();
    await settle(newEntitlementSignal);

    await act(async () => {
      resolveQuantity(
        jsonResponse(
          wireEntitlement({ plan: "pro_monthly", machineLimit: 25, machinesUsed: 2, hostPacks: 3 }),
        ),
      );
    });

    expect(screen.queryByTestId("plan-renewal-note")).toBeNull();
    const summary = screen.getByTestId("plan-summary");
    expect(summary).toHaveAttribute("data-plan", "pro_monthly");
    expect(screen.getByRole("progressbar")).toHaveAttribute("aria-valuenow", "20"); // 2 of 10, NOT 2 of 25
  });

  it("ignores deferred invite and remove responses after origin or token replacement and does not republish old state", async () => {
    let resolveInvite!: (response: Response) => void;
    let resolveRemove!: (response: Response) => void;
    const staleRequests: RecordedRequest[] = [];
    const oldOriginMembersRequests: string[] = [];

    stubFetch(
      staleRequests,
      (url) => {
        if (url.includes("/org/invite")) {
          return new Promise<Response>((res) => {
            resolveInvite = res;
          });
        }
        if (url.includes("/remove")) {
          return new Promise<Response>((res) => {
            resolveRemove = res;
          });
        }
        if (url.includes("/org/members")) {
          if (url.startsWith(ORIGIN)) {
            oldOriginMembersRequests.push(url);
          }
          return jsonResponse([
            { userId: "u-owner", email: "owner@example.test", role: "owner" },
            { userId: "u-member", email: "member@example.test", role: "member" },
          ]);
        }
        return jsonResponse(
          wireEntitlement({
            plan: "team_monthly",
            machineLimit: 20,
            machinesUsed: 2,
            seats: 2,
            orgId: "org-1",
            role: "owner",
          }),
        );
      },
      (url) => url.includes("/org/invite") || url.includes("/remove"),
    );

    const initialMembersSignal = observeRequests(staleRequests, "/org/members", 1);
    const { rerender, unmount } = render(
      <PlanSection accountOrigin={ORIGIN} accountSessionToken={TOKEN} now={NOW} />,
    );
    await flushFetch();
    await settle(initialMembersSignal);
    await flushFetch();
    await settledCount("plan-member-row", 2);

    // Trigger invite
    const inviteReqSignal = observeRequests(staleRequests, "/org/invite", 1);
    fireEvent.change(screen.getByLabelText("Invite member email"), {
      target: { value: "pending@example.test" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Invite" }));
    await settle(inviteReqSignal);

    // Switch origin
    const newEntitlementSignal = observeRequests(staleRequests, "/billing/entitlement", 2);
    rerender(
      <PlanSection
        accountOrigin="https://new-origin.example.test"
        accountSessionToken="new-token"
        now={NOW}
      />,
    );
    await flushFetch();
    await settle(newEntitlementSignal);

    const initialOldOriginFetchCount = oldOriginMembersRequests.length;

    await act(async () => {
      resolveInvite(jsonResponse({ email: "pending@example.test", expiresAt: 1_800_604_800 }));
    });

    expect(screen.queryByTestId("plan-invite-sent")).toBeNull();
    // does NOT invoke old loadMembers for old origin
    expect(oldOriginMembersRequests.length).toBe(initialOldOriginFetchCount);

    // Unmount before starting the remove flow to ensure clean DOM container
    unmount();

    // Now test deferred remove on same-origin token replacement
    const removeRequests: RecordedRequest[] = [];
    stubFetch(
      removeRequests,
      (url) => {
        if (url.includes("/remove")) {
          return new Promise<Response>((res) => {
            resolveRemove = res;
          });
        }
        if (url.includes("/org/members")) {
          if (url.startsWith(ORIGIN)) {
            oldOriginMembersRequests.push(url);
          }
          return jsonResponse([
            { userId: "u-owner", email: "owner@example.test", role: "owner" },
            { userId: "u-member", email: "member@example.test", role: "member" },
          ]);
        }
        return jsonResponse(
          wireEntitlement({
            plan: "team_monthly",
            machineLimit: 20,
            machinesUsed: 2,
            seats: 2,
            orgId: "org-1",
            role: "owner",
          }),
        );
      },
      (url) => url.includes("/remove"),
    );

    const removeMembersSignal = observeRequests(removeRequests, "/org/members", 1);
    const { rerender: rerenderRemove } = render(
      <PlanSection accountOrigin={ORIGIN} accountSessionToken={TOKEN} now={NOW} />,
    );
    await flushFetch();
    await settle(removeMembersSignal);
    await flushFetch();
    const memberRows = await settledCount("plan-member-row", 2);
    const memberRow = memberRows.find((row) => row.getAttribute("data-role") === "member");
    if (!memberRow) throw new Error("expected member row");

    const removeReqSignal = observeRequests(removeRequests, "/remove", 1);
    fireEvent.click(within(memberRow).getByRole("button", { name: /^Remove / }));
    fireEvent.click(within(memberRow).getByRole("button", { name: /^Confirm remove / }));
    await settle(removeReqSignal);

    // Replaced token before remove resolves
    const replaceTokenEntitlementSignal = observeRequests(removeRequests, "/billing/entitlement", 2);
    rerenderRemove(
      <PlanSection accountOrigin={ORIGIN} accountSessionToken="different-token-3" now={NOW} />,
    );
    await flushFetch();
    await settle(replaceTokenEntitlementSignal);

    const preResolveOldTokenCount = oldOriginMembersRequests.length;
    await act(async () => {
      resolveRemove(jsonResponse({ ok: true }, 200));
    });

    // Should not trigger old loadMembers for predecessor account
    expect(oldOriginMembersRequests.length).toBe(preResolveOldTokenCount);
  });

  it("offers a focusable retry after a server failure and recovers on retry", async () => {
    let attempt = 0;
    stubFetch([], () => {
      attempt += 1;
      if (attempt === 1) {
        return jsonResponse({ code: "INTERNAL_ERROR", message: "boom" }, 500);
      }
      return jsonResponse(wireEntitlement({ plan: "pro_monthly", machineLimit: 10 }));
    });

    const errorSignal = observeDom(() => screen.queryByTestId("plan-error"));
    renderPlan();
    await flushFetch();

    const failure = await settle(errorSignal);
    expect(failure).toHaveAttribute("data-code", "INTERNAL_ERROR");

    const retry = within(failure).getByRole("button", { name: "Retry" });
    retry.focus();
    expect(retry).toHaveFocus();

    const summarySignal = observeDom(() => screen.queryByTestId("plan-summary"));
    fireEvent.click(retry);
    await flushFetch();

    const summary = await settle(summarySignal);
    expect(summary).toHaveAttribute("data-plan", "pro_monthly");
    expect(screen.queryByTestId("plan-error")).toBeNull();
  });

  it("explains an expired session on 401 and still allows retry", async () => {
    stubFetch([], () => jsonResponse({ code: "UNAUTHORIZED", message: "expired" }, 401));

    const errorSignal = observeDom(() => screen.queryByTestId("plan-error"));
    renderPlan();
    await flushFetch();

    const failure = await settle(errorSignal);
    expect(failure).toHaveAttribute("data-code", "UNAUTHORIZED");
    expect(within(failure).getByRole("button", { name: "Retry" })).toBeEnabled();
  });

  it("retains suspension details when the entitlement fetch itself answers 402", async () => {
    stubFetch([], () =>
      jsonResponse(
        {
          code: "REMOTE_SUSPENDED",
          message: "Remote access suspended",
          details: {
            plan: "pro_monthly",
            status: "stopped",
            graceEndsAt: GRACE_ENDS_AT,
            stoppedAt: GRACE_ENDS_AT,
          },
        },
        402,
      ),
    );

    const errorSignal = observeDom(() => screen.queryByTestId("plan-error"));
    renderPlan();
    await flushFetch();

    const failure = await settle(errorSignal);
    expect(failure).toHaveAttribute("data-code", "REMOTE_SUSPENDED");
    expect(failure).toHaveAttribute("data-grace-ends-at", String(GRACE_ENDS_AT));
    expect(within(failure).getByRole("button", { name: "Retry" })).toBeEnabled();
  });
});

describe("PlanSection checkout and quantity", () => {
  it("opens the returned checkout url through the external-url bridge", async () => {
    const requests: RecordedRequest[] = [];
    stubFetch(requests, (url) => {
      if (url.endsWith("/billing/checkout")) return jsonResponse({ url: CHECKOUT_URL });
      return jsonResponse(wireEntitlement({ plan: "pro_monthly", machineLimit: 10 }));
    });

    const opened = deferred<string>();
    browserNative.openExternalUrl.mockImplementation(async (url: string) => {
      opened.resolve(url);
    });

    const summarySignal = observeDom(() => screen.queryByTestId("plan-summary"));
    renderPlan();
    await flushFetch();
    await settle(summarySignal);

    const checkoutReqSignal = observeRequests(requests, "/billing/checkout", 1);
    const button = screen.getByRole("button", { name: "Pro yearly" });
    button.focus();
    expect(button).toHaveFocus();
    fireEvent.click(button);
    await flushFetch();
    await settle(checkoutReqSignal);

    expect(await settle(opened)).toBe(CHECKOUT_URL);
    const checkout = requests.find((entry) => entry.url.endsWith("/billing/checkout"));
    expect(checkout?.method).toBe("POST");
    expect(JSON.parse(checkout?.body ?? "null")).toEqual({ plan: "pro_annual" });
  });

  it("renders plan-limit details from a 402 checkout refusal without opening a url", async () => {
    stubFetch([], (url) => {
      if (url.endsWith("/billing/checkout")) {
        return jsonResponse(
          {
            code: "PLAN_LIMIT_REACHED",
            message: "Plan limit reached",
            details: { plan: "free", limit: 1, used: 2 },
          },
          402,
        );
      }
      return jsonResponse(wireEntitlement({ machinesUsed: 2 }));
    });

    const summarySignal = observeDom(() => screen.queryByTestId("plan-summary"));
    renderPlan();
    await flushFetch();
    await settle(summarySignal);

    const errorSignal = observeDom(() => screen.queryByTestId("plan-action-error"));
    fireEvent.click(screen.getByRole("button", { name: "Pro monthly" }));
    await flushFetch();

    const notice = await settle(errorSignal);
    expect(notice).toHaveAttribute("data-code", "PLAN_LIMIT_REACHED");
    expect(notice).toHaveAttribute("data-limit", "1");
    expect(notice).toHaveAttribute("data-used", "2");
    expect(browserNative.openExternalUrl).not.toHaveBeenCalled();
  });

  it("applies a pack change, announces the next renewal and refreshes the limit", async () => {
    const requests: RecordedRequest[] = [];
    stubFetch(requests, (url) => {
      if (url.endsWith("/billing/quantity")) {
        return jsonResponse(
          wireEntitlement({ plan: "pro_monthly", machineLimit: 20, machinesUsed: 2, hostPacks: 2 }),
        );
      }
      return jsonResponse(
        wireEntitlement({ plan: "pro_monthly", machineLimit: 10, machinesUsed: 2, hostPacks: 0 }),
      );
    });

    const summarySignal = observeDom(() => screen.queryByTestId("plan-summary"));
    renderPlan();
    await flushFetch();
    await settle(summarySignal);

    const usageSignal = observeDom(() => {
      const bar = screen.queryByRole("progressbar");
      return bar?.getAttribute("aria-valuenow") === "10" ? bar : null;
    });
    const noteSignal = observeDom(() => screen.queryByTestId("plan-renewal-note"));

    fireEvent.change(screen.getByLabelText("Additional machine packs"), {
      target: { value: "2" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Apply packs" }));
    await flushFetch();

    const note = await settle(noteSignal);
    expect(note).toHaveAttribute("data-charge", "next-renewal");
    expect(note).toHaveAttribute("data-change", "hostPacks");
    expect(note).toHaveAttribute("data-limit", "20");
    const quantity = requests.find((entry) => entry.url.endsWith("/billing/quantity"));
    expect(quantity?.method).toBe("POST");
    expect(JSON.parse(quantity?.body ?? "null")).toEqual({ hostPacks: 2 });
    expect(await settle(usageSignal)).toHaveAttribute("aria-valuenow", "10");
  });

  it("shows suspension details when a quantity change is refused with 402", async () => {
    stubFetch([], (url) => {
      if (url.endsWith("/billing/quantity")) {
        return jsonResponse(
          {
            code: "REMOTE_SUSPENDED",
            message: "Remote access suspended",
            details: { plan: "pro_monthly", status: "stopped", graceEndsAt: GRACE_ENDS_AT, stoppedAt: GRACE_ENDS_AT },
          },
          402,
        );
      }
      return jsonResponse(
        wireEntitlement({ plan: "pro_monthly", machineLimit: 10, machinesUsed: 2, hostPacks: 0 }),
      );
    });

    const summarySignal = observeDom(() => screen.queryByTestId("plan-summary"));
    renderPlan();
    await flushFetch();
    await settle(summarySignal);

    const errorSignal = observeDom(() => screen.queryByTestId("plan-action-error"));
    fireEvent.change(screen.getByLabelText("Additional machine packs"), {
      target: { value: "1" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Apply packs" }));
    await flushFetch();

    const notice = await settle(errorSignal);
    expect(notice).toHaveAttribute("data-code", "REMOTE_SUSPENDED");
    expect(notice).toHaveAttribute("data-grace-ends-at", String(GRACE_ENDS_AT));
  });
});

describe("PlanSection team management", () => {
  it("keeps plan and member controls hidden for a plain member", async () => {
    stubFetch([], () =>
      jsonResponse(
        wireEntitlement({
          plan: "team_monthly",
          machineLimit: 20,
          machinesUsed: 3,
          seats: 2,
          orgId: "org-1",
          role: "member",
        }),
      ),
    );

    const summarySignal = observeDom(() => screen.queryByTestId("plan-summary"));
    const noteSignal = observeDom(() => screen.queryByTestId("plan-managed-note"));
    renderPlan();
    await flushFetch();
    await settle(summarySignal);
    await settle(noteSignal);

    expect(screen.queryByLabelText("Team seats")).toBeNull();
    expect(screen.queryByLabelText("Additional machine packs")).toBeNull();
    expect(screen.queryByRole("button", { name: "Pro monthly" })).toBeNull();
    expect(screen.queryByTestId("plan-member-row")).toBeNull();
    expect(screen.queryByTestId("plan-members-card")).toBeNull();
    expect(screen.queryByLabelText("Invite member email")).toBeNull();
    expect(screen.getByTestId("plan-managed-note")).toBeInTheDocument();
  });

  it("posts a seat change for an owner and refreshes the machine limit", async () => {
    const requests: RecordedRequest[] = [];
    stubFetch(requests, (url) => {
      if (url.endsWith("/billing/quantity")) {
        return jsonResponse(
          wireEntitlement({
            plan: "team_monthly",
            machineLimit: 30,
            machinesUsed: 3,
            seats: 3,
            orgId: "org-1",
            role: "owner",
          }),
        );
      }
      if (url.endsWith("/org/members")) {
        return jsonResponse([{ userId: "u-owner", email: "owner@example.test", role: "owner" }]);
      }
      return jsonResponse(
        wireEntitlement({
          plan: "team_monthly",
          machineLimit: 20,
          machinesUsed: 3,
          seats: 2,
          orgId: "org-1",
          role: "owner",
        }),
      );
    });

    const membersSignal = observeRequests(requests, "/org/members", 1);
    const summarySignal = observeDom(() => screen.queryByTestId("plan-summary"));
    const memberSignal = observeDom(() => screen.queryByTestId("plan-member-row"));
    renderPlan();
    await flushFetch();
    await settle(summarySignal);
    await settle(membersSignal);
    await flushFetch();
    await settle(memberSignal);

    const noteSignal = observeDom(() => screen.queryByTestId("plan-renewal-note"));
    fireEvent.change(screen.getByLabelText("Team seats"), { target: { value: "3" } });
    fireEvent.click(screen.getByRole("button", { name: "Apply seats" }));
    await flushFetch();

    const note = await settle(noteSignal);
    expect(note).toHaveAttribute("data-charge", "next-renewal");
    expect(note).toHaveAttribute("data-change", "seats");
    expect(note).toHaveAttribute("data-limit", "30");
    const quantity = requests.find((entry) => entry.url.endsWith("/billing/quantity"));
    expect(JSON.parse(quantity?.body ?? "null")).toEqual({ seats: 3 });
  });

  it("lets an admin see members, invite, and change seats but never remove", async () => {
    const requests: RecordedRequest[] = [];
    stubFetch(requests, (url) => {
      if (url.endsWith("/org/members")) {
        return jsonResponse([
          { userId: "u-owner", email: "owner@example.test", role: "owner" },
          { userId: "u-admin", email: "admin@example.test", role: "admin" },
        ]);
      }
      return jsonResponse(
        wireEntitlement({
          plan: "team_annual",
          machineLimit: 20,
          machinesUsed: 3,
          seats: 2,
          orgId: "org-1",
          role: "admin",
        }),
      );
    });

    const membersSignal = observeRequests(requests, "/org/members", 1);
    const summarySignal = observeDom(() => screen.queryByTestId("plan-summary"));
    renderPlan();
    await flushFetch();
    await settle(summarySignal);
    await settle(membersSignal);
    await flushFetch();

    await settledCount("plan-member-row", 2);
    expect(screen.getByTestId("plan-members-card")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /^Remove / })).toBeNull();
    expect(screen.getByLabelText("Invite member email")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Invite" })).toBeInTheDocument();
    expect(screen.getByLabelText("Team seats")).toBeInTheDocument();
  });

  it("protects the owner row from removal and removes a member from the team", async () => {
    const requests: RecordedRequest[] = [];
    let membersVersion = 0;
    stubFetch(requests, (url) => {
      if (url.endsWith("/org/members")) {
        membersVersion += 1;
        return jsonResponse(
          membersVersion === 1
            ? [
                { userId: "u-owner", email: "owner@example.test", role: "owner" },
                { userId: "u-member", email: "member@example.test", role: "member" },
              ]
            : [{ userId: "u-owner", email: "owner@example.test", role: "owner" }],
        );
      }
      if (url.includes("/remove")) {
        return jsonResponse({ ok: true }, 200);
      }
      return jsonResponse(
        wireEntitlement({
          plan: "team_monthly",
          machineLimit: 20,
          machinesUsed: 3,
          seats: 2,
          orgId: "org-1",
          role: "owner",
        }),
      );
    });

    const initialMembersSignal = observeRequests(requests, "/org/members", 1);
    const summarySignal = observeDom(() => screen.queryByTestId("plan-summary"));
    renderPlan();
    await flushFetch();
    await settle(summarySignal);
    await settle(initialMembersSignal);
    await flushFetch();

    const rows = await settledCount("plan-member-row", 2);
    const ownerRow = rows.find((row) => row.getAttribute("data-role") === "owner");
    const memberRow = rows.find((row) => row.getAttribute("data-role") === "member");
    if (ownerRow === undefined || memberRow === undefined) {
      throw new Error("expected one owner row and one member row");
    }
    expect(within(ownerRow).queryByRole("button")).toBeNull();

    const confirmButtonSignal = observeDom(() =>
      within(memberRow).queryByRole("button", { name: /^Confirm remove / }),
    );
    fireEvent.click(within(memberRow).getByRole("button", { name: /^Remove / }));
    const confirmButton = await settle(confirmButtonSignal);

    const removeSignal = observeRequests(requests, "/remove", 1);
    const reloadMembersSignal = observeRequests(requests, "/org/members", 2);
    const rowsSignal = observeDom(() => {
      const remaining = screen.queryAllByTestId("plan-member-row");
      return remaining.length === 1 ? remaining : null;
    });

    fireEvent.click(confirmButton);
    await flushFetch();
    await settle(removeSignal);
    await settle(reloadMembersSignal);
    await flushFetch();

    const remainingRows = await settle(rowsSignal);
    expect(remainingRows).toHaveLength(1);
    expect(screen.queryByText("member@example.test")).toBeNull();
  });

  it("reports the sent invitation without listing the invitee as a member", async () => {
    const requests: RecordedRequest[] = [];
    const inviteEmail = "invited@example.test";
    const inviteExpiresAt = 1_800_604_800;
    stubFetch(requests, (url) => {
      if (url.includes("/org/members")) {
        return jsonResponse([{ userId: "u-owner", email: "owner@example.test", role: "owner" }]);
      }
      if (url.includes("/org/invite")) {
        return jsonResponse({ email: inviteEmail, expiresAt: inviteExpiresAt });
      }
      return jsonResponse(
        wireEntitlement({
          plan: "team_monthly",
          machineLimit: 20,
          machinesUsed: 3,
          seats: 2,
          orgId: "org-1",
          role: "owner",
        }),
      );
    });

    const initialMembersSignal = observeRequests(requests, "/org/members", 1);
    const summarySignal = observeDom(() => screen.queryByTestId("plan-summary"));
    const initialMemberSignal = observeDom(() => screen.queryByTestId("plan-member-row"));
    renderPlan();
    await flushFetch();
    await settle(summarySignal);
    await settle(initialMembersSignal);
    await flushFetch();
    await settle(initialMemberSignal);

    const inviteSignal = observeRequests(requests, "/org/invite", 1);
    const reloadMembersSignal = observeRequests(requests, "/org/members", 2);
    const sentSignal = observeDom(() => screen.queryByTestId("plan-invite-sent"));

    fireEvent.change(screen.getByLabelText("Invite member email"), {
      target: { value: inviteEmail },
    });
    fireEvent.click(screen.getByRole("button", { name: "Invite" }));
    await flushFetch();
    await settle(inviteSignal);
    await settle(reloadMembersSignal);
    await flushFetch();

    const sent = await settle(sentSignal);
    expect(sent).toHaveAttribute("data-email", inviteEmail);
    expect(sent).toHaveAttribute("data-expires-at", String(inviteExpiresAt));

    const invited = await settle(inviteSignal);
    expect(JSON.parse(invited[0].body ?? "null")).toEqual({ email: inviteEmail });
    const rows = screen.getAllByTestId("plan-member-row");
    expect(rows).toHaveLength(1);
    expect(rows[0]).toHaveAttribute("data-user-id", "u-owner");
  });

  it("surfaces an owner-only refusal from the server as a typed code", async () => {
    const requests: RecordedRequest[] = [];
    stubFetch(requests, (url) => {
      if (url.endsWith("/org/members")) {
        return jsonResponse([{ userId: "u-owner", email: "owner@example.test", role: "owner" }]);
      }
      if (url.endsWith("/org/invite")) {
        return jsonResponse({ code: "ORG_ROLE_REQUIRED", message: "owner only" }, 403);
      }
      return jsonResponse(
        wireEntitlement({
          plan: "team_monthly",
          machineLimit: 20,
          machinesUsed: 3,
          seats: 2,
          orgId: "org-1",
          role: "owner",
        }),
      );
    });

    const membersSignal = observeRequests(requests, "/org/members", 1);
    const summarySignal = observeDom(() => screen.queryByTestId("plan-summary"));
    const memberSignal = observeDom(() => screen.queryByTestId("plan-member-row"));
    renderPlan();
    await flushFetch();
    await settle(summarySignal);
    await settle(membersSignal);
    await flushFetch();
    await settle(memberSignal);

    const errorSignal = observeDom(() => screen.queryByTestId("plan-action-error"));
    fireEvent.change(screen.getByLabelText("Invite member email"), {
      target: { value: "x@example.test" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Invite" }));
    await flushFetch();

    const notice = await settle(errorSignal);
    expect(notice).toHaveAttribute("data-code", "ORG_ROLE_REQUIRED");
  });

  it("surfaces a structured remove refusal and keeps the member row", async () => {
    const requests: RecordedRequest[] = [];
    stubFetch(requests, (url) => {
      if (url.includes("/remove")) {
        return jsonResponse({ code: "ORG_ROLE_REQUIRED", message: "owner only" }, 403);
      }
      if (url.includes("/org/members")) {
        return jsonResponse([
          { userId: "u-owner", email: "owner@example.test", role: "owner" },
          { userId: "u-member", email: "member@example.test", role: "member" },
        ]);
      }
      return jsonResponse(
        wireEntitlement({
          plan: "team_monthly",
          machineLimit: 20,
          machinesUsed: 3,
          seats: 2,
          orgId: "org-1",
          role: "owner",
        }),
      );
    });

    const membersSignal = observeRequests(requests, "/org/members", 1);
    const summarySignal = observeDom(() => screen.queryByTestId("plan-summary"));
    renderPlan();
    await flushFetch();
    await settle(summarySignal);
    await settle(membersSignal);
    await flushFetch();

    const rows = await settledAllTestId("plan-member-row");
    const memberRow = rows.find((row) => row.getAttribute("data-role") === "member");
    if (memberRow === undefined) throw new Error("expected a member row");

    const confirmButtonSignal = observeDom(() =>
      within(memberRow).queryByRole("button", { name: /^Confirm remove / }),
    );
    fireEvent.click(within(memberRow).getByRole("button", { name: /^Remove / }));
    const confirmButton = await settle(confirmButtonSignal);

    const errorSignal = observeDom(() => screen.queryByTestId("plan-action-error"));
    fireEvent.click(confirmButton);
    await flushFetch();

    const notice = await settle(errorSignal);
    expect(notice).toHaveAttribute("data-code", "ORG_ROLE_REQUIRED");
    expect(screen.getAllByTestId("plan-member-row")).toHaveLength(2);
  });

  it("keeps the full accessible remove name for a long member email", async () => {
    const requests: RecordedRequest[] = [];
    const longEmail = "very.long.team.member.name+ferryx-monetization@example-corporation.test";
    stubFetch(requests, (url) => {
      if (url.includes("/org/members")) {
        return jsonResponse([{ userId: "u-long", email: longEmail, role: "member" }]);
      }
      return jsonResponse(
        wireEntitlement({
          plan: "team_annual",
          machineLimit: 20,
          machinesUsed: 3,
          seats: 2,
          orgId: "org-1",
          role: "owner",
        }),
      );
    });

    const membersSignal = observeRequests(requests, "/org/members", 1);
    const summarySignal = observeDom(() => screen.queryByTestId("plan-summary"));
    const memberSignal = observeDom(() => screen.queryByTestId("plan-member-row"));
    renderPlan();
    await flushFetch();
    await settle(summarySignal);
    await settle(membersSignal);
    await flushFetch();
    const row = await settle(memberSignal);
    expect(row).toHaveAttribute("data-user-id", "u-long");
    expect(within(row).getByRole("button", { name: `Remove ${longEmail}` })).toBeInTheDocument();
    expect(within(row).queryByRole("button", { name: /^Confirm/ })).toBeNull();
  });

  it("opens the manage url when the account carries one", async () => {
    stubFetch([], () =>
      jsonResponse(
        wireEntitlement({ plan: "pro_monthly", machineLimit: 10, manageUrl: CHECKOUT_URL }),
      ),
    );

    const opened = deferred<string>();
    browserNative.openExternalUrl.mockImplementation(async (url: string) => {
      opened.resolve(url);
    });

    const summarySignal = observeDom(() => screen.queryByTestId("plan-summary"));
    renderPlan();
    await flushFetch();
    await settle(summarySignal);

    fireEvent.click(screen.getByRole("button", { name: /Manage billing/ }));
    await flushFetch();

    expect(await settle(opened)).toBe(CHECKOUT_URL);
  });
});
