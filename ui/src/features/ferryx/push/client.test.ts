import { describe, expect, it, vi } from "vitest";
import { PushClient, secureTaskLink } from "./client";
describe("push client boundaries", () => {
  it("accepts only same origin exact task links", () => {
    expect(secureTaskLink("/#task=YWJj", "https://ferryx.test")).toBe("https://ferryx.test/#task=YWJj");
    for (const link of ["https://evil.test/#task=YWJj", "//evil.test/#task=YWJj", "/#pair=secret", "/#task=", "/?token=secret#task=YWJj"]) expect(secureTaskLink(link, "https://ferryx.test")).toBeNull();
  });
  it("denied permission never subscribes", async () => {
    vi.stubGlobal("isSecureContext", true); vi.stubGlobal("Notification", { permission: "denied", requestPermission: vi.fn() });
    const subscribe = vi.fn(); const client = new PushClient({request: vi.fn()}, {pushManager: {subscribe}} as unknown as ServiceWorkerRegistration);
    expect(await client.enable()).toBe("denied"); expect(subscribe).not.toHaveBeenCalled(); vi.unstubAllGlobals();
  });
  it("server unsubscribe precedes local removal and failure preserves subscription", async () => {
    let removed = false; const subscription = { endpoint: "https://push.test/a", unsubscribe: async () => { removed = true; return true; } };
    const client = new PushClient({request: async () => { throw new Error("offline"); }}, {pushManager: {getSubscription: async () => subscription}} as unknown as ServiceWorkerRegistration);
    await expect(client.disable()).rejects.toThrow("offline"); expect(removed).toBe(false);
  });
  it("enable registers subscription with backend", async () => {
    const apiRequest = vi.fn().mockResolvedValue({ ok: true });
    const fakeSub = { endpoint: "https://push.test/sub", toJSON: () => ({ endpoint: "https://push.test/sub" }) };
    const client = new PushClient({ request: apiRequest }, {
      pushManager: {
        getSubscription: vi.fn().mockResolvedValue(fakeSub),
        subscribe: vi.fn(),
      }
    } as unknown as ServiceWorkerRegistration);
    const state = await client.enable();
    expect(state).toBe("enabled");
    expect(apiRequest).toHaveBeenCalledWith("/push/subscribe", fakeSub);
  });
});
